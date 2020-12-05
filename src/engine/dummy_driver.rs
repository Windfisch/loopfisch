use crate::midi_message::MidiMessage;
use super::driver_traits::*;

pub struct DummyMidiDevice {
	queue: Vec<MidiMessage>,
	pub committed: Vec<MidiMessage>,
	latency: u32
}
impl DummyMidiDevice {
	pub fn new(latency: u32) -> DummyMidiDevice {
		DummyMidiDevice {
			queue: vec![],
			committed: vec![],
			latency
		}
	}
}

pub struct DummyScope {
	pub n_frames: u32,
	pub time: u32,
}
impl ProcessScopeTrait for DummyScope {
	fn n_frames(&self) -> u32 { return self.n_frames; }
}
impl DummyScope {
	pub fn next(&mut self, n_frames: u32) {
		self.time += self.n_frames;
		self.n_frames = n_frames;
	}
	pub fn new() -> DummyScope {
		DummyScope {
			n_frames: 0,
			time: 0
		}
	}
}

#[derive(Clone)]
pub struct DummyMidiEvent {
	data: Vec<u8>,
	time: u32
}
impl TimestampedMidiEvent for DummyMidiEvent {
	fn time(&self) -> u32 { self.time }
	fn bytes(&self) -> &[u8] { &self.data }
}

impl MidiDeviceTrait for &mut DummyMidiDevice {
	type Event<'a> = DummyMidiEvent;
	type EventIterator<'a> = std::iter::Cloned<std::slice::Iter<'a, DummyMidiEvent>>;
	type Scope = DummyScope;

	fn incoming_events(&'a self, scope: &'a Self::Scope) -> Self::EventIterator<'a> { unimplemented!(); }
	fn commit_out_buffer(&mut self, scope: &Self::Scope) {
		for message in self.queue.iter() {
			assert!(message.timestamp < scope.n_frames);
			self.committed.push(MidiMessage {
				timestamp: message.timestamp + scope.time,
				data: message.data,
				datalen: message.datalen
			});
		}
		self.queue = vec![];
	}
	fn queue_event(&mut self, msg: MidiMessage) -> Result<(), ()> {
		self.queue.push(msg);
		Ok(())
	}
	fn update_registry(&mut self, scope: &Self::Scope) { unimplemented!(); }
	fn clone_registry(&self) -> super::midi_registry::MidiNoteRegistry { unimplemented!(); }
	fn info(&self) -> MidiDeviceInfo { unimplemented!(); }
	fn playback_latency(&self) -> u32 {
		self.latency
	}
	fn capture_latency(&self) -> u32 { unimplemented!(); }
}

