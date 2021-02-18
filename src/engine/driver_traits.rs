use crate::midi_message::MidiMessage;
use super::backend::AudioThreadState;

pub struct AudioDeviceInfo {
	pub n_channels: usize,
	pub name: String
}

pub struct MidiDeviceInfo {
	pub name: String
}

pub trait DriverTrait: Send {
	type MidiDev : MidiDeviceTrait<Scope = Self::ProcessScope>;
	type AudioDev : AudioDeviceTrait<Scope = Self::ProcessScope>;
	type ProcessScope : ProcessScopeTrait;
	type Error: std::fmt::Debug;

	fn activate(&mut self, audio_thread_state: AudioThreadState<Self>) where Self: Sized;
	fn new_audio_device(&mut self, n_channels: u32, name: &str) -> Result<Self::AudioDev, Self::Error>;
	fn new_midi_device(&mut self, name: &str) -> Result<Self::MidiDev, Self::Error>;

	fn sample_rate(&self) -> u32;
}

pub trait ProcessScopeTrait {
	fn n_frames(&self) -> u32;
}

pub trait TimestampedMidiEvent {
	fn time(&self) -> u32;
	fn bytes(&self) -> &[u8];
}

pub trait AudioDeviceTrait: std::fmt::Debug + Send + 'static {
	type SliceIter<'a>: Iterator<Item = &'a [f32]>;
	type MutSliceIter<'a>: Iterator<Item = (&'a mut [f32], &'a [f32])>;
	type Scope: ProcessScopeTrait;

	fn info(&self) -> AudioDeviceInfo;
	fn playback_latency(&self) -> u32;
	fn capture_latency(&self) -> u32;
	fn playback_and_capture_buffers(&'a mut self, scope: &'a Self::Scope) -> Self::MutSliceIter<'a>;
	fn record_buffers(&'a self, scope: &'a Self::Scope) -> Self::SliceIter<'a>;
}

pub trait MidiDeviceTrait: std::fmt::Debug + Send + 'static {
	type Event<'a>: TimestampedMidiEvent;
	type EventIterator<'a>: Iterator<Item=Self::Event<'a>>;
	type Scope: ProcessScopeTrait;

	fn incoming_events(&'a self, scope: &'a Self::Scope) -> Self::EventIterator<'a>;
	fn commit_out_buffer(&mut self, scope: &Self::Scope);
	fn queue_event(&mut self, msg: MidiMessage) -> Result<(), ()>;
	fn info(&self) -> MidiDeviceInfo;
	fn playback_latency(&self) -> u32;
	fn capture_latency(&self) -> u32;
}

