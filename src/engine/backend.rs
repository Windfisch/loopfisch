use super::takes::{AudioTakeAdapter,MidiTakeAdapter};
use super::data::Event;
use super::shared::SharedThreadState;
use super::data::*;
use super::messages::*;

use core::cmp::min;
use intrusive_collections::LinkedList;
use std::sync::Arc;
use super::jack_driver::*;
use super::driver_traits::*;

use super::metronome::AudioMetronome;
use super::midiclock::MidiClock;


use assert_no_alloc::assert_no_alloc;
use crate::realtime_send_queue;

fn for_first<T: intrusive_collections::Adapter, R>(
	list: &mut LinkedList<T>,
	func: impl Fn (&<<T as intrusive_collections::Adapter>::PointerOps as intrusive_collections::PointerOps>::Value)->Option<R>
) -> Result<R, ()>
where T::LinkOps: intrusive_collections::linked_list::LinkedListOps, 
{
	let mut cursor = list.front();
	while let Some(node) = cursor.get() {
		if let Some(result) = func(&node) {
			return Ok(result);
		}
		cursor.move_next();
	}
	Err(())
}

macro_rules! for_take {
	($list:expr, $id:expr, $take:ident -> $code:block) => {{
		let id = $id;
		for_first($list, |node| {
			let mut $take = node.take.borrow_mut();
			if $take.id == id {
				return {
					$code
				};
			}
			None
		})
	}}
}

pub struct AudioThreadState {
	devices: Vec<Option<AudioDevice>>,
	mididevices: Vec<Option<MidiDevice>>,
	metronome: AudioMetronome<AudioDevice>,
	midiclock: MidiClock<MidiDevice>,
	audiotakes: LinkedList<AudioTakeAdapter>,
	miditakes: LinkedList<MidiTakeAdapter>,
	command_channel: ringbuf::Consumer<Message>,
	transport_position: u32, // does not wrap 
	song_position: u32, // wraps
	song_length: u32,
	n_beats: u32,
	shared: Arc<SharedThreadState>,
	event_channel: realtime_send_queue::Producer<Event>,
	destructor_thread_handle: std::thread::JoinHandle<()>,
	destructor_channel: ringbuf::Producer<DestructionRequest>
}

impl Drop for AudioThreadState {
	fn drop(&mut self) {
		println!("\n\n\n############# Dropping AudioThreadState\n\n\n");
		self.event_channel.send(Event::Kill).ok();
		self.destructor_channel.push(DestructionRequest::End).ok();
	}
}

impl AudioThreadState {
	// FIXME this function signature sucks
	pub fn new(audiodevices: Vec<AudioDevice>, mididevices: Vec<MidiDevice>, metronome: AudioMetronome<AudioDevice>, midiclock: MidiClock<MidiDevice>, command_channel: ringbuf::Consumer<Message>, song_length: u32, shared: Arc<SharedThreadState>, event_channel: realtime_send_queue::Producer<Event>) -> AudioThreadState
	{
		let (destruction_sender, mut destruction_receiver) = ringbuf::RingBuffer::<DestructionRequest>::new(32).split();
		let destructor_thread_handle = std::thread::spawn(move || {
			loop {
				std::thread::park();
				println!("Handling deconstruction request");
				while let Some(request) = destruction_receiver.pop() {
					match request {
						DestructionRequest::AudioDevice(dev) => std::mem::drop(dev),
						DestructionRequest::MidiDevice(dev) => std::mem::drop(dev),
						DestructionRequest::End => {println!("destructor thread exiting..."); break;}
					}
				}
			}
		});

		AudioThreadState {
			devices: pad_option_vec(audiodevices, 32),
			mididevices: pad_option_vec(mididevices, 32),
			metronome,
			midiclock,
			audiotakes: LinkedList::new(AudioTakeAdapter::new()),
			miditakes: LinkedList::new(MidiTakeAdapter::new()),
			command_channel,
			transport_position: 0,
			song_position: 0,
			song_length,
			n_beats: 4,
			shared,
			event_channel,
			destructor_thread_handle,
			destructor_channel: destruction_sender
		}
	}

	pub fn process_callback(&mut self, client: &jack::Client, scope: &jack::ProcessScope) -> jack::Control {
		assert_no_alloc(||{
			assert!(scope.n_frames() < self.song_length);

			self.metronome.process(self.song_position, self.song_length, self.n_beats, client.sample_rate() as u32, scope);
			self.midiclock.process(self.song_position, self.song_length, self.n_beats, scope);

			self.process_command_channel();

			self.process_audio_playback(scope);
			self.process_midi_playback(scope);

			self.process_audio_recording(scope);
			self.process_midi_recording(scope);

			self.song_position = self.song_position + scope.n_frames();
			let song_wraps = self.song_position >= self.song_length;
			self.song_position %= self.song_length;
			self.transport_position += scope.n_frames();

			if song_wraps {
				println!("song wraps");
				self.event_channel.send_or_complain(Event::Timestamp(self.song_position, self.transport_position));
			}

			self.shared.song_length.store(self.song_length, std::sync::atomic::Ordering::Relaxed);
			self.shared.song_position.store(self.song_position, std::sync::atomic::Ordering::Relaxed);
			self.shared.transport_position.store(self.transport_position, std::sync::atomic::Ordering::Relaxed);
		});

		jack::Control::Continue
	}

	fn process_command_channel(&mut self) {
		loop {
			match self.command_channel.pop() {
				Some(msg) => {
					match msg {
						Message::SetSongLength(song_length, n_beats) => {
							assert!(self.audiotakes.is_empty() && self.miditakes.is_empty());
							self.song_length = song_length;
							self.n_beats = n_beats;
						}
						Message::UpdateAudioDevice(id, mut device) => {
							// FrontendThreadState has verified that audiodev_id isn't currently used by any take
							if cfg!(debug_assertions) {
								for take in self.audiotakes.iter() {
									debug_assert!(take.take.borrow().audiodev_id != id);
								}
							}

							std::mem::swap(&mut self.devices[id], &mut device);
							
							if let Some(old) = device {
								println!("submitting deconstruction request");
								if self.destructor_channel.push(DestructionRequest::AudioDevice(old)).is_err() {
									panic!("Failed to submit deconstruction request");
								}
								self.destructor_thread_handle.thread().unpark();
							}
						}
						Message::UpdateMidiDevice(id, mut device) => {
							// FrontendThreadState has verified that audiodev_id isn't currently used by any take
							if cfg!(debug_assertions) {
								for take in self.miditakes.iter() {
									debug_assert!(take.take.borrow().mididev_id != id);
								}
							}

							std::mem::swap(&mut self.mididevices[id], &mut device);

							if let Some(old) = device {
								println!("submitting deconstruction request");
								if self.destructor_channel.push(DestructionRequest::MidiDevice(old)).is_err() {
									panic!("Failed to submit deconstruction request");
								}
								self.destructor_thread_handle.thread().unpark();
							}
						}
						Message::NewAudioTake(take) => {
							println!("\ngot take");
							self.audiotakes.push_back(take);
						}
						Message::NewMidiTake(take) => { println!("\ngot miditake"); self.miditakes.push_back(take); }
						Message::FinishAudioTake(id, length) => {
							for_take!(&mut self.audiotakes, id, t -> {
								t.length = Some(length);
								if t.playback_position >= length {
									let target_position = t.playback_position % length;
									t.seek(target_position); // TODO this is heavy work and might cause glitches. maybe slow-seek over multiple periods
								}
								Some(())
							}).expect("could not find take to mute");
						}
						Message::FinishMidiTake(id, length) => {
							unimplemented!();
							for_take!(&mut self.miditakes, id, t -> {
								//t.length = length;
								Some(())
							}).expect("could not find take to mute");
						}
						Message::SetAudioMute(id, unmuted) => {
							for_take!(&mut self.audiotakes, id, t -> {
								t.unmuted = unmuted;
								Some(())
							}).expect("could not find take to mute");
						}
						Message::SetMidiMute(id, unmuted) => {
							for_take!(&mut self.miditakes, id, t -> {
								t.unmuted = unmuted;
								Some(())
							}).expect("could not find take to mute");
						}
						_ => { unimplemented!() }
					}
				}
				None => { break; }
			}
		}
	}

	/** play all playing audio takes and start playback for those that are just leaving `Recording` state */
	fn process_audio_playback(&mut self, scope: &jack::ProcessScope) {
		for dev in self.devices.iter_mut() {
			if let Some(d) = dev {
				play_silence(scope,d,0..scope.n_frames());
			}
		}
		
		let mut cursor = self.audiotakes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			let dev = self.devices[t.audiodev_id].as_mut().unwrap();
			t.playback(scope,dev, 0..scope.n_frames()); // handles finishing recording and wrapping around.
			cursor.move_next();
		}
	}

	fn process_midi_playback(&mut self, scope: &jack::ProcessScope) {
		let mut cursor = self.miditakes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			let dev = self.mididevices[t.mididev_id].as_mut().unwrap();

			let (song_wraps, song_wraps_at) = check_wrap(
				self.song_position as i32 + dev.playback_latency() as i32,
				self.song_length, scope.n_frames() );

			if t.playing {
				t.playback(dev, 0..scope.n_frames());
				if song_wraps { println!("\n10/10 would rewind\n"); }
			}
			else if t.record_state == RecordState::Recording {
				if song_wraps {
					t.playing = true;
					println!("\nAlmost finished recording on midi device {}, thus starting playback now", t.mididev_id);
					println!("Recording started at {}, now is {}", t.started_recording_at, self.transport_position + song_wraps_at);
					t.rewind();
					t.playback(dev, song_wraps_at..scope.n_frames());
				}
			}

			cursor.move_next();
		}
		
		for dev in self.mididevices.iter_mut() {
			if let Some(d) = dev {
				d.commit_out_buffer(scope);
			}
		}
	}


	fn process_audio_recording(&mut self, scope: &jack::ProcessScope) {
		use RecordState::*;
		let mut cursor = self.audiotakes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			let dev = self.devices[t.audiodev_id].as_ref().unwrap();
			
			let (song_wraps, song_wraps_at) = check_wrap(
				self.song_position as i32 - dev.capture_latency() as i32,
				self.song_length, scope.n_frames() );

			if t.record_state == Recording {
				t.record(scope,dev, 0..scope.n_frames());

				if let Some(length) = t.length {
					if t.recorded_length >= length {
						println!("\nFinished recording on device {}", t.audiodev_id);
						self.event_channel.send_or_complain(Event::AudioTakeStateChanged(t.audiodev_id, t.id, RecordState::Finished, self.transport_position + song_wraps_at));
						t.record_state = Finished;
					}
				}
			}
			else if t.record_state == Waiting {
				if song_wraps {
					println!("\nStarted recording on device {}", t.audiodev_id);
					self.event_channel.send_or_complain(Event::AudioTakeStateChanged(t.audiodev_id, t.id, RecordState::Recording, self.transport_position + song_wraps_at));
					t.record_state = Recording;
					t.started_recording_at = self.transport_position + song_wraps_at;
					t.recorded_length = 0;
					t.record(scope, dev, song_wraps_at..scope.n_frames());
					t.playback_position = scope.n_frames()-song_wraps_at + dev.capture_latency() + dev.playback_latency();
				}
			}

			cursor.move_next();
		}
	}

	fn process_midi_recording(&mut self, scope: &jack::ProcessScope) {
		use RecordState::*;
		let mut cursor = self.miditakes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			let dev = self.mididevices[t.mididev_id].as_ref().unwrap();
		
			let (song_wraps, song_wraps_at) = check_wrap(
				self.song_position as i32 - dev.capture_latency() as i32,
				self.song_length, scope.n_frames() );

			if t.record_state == Recording {
				t.record(scope,dev, 0..song_wraps_at);

				if song_wraps {
					println!("\nFinished recording on device {}", t.mididev_id);
					t.finish_recording(scope, dev, 0..song_wraps_at);
					self.event_channel.send_or_complain(Event::MidiTakeStateChanged(t.mididev_id, t.id, RecordState::Finished, self.transport_position + song_wraps_at));
					t.record_state = Finished;
				}
			}
			else if t.record_state == Waiting {
				if song_wraps {
					println!("\nStarted recording on device {}", t.mididev_id);
					self.event_channel.send_or_complain(Event::MidiTakeStateChanged(t.mididev_id, t.id, RecordState::Recording, self.transport_position + song_wraps_at));
					t.record_state = Recording;
					t.started_recording_at = self.transport_position + song_wraps_at;
					t.start_recording(scope, dev, 0..song_wraps_at);
					t.record(scope, dev, song_wraps_at..scope.n_frames());
				}
			}

			cursor.move_next();
		}

		for dev_opt in self.mididevices.iter_mut() {
			if let Some(dev) = dev_opt {
				dev.update_registry(scope);
			}
		}
	}
}

fn play_silence<'a, T: AudioDeviceTrait>(scope: &'a T::Scope, device: &'a mut T, range_u32: std::ops::Range<u32>) {
	let range = range_u32.start as usize .. range_u32.end as usize;
	for channel_slice in device.playback_buffers(scope) {
		let buffer = &mut channel_slice[range.clone()];
		for d in buffer {
			*d = 0.0;
		}
	}
}

fn pad_option_vec<T>(vec: Vec<T>, size: usize) -> Vec<Option<T>> {
	let n = vec.len();
	vec.into_iter().map(|v| Some(v))
		.chain( (n..size).map(|_| None) )
		.collect()
}

/** Given a audio chunk length of `n_frames`, returns whether and at which chunk sample position
  * the song with length `song_length` will wrap around. */
fn check_wrap(song_position: i32, song_length: u32, n_frames: u32) -> (bool, u32) {
	let pos = modulo(song_position, song_length);
	let wraps = pos + n_frames >= song_length;
	let wraps_at = min(song_length - pos, n_frames);
	return (wraps, wraps_at);
}

fn modulo(number: i32, modulo_u32: u32) -> u32 {
	let modulo = modulo_u32 as i32;
	(((number % modulo) + modulo) % modulo) as u32
}
