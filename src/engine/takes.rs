use super::data::RecordState;

use intrusive_collections::{intrusive_adapter, LinkedListLink};
use std::cell::RefCell;

use crate::midi_message::MidiMessage;

use super::jack_driver::*;

use super::midi_registry::MidiNoteRegistry;

use crate::outsourced_allocation_buffer::Buffer;


pub struct AudioTake {
	/// Sequence of all samples. The take's duration and playhead position are implicitly managed by the underlying Buffer.
	pub samples: Vec<Buffer<f32>>,
	pub record_state: RecordState,
	pub id: u32,
	pub audiodev_id: usize,
	pub unmuted: bool,
	pub playing: bool,
	pub started_recording_at: u32,
}

impl std::fmt::Debug for AudioTake {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AudioTake")
			.field("record_state", &self.record_state)
			.field("id", &self.id)
			.field("audiodev_id", &self.audiodev_id)
			.field("unmuted", &self.unmuted)
			.field("playing", &self.playing)
			.field("started_recording_at", &self.started_recording_at)
			.field("channels", &self.samples.len())
			.field("samples", &if self.samples[0].empty() { "<Empty>".to_string() } else { "[...]".to_string() })
			.finish()
	}
}

impl AudioTake {
	pub fn playback(&mut self, scope: &jack::ProcessScope, device: &mut AudioDevice, range_u32: std::ops::Range<u32>) {
		let range = range_u32.start as usize .. range_u32.end as usize;
		for (channel_buffer, channel_ports) in self.samples.iter_mut().zip(device.channels.iter_mut()) {
			let buffer = &mut channel_ports.out_port.as_mut_slice(scope)[range.clone()];
			for d in buffer {
				let mut val = channel_buffer.next(|v|*v);
				if val.is_none() {
					channel_buffer.rewind();
					println!("\nrewind in playback\n");
					val = channel_buffer.next(|v|*v);
				}
				if let Some(v) = val {
					if self.unmuted { // FIXME fade in / out to avoid clicks
						*d += v;
					}
				}
				else {
					unreachable!();
				}
			}
		}
	}
	
	pub fn rewind(&mut self) {
		for channel_buffer in self.samples.iter_mut() {
			channel_buffer.rewind();
		}
	}

	pub fn record(&mut self, scope: &jack::ProcessScope, device: &AudioDevice, range_u32: std::ops::Range<u32>) {
		let range = range_u32.start as usize .. range_u32.end as usize;
		for (channel_buffer, channel_ports) in self.samples.iter_mut().zip(device.channels.iter()) {
			let data = &channel_ports.in_port.as_slice(scope)[range.clone()];
			for d in data {
				let err = channel_buffer.push(*d).is_err();
				if err {
					// FIXME proper error handling, such as marking the take as stale, dropping it.
					panic!("Failed to add audio sample to already-full sample queue!");
				}
			}
		}
	}
}

pub struct MidiTake {
	/// Sorted sequence of all events with timestamps between 0 and self.duration
	pub events: Buffer<MidiMessage>,
	/// Current playhead position
	pub current_position: u32,
	/// Number of frames after which the recorded events shall loop.
	pub duration: u32,
	pub record_state: RecordState,
	pub id: u32,
	pub mididev_id: usize,
	pub unmuted: bool,
	pub unmuted_old: bool,
	pub playing: bool,
	pub started_recording_at: u32,
	pub note_registry: RefCell<MidiNoteRegistry> // this SUCKS. TODO.
}

impl std::fmt::Debug for MidiTake {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("MidiTake")
			.field("record_state", &self.record_state)
			.field("id", &self.id)
			.field("mididev_id", &self.mididev_id)
			.field("unmuted", &self.unmuted)
			.field("playing", &self.playing)
			.field("started_recording_at", &self.started_recording_at)
			.field("events", &if self.events.empty() { "<Empty>".to_string() } else { "[...]".to_string() })
			.finish()
	}
}



impl MidiTake {
	/// Enumerates all events that take place in the next `range.len()` frames and puts
	/// them into device's playback queue. The events are automatically looped every
	/// `self.duration` frames.
	pub fn playback(&mut self, device: &mut MidiDevice, range_u32: std::ops::Range<u32>) {
		let range = range_u32.start as usize .. range_u32.end as usize;
		if self.unmuted != self.unmuted_old {
			if self.unmuted {
				self.note_registry.borrow_mut().send_noteons(device);
			}
			else {
				self.note_registry.borrow_mut().send_noteoffs(device);
			}
			self.unmuted_old = self.unmuted;
		}
		

		let position_after = self.current_position + range.len() as u32;

		// iterate through the events until either a) we've reached the end or b) we've reached
		// an event which is past the current period.
		let curr_pos = self.current_position;
		let mut rewind_offset = 0;
		loop {
			let unmuted = self.unmuted;
			let mut note_registry = self.note_registry.borrow_mut(); // TODO this SUCKS! oh god why, rust. this whole callback thing is garbage.
			let result = self.events.peek( |event| {
				assert!(event.timestamp + rewind_offset >= curr_pos);
				let relative_timestamp = event.timestamp + rewind_offset - curr_pos + range.start as u32;
				assert!(relative_timestamp >= range.start as u32);
				if relative_timestamp >= range.end as u32 {
					return false; // signify "please break the loop"
				}

				if unmuted {
					device.queue_event(
						MidiMessage {
							timestamp: relative_timestamp,
							data: event.data
						}
					).unwrap();
				}
				note_registry.register_event(event.data);

				return true; // signify "please continue the loop"
			});

			match result {
				None => { // we hit the end of the event list
					if  position_after - rewind_offset > self.duration {
						// we actually should rewind, since the playhead position has hit the end
						self.events.rewind();
						rewind_offset += self.duration;
					}
					else {
						// we should *not* rewind: we're at the end of the event list, but the
						// loop duration itself has not passed yet
						break;
					}
				}
				Some(true) => { // the usual case
					self.events.next(|_|());
				}
				Some(false) => { // we found an event which is past the current range
					break;
				}
			}
		}

		self.current_position = position_after - rewind_offset;
	}

	pub fn rewind(&mut self) {
		self.current_position = 0;
		self.events.rewind();
	}

	pub fn start_recording(&mut self, scope: &jack::ProcessScope, device: &MidiDevice, range_u32: std::ops::Range<u32>) {
		use std::convert::TryInto;
		let range = range_u32.start as usize .. range_u32.end as usize;
		
		let mut registry = device.clone_registry();
		for event in device.in_port.iter(scope) {
			if range.contains(&(event.time as usize)) {
				if event.bytes.len() == 3 {
					let data: [u8;3] = event.bytes.try_into().unwrap();
					registry.register_event(data);
				}
			}
		}

		for data in registry.active_notes() {
			self.events.push( MidiMessage {
				timestamp: 0,
				data
			});
		}
	}

	pub fn finish_recording(&mut self, scope: &jack::ProcessScope, device: &MidiDevice, range_u32: std::ops::Range<u32>) {
		use std::convert::TryInto;
		let range = range_u32.start as usize .. range_u32.end as usize;
		
		let mut registry = device.clone_registry();
		for event in device.in_port.iter(scope) {
			if range.contains(&(event.time as usize)) {
				if event.bytes.len() == 3 {
					let data: [u8;3] = event.bytes.try_into().unwrap();
					registry.register_event(data);
				}
			}
		}

		for mut data in registry.active_notes() {
			data[0] = 0x80 | (0x3f & data[0]); // turn the note-on that was returned into a note-off
			data[2] = 64;
			self.events.push( MidiMessage {
				timestamp: 0,
				data
			});
		}
	}

	pub fn record(&mut self, scope: &jack::ProcessScope, device: &MidiDevice, range_u32: std::ops::Range<u32>) {
		use std::convert::TryInto;
		let range = range_u32.start as usize .. range_u32.end as usize;
		for event in device.in_port.iter(scope) {
			if range.contains(&(event.time as usize)) {
				if event.bytes.len() != 3 {
					// FIXME
					println!("ignoring event with length != 3");
				}
				else {
					let data: [u8;3] = event.bytes.try_into().unwrap();
					let timestamp = event.time - range.start as u32 + self.duration;
					
					let result = self.events.push( MidiMessage {
						timestamp,
						data
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
		
		self.duration += range.len() as u32;
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
