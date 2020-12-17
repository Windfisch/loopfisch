use crate::midi_message::MidiMessage;

pub struct AudioDeviceInfo {
	pub n_channels: usize,
	pub name: String
}

pub struct MidiDeviceInfo {
	pub name: String
}


pub trait ProcessScopeTrait {
	fn n_frames(&self) -> u32;
}

pub trait TimestampedMidiEvent {
	fn time(&self) -> u32;
	fn bytes(&self) -> &[u8];
}

pub trait AudioDeviceTrait {
	type SliceIter<'a>: Iterator<Item = &'a [f32]>;
	type MutSliceIter<'a>: Iterator<Item = (&'a mut [f32], &'a [f32])>;
	type Scope: ProcessScopeTrait;

	fn info(&self) -> AudioDeviceInfo;
	fn playback_latency(&self) -> u32;
	fn capture_latency(&self) -> u32;
	fn playback_and_capture_buffers(&'a mut self, scope: &'a Self::Scope) -> Self::MutSliceIter<'a>;
	fn record_buffers(&'a self, scope: &'a Self::Scope) -> Self::SliceIter<'a>;
}

pub trait MidiDeviceTrait {
	type Event<'a>: TimestampedMidiEvent;
	type EventIterator<'a>: Iterator<Item=Self::Event<'a>>;
	type Scope: ProcessScopeTrait;

	fn incoming_events(&'a self, scope: &'a Self::Scope) -> Self::EventIterator<'a>;
	fn commit_out_buffer(&mut self, scope: &Self::Scope);
	fn queue_event(&mut self, msg: MidiMessage) -> Result<(), ()>;
	fn update_registry(&mut self, scope: &Self::Scope);
	fn clone_registry(&self) -> super::midi_registry::MidiNoteRegistry;
	fn info(&self) -> MidiDeviceInfo;
	fn playback_latency(&self) -> u32;
	fn capture_latency(&self) -> u32;
}

