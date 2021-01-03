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
	/** not real-time-safe! */
	pub fn new(id: u32, audiodev_id: usize, unmuted: bool, n_channels: usize, chunksize: usize) -> AudioTake {
		AudioTake {
			samples: (0..n_channels).map(|_| Buffer::new(chunksize,chunksize/2)).collect(),
			length: None,
			recorded_length: 0,
			playback_position: 0,
			record_state: RecordState::Waiting,
			id,
			audiodev_id,
			unmuted,
			started_recording_at: 0
		}
	}

	pub fn playback<T: AudioDeviceTrait>(&mut self, scope: &T::Scope, device: &mut T, range_u32: std::ops::Range<u32>) {
		if let Some(length) = self.length {
			let range = range_u32.start as usize .. range_u32.end as usize;
			for (channel_buffer, channel_slices) in self.samples.iter_mut().zip(device.playback_and_capture_buffers(scope)) {
				let mut position = self.playback_position;
				let buffer = &mut channel_slices.0[range.clone()];
				for d in buffer {
					if position >= length {
						channel_buffer.rewind();
						position = 0;
						println!("\nrewind in playback\n");
					}
					position += 1;

					let val = channel_buffer.next();
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
				channel_buffer.next();
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
	/** not real-time-safe! */
	pub fn new(id: u32, mididev_id: usize, unmuted: bool) -> MidiTake {
		MidiTake {
			events: Buffer::new(1024, 512),
			record_state: RecordState::Waiting,
			id,
			mididev_id,
			unmuted,
			unmuted_old: unmuted,
			started_recording_at: 0,
			playback_position: 0,
			length: None,
			recorded_length: 0,
			note_registry: RefCell::new(MidiNoteRegistry::new())
		}
	}

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

			let mut rewind_offset = 0;
			loop {
				let mut note_registry = self.note_registry.borrow_mut();

				if let Some(event) = self.events.peek().filter(|event| event.timestamp < length) {
					assert!(event.timestamp + rewind_offset >= self.playback_position);
					let relative_timestamp = event.timestamp + rewind_offset - self.playback_position + range.start;

					if relative_timestamp >= range.end {
						break;
					}
				
					assert!(range.contains(&relative_timestamp));
					if self.unmuted {
						device.queue_event(
							MidiMessage {
								timestamp: relative_timestamp,
								data: event.data,
								datalen: event.datalen
							}
						).unwrap();
					}
					note_registry.register_event(event.data);

					self.events.next();
				}
				else {
					// no (relevant) events left.
					let last_timestamp_before_loop = rewind_offset + length - 1;
					assert!(last_timestamp_before_loop >= self.playback_position);

					if last_timestamp_before_loop < self.playback_position + range.len() as u32 {
						// rewind only when the song actually passes the take length
						println!("MIDI REWIND");
						self.events.rewind();

						let relative_timestamp = last_timestamp_before_loop - self.playback_position + range.start;
						debug_assert!(range.contains(&relative_timestamp));
						if self.unmuted {
							note_registry.send_noteoffs_at(device, relative_timestamp);
						}
						note_registry.clear();
						rewind_offset += length;
					}
					else {
						// do not rewind yet when take length hasn't been exceeded yet (but the last note has)
						break;
					}
				}
			}
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
			if let Some(_) = self.events.peek().filter(|event| event.timestamp < position) {
				self.events.next();
			}
			else {
				break;
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


#[cfg(test)]
mod tests {
	use super::super::dummy_driver::*;
	use super::*;
	use super::super::testutils::rand_vec_f32;

	fn prepare() -> (AudioTake, DummyScope, DummyAudioDevice) {
		const HUGE_CHUNKSIZE: usize = 100000;
		let t = AudioTake::new(0, 0, false, 2, HUGE_CHUNKSIZE);
		let scope = DummyScope::new();
		let mut dev = DummyAudioDevice::new(2, 0, 0);

		dev.capture_buffers[0] = rand_vec_f32(1337, 44100);
		dev.capture_buffers[1] = rand_vec_f32(42, 44100);

		return (t, scope, dev);
	}

	#[test]
	pub fn audiotake_with_unknown_length_plays_silence() {
		let (mut t, mut scope, mut dev) = prepare();

		scope.run_for(44100, 1024, |scope| {
			t.record(scope, &dev, 0..scope.n_frames());
		});

		scope.run_for(44100, 1024, |scope| {
			t.playback(scope, &mut dev, 0..scope.n_frames());
		});

		assert!(dev.playback_buffers[0].iter().all(|x| *x == 0.0));
		assert!(dev.playback_buffers[1].iter().all(|x| *x == 0.0));
	}

	#[test]
	pub fn unmuted_audiotake_plays_back_recorded_audio() {
		let (mut t, mut scope, mut dev) = prepare();

		scope.run_for(44100, 1024, |scope| {
			t.record(scope, &dev, 0..scope.n_frames());
		});

		t.length = Some(44100);
		t.unmuted = true;
		t.rewind();

		scope.run_for(44100, 1024, |scope| {
			t.playback(scope, &mut dev, 0..scope.n_frames());
		});

		let offset = 44100;
		assert!(dev.capture_buffers[0].len() == dev.capture_buffers[1].len());
		assert!(dev.playback_buffers[0][offset..] == dev.capture_buffers[0][0..offset]);
		assert!(dev.playback_buffers[1][offset..] == dev.capture_buffers[1][0..offset]);
	}
	
	#[test]
	pub fn unmuted_audiotake_plays_back_additively() {
		let (mut t, mut scope, mut dev) = prepare();

		scope.run_for(44100, 1024, |scope| {
			t.record(scope, &dev, 0..scope.n_frames());
		});

		t.length = Some(44100);
		t.unmuted = true;
		t.rewind();

		scope.run_for(44100, 1024, |scope| {
			for (buf, _) in dev.playback_and_capture_buffers(scope) {
				for v in buf.iter_mut() {
					*v = 1.0;
				}
			}
			t.playback(scope, &mut dev, 0..scope.n_frames());
		});


		let offset = 44100;
		assert!(dev.capture_buffers[0].len() == dev.capture_buffers[1].len());
		for i in 0..=1 {
			assert!(
				dev.playback_buffers[i][offset..].iter()
				.zip( dev.capture_buffers[i][0..offset].iter().map(|x|x+1.0) )
				.all( |tup| *tup.0 == tup.1 )
			);
		}
	}
	
	#[test]
	pub fn audiotake_rewind_works() {
		let (mut t, mut scope, mut dev) = prepare();

		scope.run_for(44100, 1024, |scope| {
			t.record(scope, &dev, 0..scope.n_frames());
		});

		t.length = Some(44100);
		t.unmuted = true;
		t.rewind();

		scope.run_for(1000, 1024, |scope| {
			t.playback(scope, &mut dev, 0..scope.n_frames());
		});

		t.rewind();
		
		scope.run_for(1000, 1024, |scope| {
			t.playback(scope, &mut dev, 0..scope.n_frames());
		});

		let offset = 44100;
		assert!(dev.capture_buffers[0].len() == dev.capture_buffers[1].len());
		for i in 0..=1 {
			assert!(dev.playback_buffers[i][offset..offset+1000] == dev.capture_buffers[i][0..1000]);
			assert!(dev.playback_buffers[i][offset+1000..offset+2000] == dev.capture_buffers[i][0..1000]);
		}
	}
	
	#[test]
	pub fn audiotake_seek_works() {
		let (mut t, mut scope, mut dev) = prepare();

		scope.run_for(44100, 1024, |scope| {
			t.record(scope, &dev, 0..scope.n_frames());
		});

		t.length = Some(44100);
		t.unmuted = true;
		t.rewind();

		scope.run_for(1000, 1024, |scope| {
			t.playback(scope, &mut dev, 0..scope.n_frames());
		});

		t.seek(3000); // seek forward
		
		scope.run_for(1000, 1024, |scope| {
			t.playback(scope, &mut dev, 0..scope.n_frames());
		});

		t.seek(1000); // seek backward
		
		scope.run_for(1000, 1024, |scope| {
			t.playback(scope, &mut dev, 0..scope.n_frames());
		});

		let offset = 44100;
		assert!(dev.capture_buffers[0].len() == dev.capture_buffers[1].len());
		for i in 0..=1 {
			assert!(dev.playback_buffers[i][offset..offset+1000] == dev.capture_buffers[i][0..1000]);
			assert!(dev.playback_buffers[i][offset+1000..offset+2000] == dev.capture_buffers[i][3000..4000]);
			assert!(dev.playback_buffers[i][offset+2000..offset+3000] == dev.capture_buffers[i][1000..2000]);
		}
	}
	
	#[test]
	pub fn muted_audiotake_plays_silence() {
		let (mut t, mut scope, mut dev) = prepare();

		scope.run_for(44100, 1024, |scope| {
			t.record(scope, &dev, 0..scope.n_frames());
		});

		t.length = Some(44100);
		t.unmuted = false;
		t.rewind();

		scope.run_for(44100, 1024, |scope| {
			t.playback(scope, &mut dev, 0..scope.n_frames());
		});

		assert!(dev.capture_buffers[0].len() == dev.capture_buffers[1].len());
		assert!(dev.playback_buffers[0].iter().all(|x| *x == 0.0));
		assert!(dev.playback_buffers[1].iter().all(|x| *x == 0.0));
	}
	
	#[test]
	pub fn audiotake_past_the_end_loops() {
		let (mut t, mut scope, mut dev) = prepare();

		scope.run_for(44100, 1024, |scope| {
			t.record(scope, &dev, 0..scope.n_frames());
		});

		t.length = Some(44100);
		t.unmuted = true;
		t.rewind();

		scope.run_for(88200, 1024, |scope| {
			t.playback(scope, &mut dev, 0..scope.n_frames());
		});

		let len = 44100;
		assert!(dev.capture_buffers[0].len() == dev.capture_buffers[1].len());
		assert!(dev.playback_buffers[0][len..len*2] == dev.capture_buffers[0][0..len]);
		assert!(dev.playback_buffers[1][len..len*2] == dev.capture_buffers[1][0..len]);
		assert!(dev.playback_buffers[0][len*2..] == dev.capture_buffers[0][0..len]);
		assert!(dev.playback_buffers[1][len*2..] == dev.capture_buffers[1][0..len]);
	}
	
	#[test]
	pub fn audiotake_playback_can_be_interleaved_with_recording() {
		let (mut t, mut scope, mut dev) = prepare();

		scope.next(1024);
		t.record(&scope, &dev, 0..scope.n_frames());

		t.length = Some(20000);
		t.unmuted = true;
		t.rewind();

		scope.run_for(44100-1024, 1024, |scope| {
			t.record(scope, &dev, 0..scope.n_frames());
			t.playback(scope, &mut dev, 0..scope.n_frames());
		});

		assert!(dev.capture_buffers[0].len() == dev.capture_buffers[1].len());
		for i in 0..=1 {
			assert!(dev.playback_buffers[i][1024..1024+20000] == dev.capture_buffers[i][0..20000]);
			assert!(dev.playback_buffers[i][1024+20000..1024+20000+20000] == dev.capture_buffers[i][0..20000]);
		}
	}

	fn prepare2() -> (MidiTake, DummyScope, DummyMidiDevice) {
		let mut t = MidiTake::new(0, 0, false);
		let mut scope = DummyScope::new();
		let mut dev = DummyMidiDevice::new(0, 0);

		dev.incoming_events = vec![
			DummyMidiEvent { time:     0, data: vec![0x90, 50, 64] },
			DummyMidiEvent { time:     1, data: vec![0x90, 51, 64] },
			DummyMidiEvent { time:   230, data: vec![0x90, 60, 64] },
			DummyMidiEvent { time:  1023, data: vec![0x80, 50, 64] },
			DummyMidiEvent { time:  1023, data: vec![0x80, 51, 64] },
			DummyMidiEvent { time:  1024, data: vec![0x90, 52, 64] },
			DummyMidiEvent { time:  1024, data: vec![0x90, 53, 64] },
			DummyMidiEvent { time:  1100, data: vec![0x80, 53, 64] },
			DummyMidiEvent { time:  1100, data: vec![0x80, 52, 64] },
			DummyMidiEvent { time:  1200, data: vec![0x80, 60, 64] },
		];

		scope.next(1024);
		t.start_recording(&scope, &mut dev, 0..0);
		t.record(&scope, &mut dev, 0..scope.n_frames());
		
		scope.next(1024);
		t.record(&scope, &mut dev, 0..scope.n_frames());

		return (t, scope, dev);
	}

	fn extract_and_convert(dev: &DummyMidiDevice, range: std::ops::Range<u32>) -> Vec<DummyMidiEvent> {
		dev.committed
			.iter()
			.filter(|msg| range.contains(&msg.timestamp))
			.map(|msg| DummyMidiEvent {
				time: msg.timestamp - range.start,
				data: msg.data[0..msg.datalen as usize].into()
			}
		).collect()
	}

	#[test]
	pub fn miditake_with_unknown_length_plays_nothing() {
		let (mut t, mut scope, mut dev) = prepare2();

		t.unmuted = true;
		t.unmuted_old = true;
		t.length = None;

		scope.run_for(2048, 1024, |scope| {
			t.playback(&mut dev, 0..scope.n_frames());
			dev.commit_out_buffer(scope);
		});

		assert!(dev.committed.len() == 0);
	}

	#[test]
	pub fn muted_miditake_with_known_length_plays_nothing() {
		let (mut t, mut scope, mut dev) = prepare2();

		t.unmuted = false;
		t.unmuted_old = false;
		t.length = Some(4096);
		t.rewind();

		scope.run_for(2048, 1024, |scope| {
			t.playback(&mut dev, 0..scope.n_frames());
			dev.commit_out_buffer(scope);
		});
		
		assert!(dev.committed.len() == 0);
	}

	#[test]
	pub fn unmuted_miditake_with_known_length_plays_recorded_events_and_loops() {
		let execute_with_buffersize = |buffersize| {
			let (mut t, mut scope, mut dev) = prepare2();

			t.unmuted = true;
			t.unmuted_old = true;
			t.length = Some(2048);
			t.rewind();

			scope.run_for(2048*3, buffersize, |scope| {
				t.playback(&mut dev, 0..scope.n_frames());
				dev.commit_out_buffer(scope);
			});
		
			assert!(extract_and_convert(&dev, 2048..4096) == dev.incoming_events);
			assert!(extract_and_convert(&dev, 4096..6144) == dev.incoming_events);
			assert!(extract_and_convert(&dev, 6144..8192) == dev.incoming_events);
		};

		// loop length is divisible by the buffer size
		execute_with_buffersize(1024);
		execute_with_buffersize(32);
		execute_with_buffersize(1);

		// loop length is not divisible by the buffer size
		execute_with_buffersize(997);

		// loop length is shorter than the buffer size
		execute_with_buffersize(3000);

		// 3 * loop length is shorter than the buffer size
		execute_with_buffersize(2048 * 4);
	}

	#[test]
	pub fn miditake_sends_noteoff_for_dangling_notes_at_the_end() {
		let execute_with_buffersize = |buffersize| {
			let (mut t, mut scope, mut dev) = prepare2();

			t.unmuted = true;
			t.unmuted_old = true;
			t.length = Some(1024);
			t.rewind();

			scope.run_for(1024*3, buffersize, |scope| {
				t.playback(&mut dev, 0..scope.n_frames());
				dev.commit_out_buffer(scope);
			});
			
			let expected_events : Vec<_> =
				dev.incoming_events.iter().filter(|ev| ev.time < 1024)
				.chain([ DummyMidiEvent { time:  1023, data: vec![0x80, 60, 64] } ].iter())
				.cloned().collect();

			assert!(extract_and_convert(&dev, 2048..3072) == expected_events);
			assert!(extract_and_convert(&dev, 3072..4096) == expected_events);
			assert!(extract_and_convert(&dev, 4096..5120) == expected_events);
		};

		// loop length is divisible by the buffer size
		execute_with_buffersize(1024);
		execute_with_buffersize(32);
		execute_with_buffersize(1);

		// loop length is not divisible by the buffer size
		execute_with_buffersize(997);

		// loop length is shorter than the buffer size
		execute_with_buffersize(3000);

		// 3 * loop length is shorter than the buffer size
		execute_with_buffersize(2048 * 4);
	}

	#[test]
	pub fn miditake_sends_noteon_for_already_held_notes_at_the_start() {
		let mut t = MidiTake::new(0, 0, false);
		let mut scope = DummyScope::new();
		let mut dev = DummyMidiDevice::new(0, 0);

		dev.registry.register_event([0x90, 31, 64]);
		dev.incoming_events = vec![
			DummyMidiEvent { time:  500, data: vec![0x90, 30, 64] },
			DummyMidiEvent { time: 1000, data: vec![0x90, 50, 64] },
			DummyMidiEvent { time: 1001, data: vec![0x90, 51, 64] },
			DummyMidiEvent { time: 1200, data: vec![0x80, 30, 64] },
			DummyMidiEvent { time: 1200, data: vec![0x80, 31, 64] },
			DummyMidiEvent { time: 1800, data: vec![0x80, 50, 64] },
			DummyMidiEvent { time: 2023, data: vec![0x80, 51, 64] },
		];

		scope.next(2024);
		t.start_recording(&scope, &mut dev, 0..1000);
		t.record(&scope, &mut dev, 1000..scope.n_frames());
		
		t.unmuted = true;
		t.unmuted_old = true;
		t.length = Some(1024);
		t.rewind();

		scope.next(2024);
		t.playback(&mut dev, 0..scope.n_frames());
		dev.commit_out_buffer(&scope);
		
		let expected_events = vec![
			DummyMidiEvent { time:    0, data: vec![0x90, 30, 64] },
			DummyMidiEvent { time:    0, data: vec![0x90, 31, 64] },
			DummyMidiEvent { time:    0, data: vec![0x90, 50, 64] },
			DummyMidiEvent { time:    1, data: vec![0x90, 51, 64] },
			DummyMidiEvent { time:  200, data: vec![0x80, 30, 64] },
			DummyMidiEvent { time:  200, data: vec![0x80, 31, 64] },
			DummyMidiEvent { time:  800, data: vec![0x80, 50, 64] },
			DummyMidiEvent { time: 1023, data: vec![0x80, 51, 64] },
		];

		assert!(extract_and_convert(&dev, 2024..(2024+1024)) == expected_events);
	}

	#[test]
	pub fn miditake_seek_works() {
		let (mut t, mut scope, mut dev) = prepare2();

		t.unmuted = true;
		t.unmuted_old = true;
		t.length = Some(1024);
		t.rewind();

		t.seek(128);

		scope.run_for(256, 16, |scope| {
			t.playback(&mut dev, 0..scope.n_frames());
			dev.commit_out_buffer(scope);
		});

		t.seek(200);
		scope.run_for(64, 16, |scope| {
			t.playback(&mut dev, 0..scope.n_frames());
			dev.commit_out_buffer(scope);
		});

		let expected_events = vec![
			DummyMidiEvent { time: 102, data: vec![0x90, 60, 64] },
			DummyMidiEvent { time: 286, data: vec![0x90, 60, 64] },
		];
		
		assert!(extract_and_convert(&dev, 2048..3072) == expected_events);
	}

	#[test]
	pub fn miditake_playback_and_capture_can_be_interleaved_as_long_the_end_is_never_hit() {
		let mut t = MidiTake::new(0, 0, false);
		let mut scope = DummyScope::new();
		let mut dev = DummyMidiDevice::new(0, 0);

		dev.incoming_events = vec![
			DummyMidiEvent { time:     0, data: vec![0x90, 50, 64] },
			DummyMidiEvent { time:     1, data: vec![0x90, 51, 64] },
			DummyMidiEvent { time:   230, data: vec![0x90, 60, 64] },
			DummyMidiEvent { time:   570, data: vec![0x80, 50, 64] },
			DummyMidiEvent { time:   800, data: vec![0x80, 51, 64] },
			DummyMidiEvent { time:  1024, data: vec![0x90, 52, 64] },
			DummyMidiEvent { time:  1024, data: vec![0x90, 53, 64] },
			DummyMidiEvent { time:  1100, data: vec![0x80, 53, 64] },
			DummyMidiEvent { time:  1100, data: vec![0x80, 52, 64] },
			DummyMidiEvent { time:  1200, data: vec![0x80, 60, 64] },
		];

		scope.next(512);
		t.start_recording(&scope, &mut dev, 0..0);
		t.record(&scope, &mut dev, 0..scope.n_frames());
		
		t.unmuted = true;
		t.unmuted_old = true;
		t.length = Some(4096);
		t.rewind();
		
		scope.run_for(2048, 16, |scope| {
			t.record(scope, &mut dev, 0..scope.n_frames());
			t.playback(&mut dev, 0..scope.n_frames());
			dev.commit_out_buffer(scope);
		});

		assert!(extract_and_convert(&dev, 512..(2048+512)) == dev.incoming_events);
	}

	#[test]
	pub fn miditake_stops_notes_upon_mute() {
		let (mut t, mut scope, mut dev) = prepare2();

		t.unmuted = true;
		t.unmuted_old = true;
		t.length = Some(4096);
		t.rewind();

		scope.run_for(1040, 16, |scope| {
			t.playback(&mut dev, 0..scope.n_frames());
			dev.commit_out_buffer(scope);
		});

		dev.committed.clear();

		t.unmuted = false;
		
		scope.run_for(32, 16, |scope| {
			t.playback(&mut dev, 0..scope.n_frames());
			dev.commit_out_buffer(scope);
		});

		assert!(extract_and_convert(&dev, 2048..4096) == vec![
			DummyMidiEvent { time: 1040, data: vec![0x80, 52, 64] },
			DummyMidiEvent { time: 1040, data: vec![0x80, 53, 64] },
			DummyMidiEvent { time: 1040, data: vec![0x80, 60, 64] },
		]);

		assert!(t.unmuted_old == false);
	}

	#[test]
	pub fn miditake_reactivates_notes_upon_unmute() {
		let (mut t, mut scope, mut dev) = prepare2();

		t.unmuted = false;
		t.unmuted_old = false;
		t.length = Some(4096);
		t.rewind();

		scope.run_for(1040, 16, |scope| {
			t.playback(&mut dev, 0..scope.n_frames());
			dev.commit_out_buffer(scope);
		});

		assert!(dev.committed.len() == 0);

		t.unmuted = true;
		
		scope.run_for(32, 16, |scope| {
			t.playback(&mut dev, 0..scope.n_frames());
			dev.commit_out_buffer(scope);
		});

		assert!(extract_and_convert(&dev, 2048..4096) == vec![
			DummyMidiEvent { time: 1040, data: vec![0x90, 52, 64] },
			DummyMidiEvent { time: 1040, data: vec![0x90, 53, 64] },
			DummyMidiEvent { time: 1040, data: vec![0x90, 60, 64] },
		]);

		assert!(t.unmuted_old == true);
	}
}
