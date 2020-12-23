use crate::midi_message::MidiMessage;
use super::driver_traits::*;

use std::slice::*;
use std::iter::*;

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
	pub fn run_for(&mut self, run_time: u32, chunksize: u32, mut callback: impl FnMut(&mut DummyScope)) {
		let n_full_chunks = run_time / chunksize;
		let last_chunk = run_time % chunksize;
		for _ in 0..n_full_chunks {
			self.next(chunksize);
			callback(self);
		}
		if last_chunk > 0 {
			self.next(last_chunk);
			callback(self);
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
	type EventIterator<'a> = Cloned<Iter<'a, DummyMidiEvent>>;
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


pub struct DummyAudioDevice {
	pub playback_latency: u32,
	pub capture_latency: u32,

	pub playback_buffers: Vec<Vec<f32>>,
	pub capture_buffers: Vec<Vec<f32>>,
}

pub struct CaptureIter<'a>(Iter<'a, Vec<f32>>, usize);
impl<'a> Iterator for CaptureIter<'a> {
	type Item = &'a [f32];
	fn next(&mut self) -> Option<Self::Item> {
		self.0.next().map(|vec| &vec[self.1..])
	}
}

pub struct PlaybackCaptureIter<'a>(Zip<IterMut<'a, Vec<f32>>, Iter<'a, Vec<f32>>>, usize);
impl<'a> Iterator for PlaybackCaptureIter<'a> {
	type Item = (&'a mut [f32], &'a [f32]);
	fn next(&mut self) -> Option<Self::Item> {
		self.0.next().map(|x| (&mut x.0[self.1..], &x.1[self.1..]) )
	}
}

impl DummyAudioDevice {
	pub fn new(n_channels: usize, playback_latency: u32, capture_latency: u32) -> DummyAudioDevice {
		DummyAudioDevice {
			playback_latency,
			capture_latency,
			playback_buffers: vec![ vec![]; n_channels ],
			capture_buffers: vec![ vec![]; n_channels ],
		}
	}
}

impl AudioDeviceTrait for DummyAudioDevice {
	type Scope = DummyScope;
	type MutSliceIter<'a> = PlaybackCaptureIter<'a>;
	type SliceIter<'a> = CaptureIter<'a>;

	fn info(&self) -> AudioDeviceInfo { unimplemented!(); }
	fn playback_latency(&self) -> u32 { self.playback_latency }
	fn capture_latency(&self) -> u32 { self.capture_latency }
	fn playback_and_capture_buffers(&mut self, scope: &DummyScope) -> PlaybackCaptureIter {
		for vec in self.playback_buffers.iter_mut().chain( self.capture_buffers.iter_mut() ) {
			if vec.len() < (scope.time + scope.n_frames) as usize {
				vec.resize((scope.time + scope.n_frames) as usize, 0.0);
			}
		}
		PlaybackCaptureIter(self.playback_buffers.iter_mut().zip(self.capture_buffers.iter()), scope.time as usize)
	}
	fn record_buffers(&self, scope: &DummyScope) -> CaptureIter {
		CaptureIter(self.capture_buffers.iter(), scope.time as usize)
	}
}
