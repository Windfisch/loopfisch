use crate::midi_message::MidiMessage;
use super::driver_traits::*;
use super::midi_registry::MidiNoteRegistry;

use std::sync::{Arc, Mutex};
use std::slice::*;
use std::iter::*;
use crate::owning_iter::OwningIterator;

#[derive(Debug)]
pub struct DummyMidiDevice {
	queue: Vec<MidiMessage>,
	pub committed: Vec<MidiMessage>,
	pub registry: MidiNoteRegistry,
	pub incoming_events: Vec<DummyMidiEvent>,
	latency: u32
}
impl DummyMidiDevice {
	pub fn new(latency: u32) -> DummyMidiDevice { // FIXME add playback and capture latency here!
		DummyMidiDevice {
			queue: vec![],
			committed: vec![],
			latency,
			registry: MidiNoteRegistry::new(),
			incoming_events: Vec::new()
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

#[derive(Clone, PartialEq, Debug)]
pub struct DummyMidiEvent {
	pub data: Vec<u8>,
	pub time: u32
}
impl TimestampedMidiEvent for DummyMidiEvent {
	fn time(&self) -> u32 { self.time }
	fn bytes(&self) -> &[u8] { &self.data }
}

impl MidiDeviceTrait for DummyMidiDevice {
	type Event<'a> = DummyMidiEvent;
	type EventIterator<'a> = Box<dyn Iterator<Item=DummyMidiEvent> + 'a>;
	type Scope = DummyScope;

	fn incoming_events(&'a self, scope: &'a Self::Scope) -> Self::EventIterator<'a> {
		let range = scope.time .. scope.time+scope.n_frames;
		let time0 = scope.time;
		Box::new(
			self.incoming_events
				.iter()
				.filter(move |ev| range.contains(&ev.time))
				.map(move |ev| DummyMidiEvent { time: ev.time - time0, data: ev.data.clone() })
		)
	}
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
	fn update_registry(&mut self, _scope: &Self::Scope) { unimplemented!(); }
	fn clone_registry(&self) -> super::midi_registry::MidiNoteRegistry {
		self.registry.clone()
	}
	fn info(&self) -> MidiDeviceInfo { unimplemented!(); }
	fn playback_latency(&self) -> u32 {
		self.latency
	}
	fn capture_latency(&self) -> u32 { unimplemented!(); }
}

impl MidiDeviceTrait for Arc<Mutex<DummyMidiDevice>> {
	type Event<'a> = DummyMidiEvent;
	type EventIterator<'a> = Box<dyn Iterator<Item=DummyMidiEvent> + 'a>;
	type Scope = DummyScope;

	fn incoming_events(&'a self, scope: &'a Self::Scope) -> Self::EventIterator<'a> {
		Box::new(
			OwningIterator::new(
				self.lock().unwrap(),
				|v| unsafe {(*v).incoming_events(scope)}
			)
		)
	}
	fn commit_out_buffer(&mut self, scope: &Self::Scope) { self.lock().unwrap().commit_out_buffer(scope) }
	fn queue_event(&mut self, msg: MidiMessage) -> Result<(), ()> { self.lock().unwrap().queue_event(msg) }
	fn update_registry(&mut self, scope: &Self::Scope) { self.lock().unwrap().update_registry(scope) }
	fn clone_registry(&self) -> super::midi_registry::MidiNoteRegistry { self.lock().unwrap().clone_registry() }
	fn info(&self) -> MidiDeviceInfo { self.lock().unwrap().info() }
	fn playback_latency(&self) -> u32 { self.lock().unwrap().playback_latency() }
	fn capture_latency(&self) -> u32 { self.lock().unwrap().capture_latency() }
}

#[derive(Debug)]
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

impl AudioDeviceTrait for Arc<Mutex<DummyAudioDevice>> {
	type Scope = <DummyAudioDevice as AudioDeviceTrait>::Scope;
	type MutSliceIter<'a> = Box<dyn Iterator<Item = (&'a mut[f32], &'a [f32])> + 'a>;
	type SliceIter<'a> = Box<dyn Iterator<Item = &'a [f32]> + 'a>;

	fn info(&self) -> AudioDeviceInfo { self.lock().unwrap().info() }
	fn playback_latency(&self) -> u32 { self.lock().unwrap().playback_latency() }
	fn capture_latency(&self) -> u32 { self.lock().unwrap().capture_latency() }
	fn playback_and_capture_buffers<'a>(&'a mut self, scope: &'a DummyScope) -> Self::MutSliceIter<'a> {
		Box::new(
			OwningIterator::new(
				self.lock().unwrap(),
				|v| unsafe { (*v).playback_and_capture_buffers(&scope) }
			)
		)
	}
	fn record_buffers<'a>(&'a self, scope: &'a DummyScope) -> Self::SliceIter<'a> {
		Box::new(
			OwningIterator::new(
				self.lock().unwrap(),
				|v| unsafe { (*v).record_buffers(&scope)
			})
		)
	}
}

pub struct DummyDriver {
	playback_latency: u32,
	capture_latency: u32,
	sample_rate: u32,

	audio_devices: std::collections::HashMap<String, Arc<Mutex<DummyAudioDevice>> >,
	midi_devices: std::collections::HashMap<String, Arc<Mutex<DummyMidiDevice>> >,
}

impl DriverTrait for DummyDriver {
	type MidiDev = Arc<Mutex<DummyMidiDevice>>;
	type AudioDev = Arc<Mutex<DummyAudioDevice>>;
	type ProcessScope = DummyScope;
	type Error = ();

	fn activate(&mut self, _: super::backend::AudioThreadState<Self>) { }

	fn new_audio_device(&mut self, n_channels: u32, name: &str) -> Result<Self::AudioDev, Self::Error> {
		let arc = Arc::new(Mutex::new(DummyAudioDevice::new(n_channels as usize, self.playback_latency, self.capture_latency)));
		self.audio_devices.insert(name.into(), arc.clone());
		return Ok(arc);
	}
	fn new_midi_device(&mut self, name: &str) -> Result<Self::MidiDev, Self::Error> {
		let arc = Arc::new(Mutex::new(DummyMidiDevice::new(self.playback_latency)));
		self.midi_devices.insert(name.into(), arc.clone());
		return Ok(arc);
	}

	fn sample_rate(&self) -> u32 {
		self.sample_rate
	}
}

impl DummyDriver {
	pub fn new(playback_latency: u32, capture_latency: u32, sample_rate: u32) -> DummyDriver {
		DummyDriver {
			playback_latency,
			capture_latency,
			sample_rate,
			audio_devices: std::collections::HashMap::new(),
			midi_devices: std::collections::HashMap::new(),
		}
	}
}
