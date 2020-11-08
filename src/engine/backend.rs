use super::takes::{AudioTakeAdapter,MidiTakeAdapter};
use super::data::Event;
use super::shared::SharedThreadState;
use super::data::*;
use super::messages::*;

use core::cmp::min;
use intrusive_collections::LinkedList;
use std::sync::Arc;
use crate::jack_driver::*;

use crate::metronome::AudioMetronome;


use assert_no_alloc::assert_no_alloc;
use crate::realtime_send_queue;

pub struct AudioThreadState {
	devices: Vec<Option<AudioDevice>>,
	mididevices: Vec<Option<MidiDevice>>,
	metronome: AudioMetronome,
	audiotakes: LinkedList<AudioTakeAdapter>,
	miditakes: LinkedList<MidiTakeAdapter>,
	command_channel: ringbuf::Consumer<Message>,
	transport_position: u32, // does not wrap 
	song_position: u32, // wraps
	song_length: u32,
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
	pub fn new(audiodevices: Vec<AudioDevice>, mididevices: Vec<MidiDevice>, metronome: AudioMetronome, command_channel: ringbuf::Consumer<Message>, song_length: u32, shared: Arc<SharedThreadState>, event_channel: realtime_send_queue::Producer<Event>) -> AudioThreadState
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
			audiotakes: LinkedList::new(AudioTakeAdapter::new()),
			miditakes: LinkedList::new(MidiTakeAdapter::new()),
			command_channel,
			transport_position: 0,
			song_position: 0,
			song_length,
			shared,
			event_channel,
			destructor_thread_handle,
			destructor_channel: destruction_sender
		}
	}

	pub fn process_callback(&mut self, _client: &jack::Client, scope: &jack::ProcessScope) -> jack::Control {
		use RecordState::*;
		assert_no_alloc(||{
			assert!(scope.n_frames() < self.song_length);

			self.metronome.process(self.song_position, self.song_length / 8, 4, scope);

			self.process_command_channel();

			self.process_audio_playback(scope);
			self.process_midi_playback(scope);

			self.process_audio_recording(scope);
			self.process_midi_recording(scope);

			self.song_position = (self.song_position + scope.n_frames()) % self.song_length;
			self.transport_position += scope.n_frames();

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
						Message::NewAudioTake(take) => { println!("\ngot take"); self.audiotakes.push_back(take); }
						Message::NewMidiTake(take) => { println!("\ngot miditake"); self.miditakes.push_back(take); }
						Message::SetAudioMute(id, muted) => {
							// FIXME this is not nice...
							let mut cursor = self.audiotakes.front();
							while let Some(node) = cursor.get() {
								let mut t = node.take.borrow_mut();
								if t.id == id {
									t.unmuted = !muted;
									break;
								}
								cursor.move_next();
							}
							if cursor.get().is_none() {
								panic!("could not find take to mute");
							}
						}
						Message::SetMidiMute(id, muted) => {
							// FIXME this is not nice...
							let mut cursor = self.miditakes.front();
							while let Some(node) = cursor.get() {
								let mut t = node.take.borrow_mut();
								if t.id == id {
									t.unmuted = !muted;
									break;
								}
								cursor.move_next();
							}
							if cursor.get().is_none() {
								panic!("could not find miditake to mute");
							}
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
				play_silence(scope,d,0..scope.n_frames() as usize);
			}
		}
		
		let mut cursor = self.audiotakes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			let dev = self.devices[t.audiodev_id].as_mut().unwrap();
			// we assume that all channels have the same latencies.
			let playback_latency = dev.channels[0].out_port.get_latency_range(jack::LatencyType::Playback).1;

			let song_position = (self.song_position + self.song_length + playback_latency) % self.song_length;
			let song_position_after = song_position + scope.n_frames();
			let song_wraps = self.song_length <= song_position_after;
			let song_wraps_at = min(self.song_length - song_position, scope.n_frames()) as usize;

			if t.playing {
				t.playback(scope,dev, 0..scope.n_frames() as usize);
				if song_wraps { println!("\n10/10 would rewind\n"); }
			}
			else if t.record_state == RecordState::Recording {
				if song_wraps {
					t.playing = true;
					println!("\nAlmost finished recording on device {}, thus starting playback now", t.audiodev_id);
					println!("Recording started at {}, now is {}", t.started_recording_at, self.transport_position + song_wraps_at as u32);
					t.rewind();
					t.playback(scope,dev, song_wraps_at..scope.n_frames() as usize);
				}
			}

			cursor.move_next();
		}
	}

	fn process_midi_playback(&mut self, scope: &jack::ProcessScope) {
		let mut cursor = self.miditakes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			let dev = self.mididevices[t.mididev_id].as_mut().unwrap();
			let playback_latency = dev.out_port.get_latency_range(jack::LatencyType::Playback).1;

			let song_position = (self.song_position + self.song_length + playback_latency) % self.song_length;
			let song_position_after = song_position + scope.n_frames();
			let song_wraps = self.song_length <= song_position_after;
			let song_wraps_at = min(self.song_length - song_position, scope.n_frames()) as usize;
			
			if t.playing {
				t.playback(dev, 0..scope.n_frames() as usize);
				if song_wraps { println!("\n10/10 would rewind\n"); }
			}
			else if t.record_state == RecordState::Recording {
				if song_wraps {
					t.playing = true;
					println!("\nAlmost finished recording on midi device {}, thus starting playback now", t.mididev_id);
					println!("Recording started at {}, now is {}", t.started_recording_at, self.transport_position + song_wraps_at as u32);
					t.rewind();
					t.playback(dev, song_wraps_at..scope.n_frames() as usize);
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
			// we assume that all channels have the same latencies.
			let capture_latency = dev.channels[0].in_port.get_latency_range(jack::LatencyType::Capture).1;
		
			let song_position = (self.song_position + self.song_length - capture_latency) % self.song_length;
			let song_position_after = song_position + scope.n_frames();
			let song_wraps = self.song_length <= song_position_after;
			let song_wraps_at = min(self.song_length - song_position, scope.n_frames());

			
			if t.record_state == Recording {
				t.record(scope,dev, 0..song_wraps_at as usize);

				if song_wraps {
					println!("\nFinished recording on device {}", t.audiodev_id);
					self.event_channel.send_or_complain(Event::AudioTakeStateChanged(t.audiodev_id, t.id, RecordState::Finished));
					t.record_state = Finished;
				}
			}
			else if t.record_state == Waiting {
				if song_wraps {
					println!("\nStarted recording on device {}", t.audiodev_id);
					self.event_channel.send_or_complain(Event::AudioTakeStateChanged(t.audiodev_id, t.id, RecordState::Recording));
					t.record_state = Recording;
					t.started_recording_at = self.transport_position + song_wraps_at;
					t.record(scope, dev, song_wraps_at as usize ..scope.n_frames() as usize);
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
			// we assume that all channels have the same latencies.
			let capture_latency = dev.in_port.get_latency_range(jack::LatencyType::Capture).1;
		
			let song_position = (self.song_position + self.song_length - capture_latency) % self.song_length;
			let song_position_after = song_position + scope.n_frames();
			let song_wraps = self.song_length <= song_position_after;
			let song_wraps_at = min(self.song_length - song_position, scope.n_frames());

			
			if t.record_state == Recording {
				t.record(scope,dev, 0..song_wraps_at as usize);

				if song_wraps {
					println!("\nFinished recording on device {}", t.mididev_id);
					self.event_channel.send_or_complain(Event::MidiTakeStateChanged(t.mididev_id, t.id, RecordState::Finished));
					t.record_state = Finished;
				}
			}
			else if t.record_state == Waiting {
				if song_wraps {
					println!("\nStarted recording on device {}", t.mididev_id);
					self.event_channel.send_or_complain(Event::MidiTakeStateChanged(t.mididev_id, t.id, RecordState::Recording));
					t.record_state = Recording;
					t.started_recording_at = self.transport_position + song_wraps_at;
					t.record(scope, dev, song_wraps_at as usize ..scope.n_frames() as usize);
				}
			}

			cursor.move_next();
		}
	}
}

fn play_silence(scope: &jack::ProcessScope, device: &mut AudioDevice, range: std::ops::Range<usize>) {
	for channel_ports in device.channels.iter_mut() {
		let buffer = &mut channel_ports.out_port.as_mut_slice(scope)[range.clone()];
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

