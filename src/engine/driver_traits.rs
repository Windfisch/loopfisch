use crate::midi_message::MidiMessage;

pub struct AudioDeviceInfo {
	pub n_channels: usize,
	pub name: String
}

pub struct MidiDeviceInfo {
	pub name: String
}


pub trait ProcessScopeTrait {}
pub trait TimestampedMidiEvent<'a> {
	fn time(&self) -> u32;
	fn bytes(&self) -> &[u8];
}

pub trait AudioDeviceTrait<'a> {
	type SliceIter: Iterator<Item = &'a [f32]>;
	type MutSliceIter: Iterator<Item = &'a mut [f32]>;
	type Scope: ProcessScopeTrait;

	fn info(&self) -> AudioDeviceInfo;
	fn playback_latency(&self) -> u32;
	fn capture_latency(&self) -> u32;
	fn playback_buffers(&'a mut self, scope: &'a Self::Scope) -> Self::MutSliceIter;
	fn record_buffers(&'a self, scope: &'a Self::Scope) -> Self::SliceIter;
}

pub trait MidiDeviceTrait<'a> {
	type Event: TimestampedMidiEvent<'a>;
	type EventIterator: Iterator<Item=Self::Event>;
	type Scope: ProcessScopeTrait;

	fn incoming_events(&'a self, scope: &'a Self::Scope) -> Self::EventIterator;
	fn commit_out_buffer(&mut self, scope: &Self::Scope);
	fn queue_event(&mut self, msg: MidiMessage) -> Result<(), ()>;
	fn update_registry(&mut self, scope: &Self::Scope);
	fn clone_registry(&self) -> super::midi_registry::MidiNoteRegistry;
	fn info(&self) -> MidiDeviceInfo;
	fn playback_latency(&self) -> u32;
	fn capture_latency(&self) -> u32;
}

