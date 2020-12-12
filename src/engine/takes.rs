use super::data::RecordState;

use intrusive_collections::{intrusive_adapter, LinkedListLink};
use std::cell::RefCell;

use crate::midi_message::MidiMessage;

use super::driver_traits::*;

use super::midi_registry::MidiNoteRegistry;

use crate::outsourced_allocation_buffer::Buffer;


pub struct AudioTake {
	/// Sequence of all samples. The take's duration and playhead position are implicitly managed by the underlying Buffer.
	pub samples: Vec<Buffer<f32>>,
	pub length: Option<u32>, // FIXME rename this in playback_length
	pub recorded_length: u32,
	pub record_state: RecordState,
	pub playback_position: u32,
	pub id: u32,
	pub audiodev_id: usize,
	pub unmuted: bool,
	pub started_recording_at: u32,
}

impl std::fmt::Debug for AudioTake {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AudioTake")
			.field("record_state", &self.record_state)
			.field("id", &self.id)
			.field("audiodev_id", &self.audiodev_id)
			.field("unmuted", &self.unmuted)
			.field("started_recording_at", &self.started_recording_at)
			.field("channels", &self.samples.len())
			.field("samples", &if self.samples[0].empty() { "<Empty>".to_string() } else { "[...]".to_string() })
			.finish()
	}
}

impl AudioTake {
	pub fn playback<T: AudioDeviceTrait>(&mut self, scope: &T::Scope, device: &mut T, range_u32: std::ops::Range<u32>) {
		if let Some(length) = self.length {
			let range = range_u32.start as usize .. range_u32.end as usize;
			for (channel_buffer, channel_slice) in self.samples.iter_mut().zip(device.playback_buffers(scope)) {
				let mut position = self.playback_position;
				let buffer = &mut channel_slice[range.clone()];
				for d in buffer {
					if position >= length {
						channel_buffer.rewind();
						position = 0;
						println!("\nrewind in playback\n");
					}
					position += 1;

					let val = channel_buffer.next(|v|*v);
					if let Some(v) = val {
						if self.unmuted { // FIXME fade in / out to avoid clicks
							*d += v;
						}
					}
				}
			}
		}

		self.playback_position += range_u32.len() as u32;
		
		if let Some(length) = self.length {
			self.playback_position %= length;
		}
	}

	pub fn seek(&mut self, position: u32) {
		if position < self.playback_position {
			self.rewind();
		}
		assert!(position >= self.playback_position);

		let difference = position - self.playback_position;
		for channel_buffer in self.samples.iter_mut() {
			for _ in 0..difference {
				channel_buffer.next(|_|{});
			}
		}

		self.playback_position = position;
	}
	
	pub fn rewind(&mut self) {
		for channel_buffer in self.samples.iter_mut() {
			channel_buffer.rewind();
		}
		self.playback_position = 0;
	}

	pub fn record<T: AudioDeviceTrait>(&mut self, scope: &T::Scope, device: &T, range_u32: std::ops::Range<u32>) {
		let range = range_u32.start as usize .. range_u32.end as usize;
		for (channel_buffer, channel_slice) in self.samples.iter_mut().zip(device.record_buffers(scope)) {
			let data = &channel_slice[range.clone()];
			for d in data {
				let err = channel_buffer.push(*d).is_err();
				if err {
					// FIXME proper error handling, such as marking the take as stale, dropping it.
					panic!("Failed to add audio sample to already-full sample queue!");
				}
			}
		}
		self.recorded_length += range.len() as u32;
	}
}

pub struct MidiTake {
	/// Sorted sequence of all events with timestamps between 0 and self.recorded_length
	pub events: Buffer<MidiMessage>,
	/// Current playhead position
	pub playback_position: u32,
	/// Number of frames after which the recorded events shall loop.
	pub recorded_length: u32,
	pub record_state: RecordState,
	pub length: Option<u32>, // FIXME rename this in playback_length
	pub id: u32,
	pub mididev_id: usize,
	pub unmuted: bool,
	pub unmuted_old: bool,
	pub started_recording_at: u32,
	pub note_registry: RefCell<MidiNoteRegistry>, // this RefCell here SUCKS. TODO.
	pub is_post_rewind_action_pending: bool
}

impl std::fmt::Debug for MidiTake {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("MidiTake")
			.field("record_state", &self.record_state)
			.field("id", &self.id)
			.field("mididev_id", &self.mididev_id)
			.field("unmuted", &self.unmuted)
			.field("started_recording_at", &self.started_recording_at)
			.field("events", &if self.events.empty() { "<Empty>".to_string() } else { "[...]".to_string() })
			.finish()
	}
}



impl MidiTake {
	fn handle_mute_change(&mut self, device: &mut impl MidiDeviceTrait) {
		if self.unmuted != self.unmuted_old {
			if self.unmuted {
				self.note_registry.borrow_mut().send_noteons(device);
			}
			else {
				self.note_registry.borrow_mut().send_noteoffs(device);
			}
			self.unmuted_old = self.unmuted;
		}
	}

	/// Enumerates all events that take place in the next `range.len()` frames and puts
	/// them into device's playback queue. The events are automatically looped every
	/// `self.length` frames.
	pub fn playback(&mut self, device: &mut impl MidiDeviceTrait, range: std::ops::Range<u32>) {
		if let Some(length) = self.length {

			self.handle_mute_change(device);

			let curr_pos = self.playback_position;
			let unmuted = self.unmuted;
			let mut rewind_offset = 0;
			loop {
				let mut note_registry = self.note_registry.borrow_mut(); // TODO this SUCKS! oh god why, rust. this whole callback thing is garbage.

				if self.is_post_rewind_action_pending {
					let relative_timestamp = rewind_offset - curr_pos + range.start;
					if relative_timestamp < range.end {
						if unmuted {
							note_registry.send_noteoffs_at(device, relative_timestamp);
						}
						note_registry.clear();
						self.is_post_rewind_action_pending = false;
					}
				}
				enum Outcome {
					RewindAndContinue,
					Break,
					NextAndContinue
				}
				let result = self.events.peek( |event| {
					if event.timestamp > length {
						return Outcome::RewindAndContinue;
					}

					assert!(event.timestamp + rewind_offset >= curr_pos);
					let relative_timestamp = event.timestamp + rewind_offset - curr_pos + range.start;
					assert!(relative_timestamp >= range.start);

					if relative_timestamp >= range.end {
						return Outcome::Break;
					}
					
					if unmuted {
						device.queue_event(
							MidiMessage {
								timestamp: relative_timestamp,
								data: event.data,
								datalen: event.datalen
							}
						).unwrap();
					}
					note_registry.register_event(event.data);

					return Outcome::NextAndContinue;
				});
				
				match result {
					None | Some(Outcome::RewindAndContinue) => {
						self.events.rewind();
						rewind_offset += length;
						if rewind_offset - curr_pos + range.start >= range.end {
							break;
						}
					}
					Some(Outcome::NextAndContinue) => {
						self.events.next(|_|());
					}
					Some(Outcome::Break) => {
						break;
					}
				}
			}

			assert!(self.playback_position + range.len() as u32 - rewind_offset == (self.playback_position + range.len() as u32) % length);
		}

		self.playback_position += range.len() as u32;
		
		if let Some(length) = self.length {
			self.playback_position %= length;
		}
	}

	pub fn rewind(&mut self) {
		self.playback_position = 0;
		self.events.rewind();
	}

	pub fn seek(&mut self, position: u32) {
		if position < self.playback_position {
			self.rewind();
		}
		assert!(position >= self.playback_position);

		loop {
			match self.events.peek( |event| event.timestamp >= position ) {
				None | Some(true) => { break; }
				Some(false) => { self.events.next(|_|{}); }
			}
		}

		self.playback_position = position;
	}

	/** registers all notes that are currently held down (at time range.begin) as if they were
	  * pressed down at the very beginning of the recording */
	pub fn start_recording<T: MidiDeviceTrait>(&mut self, scope: &T::Scope, device: &T, range: std::ops::Range<u32>) {
		use std::convert::TryInto;
		
		let mut registry = device.clone_registry();
		for event in device.incoming_events(scope) {
			if range.contains(&event.time()) {
				if event.bytes().len() == 3 {
					let data: [u8;3] = event.bytes().try_into().unwrap();
					registry.register_event(data);
				}
			}
		}

		for data in registry.active_notes() {
			self.events.push( MidiMessage {
				timestamp: 0,
				data,
				datalen: 3
			});
		}
	}

	pub fn record<T: MidiDeviceTrait>(&mut self, scope: &T::Scope, device: &T, range: std::ops::Range<u32>) {
		use std::convert::TryInto;
		for event in device.incoming_events(scope) {
			if range.contains(&event.time()) {
				if event.bytes().len() != 3 {
					// FIXME
					println!("ignoring event with length != 3");
				}
				else {
					let data: [u8;3] = event.bytes().try_into().unwrap();
					let timestamp = event.time() - range.start + self.recorded_length;
					
					let result = self.events.push( MidiMessage {
						timestamp,
						data,
						datalen: 3
					});
					// TODO: assert that this is monotonic

					if result.is_err() {
						//log_error("Failed to add MIDI event to already-full MIDI queue! Dropping it...");
						// FIXME
						panic!("Failed to add MIDI event to already-full MIDI queue!");
					}
				}
			}
		}
		
		self.recorded_length += range.len() as u32;
	}
}


#[derive(Debug)]
pub struct AudioTakeNode {
	pub take: RefCell<AudioTake>,
	link: LinkedListLink
}

impl AudioTakeNode {
	pub fn new(take: AudioTake) -> AudioTakeNode {
		AudioTakeNode {
			take: RefCell::new(take),
			link: LinkedListLink::new()
		}
	}
}

#[derive(Debug)]
pub struct MidiTakeNode {
	pub take: RefCell<MidiTake>,
	link: LinkedListLink
}

impl MidiTakeNode {
	pub fn new(take: MidiTake) -> MidiTakeNode {
		MidiTakeNode {
			take: RefCell::new(take),
			link: LinkedListLink::new()
		}
	}
}

intrusive_adapter!(pub AudioTakeAdapter = Box<AudioTakeNode>: AudioTakeNode { link: LinkedListLink });
intrusive_adapter!(pub MidiTakeAdapter = Box<MidiTakeNode>: MidiTakeNode { link: LinkedListLink });
