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

pub struct AudioDeviceSettings {
	echo: bool
}

impl AudioDeviceSettings {
	pub fn new() -> AudioDeviceSettings {
		AudioDeviceSettings {
			echo: false
		}
	}
}

pub struct MidiDeviceSettings {
	start_transport_pending: bool,
	stop_transport_pending: bool
}

impl MidiDeviceSettings {
	pub fn new() -> MidiDeviceSettings {
		MidiDeviceSettings {
			start_transport_pending: true,
			stop_transport_pending: true
		}
	}
}

pub struct AudioThreadState {
	devices: Vec<Option<(AudioDevice, AudioDeviceSettings)>>,
	mididevices: Vec<Option<(MidiDevice, MidiDeviceSettings)>>,
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
			devices: pad_option_vec(audiodevices.into_iter().map(|d| (d, AudioDeviceSettings::new())), 32),
			mididevices: pad_option_vec(mididevices.into_iter().map(|d| (d, MidiDeviceSettings::new())), 32),
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
						Message::UpdateAudioDevice(id, device) => {
							// FrontendThreadState has verified that audiodev_id isn't currently used by any take
							if cfg!(debug_assertions) {
								for take in self.audiotakes.iter() {
									debug_assert!(take.take.borrow().audiodev_id != id);
								}
							}
							
							let mut devtuple = device.map(|d| (d, AudioDeviceSettings::new()));
							std::mem::swap(&mut self.devices[id], &mut devtuple);
							
							if let Some((old, _)) = devtuple {
								println!("submitting deconstruction request");
								if self.destructor_channel.push(DestructionRequest::AudioDevice(old)).is_err() {
									panic!("Failed to submit deconstruction request");
								}
								self.destructor_thread_handle.thread().unpark();
							}
						}
						Message::UpdateMidiDevice(id, device) => {
							// FrontendThreadState has verified that audiodev_id isn't currently used by any take
							if cfg!(debug_assertions) {
								for take in self.miditakes.iter() {
									debug_assert!(take.take.borrow().mididev_id != id);
								}
							}

							let mut devtuple = device.map(|d| (d, MidiDeviceSettings::new()));
							std::mem::swap(&mut self.mididevices[id], &mut devtuple);

							if let Some((old, _)) = devtuple {
								println!("submitting deconstruction request");
								if self.destructor_channel.push(DestructionRequest::MidiDevice(old)).is_err() {
									panic!("Failed to submit deconstruction request");
								}
								self.destructor_thread_handle.thread().unpark();
							}
						}
						Message::SetAudioEcho(id, echo) => {
							self.devices[id].as_mut().unwrap().1.echo = echo;
						}
						Message::RestartMidiTransport(id) => {
							self.mididevices[id].as_mut().unwrap().1.start_transport_pending = true;
							self.mididevices[id].as_mut().unwrap().1.stop_transport_pending = true;
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
						Message::FinishMidiTake(id, length) => { // TODO duplicated code
							for_take!(&mut self.miditakes, id, t -> {
								t.length = Some(length);
								if t.playback_position >= length {
									let target_position = t.playback_position % length;
									t.seek(target_position); // TODO this is heavy work and might cause glitches. maybe slow-seek over multiple periods
								}
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

	fn process_audio_playback(&mut self, scope: &jack::ProcessScope) {
		for dev in self.devices.iter_mut() {
			if let Some(d) = dev {
				if d.1.echo {
					play_echo(scope, &mut d.0);
				}
				else {
					play_silence(scope,&mut d.0,0..scope.n_frames());
				}
			}
		}

		let mut cursor = self.audiotakes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			let dev = self.devices[t.audiodev_id].as_mut().unwrap();
			t.playback(scope, &mut dev.0, 0..scope.n_frames()); // handles finishing recording and wrapping around.
			cursor.move_next();
		}
	}

	fn process_midi_playback(&mut self, scope: &jack::ProcessScope) {
		let mut cursor = self.miditakes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			let dev = &mut self.mididevices[t.mididev_id].as_mut().unwrap().0;
			t.playback(dev, 0..scope.n_frames()); // handles finishing recording and wrapping around.
			cursor.move_next();
		}

		for d in self.mididevices.iter_mut() {
			if let Some((dev,data)) = d {
				if data.stop_transport_pending {
					dev.queue_event( crate::midi_message::MidiMessage {
						timestamp: 0,
						data: [0xFC, 0, 0],
						datalen: 1
					});
					data.stop_transport_pending = false;
				}
				if data.start_transport_pending {
					let time_until_action = self.song_length - (self.song_position + dev.capture_latency()) % self.song_length;
					if time_until_action < scope.n_frames() {
						dev.queue_event( crate::midi_message::MidiMessage {
							timestamp: time_until_action,
							data: [0xFA, 0, 0],
							datalen: 1
						});
						data.start_transport_pending = false;
					}
				}
				dev.commit_out_buffer(scope);
			}
		}
	}


	fn process_audio_recording(&mut self, scope: &jack::ProcessScope) {
		use RecordState::*;
		let mut cursor = self.audiotakes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			let dev = &mut self.devices[t.audiodev_id].as_mut().unwrap().0;
			
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
			let dev = &self.mididevices[t.mididev_id].as_ref().unwrap().0;
		
			let (song_wraps, song_wraps_at) = check_wrap(
				self.song_position as i32 - dev.capture_latency() as i32,
				self.song_length, scope.n_frames() );

			if t.record_state == Recording {
				t.record(scope,dev, 0..scope.n_frames());

				if let Some(length) = t.length {
					if t.recorded_length >= length {
						println!("\nFinished recording on device {}", t.mididev_id);
						self.event_channel.send_or_complain(Event::MidiTakeStateChanged(t.mididev_id, t.id, RecordState::Finished, self.transport_position + song_wraps_at));
						t.record_state = Finished;
					}
				}
			}
			else if t.record_state == Waiting {
				if song_wraps {
					println!("\nStarted recording on device {}", t.mididev_id);
					self.event_channel.send_or_complain(Event::MidiTakeStateChanged(t.mididev_id, t.id, RecordState::Recording, self.transport_position + song_wraps_at));
					t.record_state = Recording;
					t.started_recording_at = self.transport_position + song_wraps_at;
					t.start_recording(scope, dev, 0..song_wraps_at);
					t.recorded_length = 0;
					t.record(scope, dev, song_wraps_at..scope.n_frames());
					t.playback_position = scope.n_frames()-song_wraps_at + dev.capture_latency() + dev.playback_latency();
				}
			}

			cursor.move_next();
		}

		for dev_opt in self.mididevices.iter_mut() {
			if let Some((dev, _data)) = dev_opt {
				dev.update_registry(scope);
			}
		}
	}
}

fn play_echo<'a, T: AudioDeviceTrait>(scope: &'a T::Scope, device: &'a mut T) {
	for (output, input) in device.playback_and_capture_buffers(scope) {
		output.copy_from_slice(input);
	}
}

fn play_silence<'a, T: AudioDeviceTrait>(scope: &'a T::Scope, device: &'a mut T, range_u32: std::ops::Range<u32>) {
	let range = range_u32.start as usize .. range_u32.end as usize;
	for channel_slices in device.playback_and_capture_buffers(scope) {
		let buffer = &mut channel_slices.0[range.clone()];
		for d in buffer {
			*d = 0.0;
		}
	}
}

fn pad_option_vec<T: Iterator>(iter: T, size: usize) -> Vec<Option<T::Item>> {
	iter.map(|v| Some(v))
		.chain( (0..).map(|_| None) )
		.take(size)
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
