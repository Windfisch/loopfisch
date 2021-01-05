use crate::midi_message::MidiMessage;
use super::driver_traits::*;
use super::midi_registry::MidiNoteRegistry;

use std::sync::{Arc, Mutex};
use std::slice::*;
use std::iter::*;
use crate::owning_iter::OwningIterator;
use assert_no_alloc::{permit_alloc, PermitDrop};
use std::cell::RefCell;
use smallvec::SmallVec;

#[derive(Debug)]
pub struct DummyMidiDevice {
	queue: Vec<MidiMessage>,
	pub committed: Vec<MidiMessage>,
	pub registry: RefCell<MidiNoteRegistry>, // FIXME this should not be a RefCell!
	pub incoming_events: Vec<DummyMidiEvent>,
	playback_latency: u32,
	capture_latency: u32
}
impl DummyMidiDevice {
	pub fn new(playback_latency: u32, capture_latency: u32) -> DummyMidiDevice {
		DummyMidiDevice {
			queue: vec![],
			committed: vec![],
			playback_latency,
			capture_latency,
			registry: RefCell::new(MidiNoteRegistry::new()),
			incoming_events: Vec::new()
		}
	}
}

#[derive(Clone)]
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
	pub data: SmallVec<[u8; 4]>,
	pub time: u32
}
impl TimestampedMidiEvent for DummyMidiEvent {
	fn time(&self) -> u32 { self.time }
	fn bytes(&self) -> &[u8] { &self.data }
}

impl MidiDeviceTrait for DummyMidiDevice {
	type Event<'a> = DummyMidiEvent;
	type EventIterator<'a> = PermitDrop<Box<dyn Iterator<Item=DummyMidiEvent> + 'a>>;
	type Scope = DummyScope;

	fn incoming_events(&'a self, scope: &'a Self::Scope) -> Self::EventIterator<'a> {
		let range = scope.time .. scope.time+scope.n_frames;
		let time0 = scope.time;

		PermitDrop::new( permit_alloc(||
			Box::new(
				self.incoming_events
					.iter()
					.filter(move |ev| range.contains(&ev.time))
					.map(move |ev| DummyMidiEvent { time: ev.time - time0, data: ev.data.clone() })
			)
		))
	}
	fn commit_out_buffer(&mut self, scope: &Self::Scope) {
		permit_alloc(|| {
			for message in self.queue.iter() {
				assert!(message.timestamp < scope.n_frames);
				self.committed.push(MidiMessage {
					timestamp: message.timestamp + scope.time,
					data: message.data,
					datalen: message.datalen
				});
			}
			self.queue = vec![];
		})
	}
	fn queue_event(&mut self, msg: MidiMessage) -> Result<(), ()> {
		permit_alloc(|| {
			self.queue.push(msg);
		});
		Ok(())
	}
	fn update_registry(&mut self, scope: &Self::Scope) { 
		// FIXME duplicate code, same as in jack_driver.rs.
		// This method should not be part of the DriverTrait API,
		// instead it should be a detail of the backend.
		permit_alloc(|| {
			for event in self.incoming_events(scope) {
				if event.data.len() == 3 {
					let data = [event.data[0], event.data[1], event.data[2]];
					self.registry.borrow_mut().register_event(data);
				}
			}
		});
	}
	fn clone_registry(&self) -> super::midi_registry::MidiNoteRegistry {
		self.registry.borrow_mut().clone()
	}
	fn info(&self) -> MidiDeviceInfo {
		MidiDeviceInfo {
			name: "??".into()
		}
	}
	fn playback_latency(&self) -> u32 {
		self.playback_latency
	}
	fn capture_latency(&self) -> u32 {
		self.capture_latency
	}
}

impl MidiDeviceTrait for Arc<Mutex<DummyMidiDevice>> {
	type Event<'a> = DummyMidiEvent;
	type EventIterator<'a> = PermitDrop<Box<dyn Iterator<Item=DummyMidiEvent> + 'a>>;
	type Scope = DummyScope;

	fn incoming_events(&'a self, scope: &'a Self::Scope) -> Self::EventIterator<'a> {
		PermitDrop::new( permit_alloc(|| Box::new(
			OwningIterator::new(
				self.lock().unwrap(),
				|v| unsafe {(*v).incoming_events(scope)}
			)
		)))
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

pub struct CaptureIter<'a>(Iter<'a, Vec<f32>>, usize, usize);
impl<'a> Iterator for CaptureIter<'a> {
	type Item = &'a [f32];
	fn next(&mut self) -> Option<Self::Item> {
		self.0.next().map(|vec| &vec[self.1..self.2])
	}
}

pub struct PlaybackCaptureIter<'a>(Zip<IterMut<'a, Vec<f32>>, Iter<'a, Vec<f32>>>, usize, usize);
impl<'a> Iterator for PlaybackCaptureIter<'a> {
	type Item = (&'a mut [f32], &'a [f32]);
	fn next(&mut self) -> Option<Self::Item> {
		self.0.next().map(|x| (&mut x.0[self.1..], &x.1[self.1..self.2]) )
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

	fn info(&self) -> AudioDeviceInfo {
		AudioDeviceInfo {
			name: "??".into(),
			n_channels: self.playback_buffers.len()
		}
	}
	fn playback_latency(&self) -> u32 { self.playback_latency }
	fn capture_latency(&self) -> u32 { self.capture_latency }
	fn playback_and_capture_buffers(&mut self, scope: &DummyScope) -> PlaybackCaptureIter {
		permit_alloc(|| {
			for vec in self.playback_buffers.iter_mut().chain( self.capture_buffers.iter_mut() ) {
				if vec.len() < (scope.time + scope.n_frames) as usize {
					vec.resize((scope.time + scope.n_frames) as usize, 0.0);
				}
			}
		});
		PlaybackCaptureIter(self.playback_buffers.iter_mut().zip(self.capture_buffers.iter()), scope.time as usize, (scope.time + scope.n_frames) as usize)
	}
	fn record_buffers(&self, scope: &DummyScope) -> CaptureIter {
		CaptureIter(self.capture_buffers.iter(), scope.time as usize, (scope.time + scope.n_frames) as usize)
	}
}

impl AudioDeviceTrait for Arc<Mutex<DummyAudioDevice>> {
	type Scope = <DummyAudioDevice as AudioDeviceTrait>::Scope;
	type MutSliceIter<'a> = PermitDrop<Box<dyn Iterator<Item = (&'a mut[f32], &'a [f32])> + 'a>>;
	type SliceIter<'a> = PermitDrop<Box<dyn Iterator<Item = &'a [f32]> + 'a>>;

	fn info(&self) -> AudioDeviceInfo { self.lock().unwrap().info() }
	fn playback_latency(&self) -> u32 { self.lock().unwrap().playback_latency() }
	fn capture_latency(&self) -> u32 { self.lock().unwrap().capture_latency() }
	fn playback_and_capture_buffers<'a>(&'a mut self, scope: &'a DummyScope) -> Self::MutSliceIter<'a> {
		PermitDrop::new(
			permit_alloc(move || Box::new(
				OwningIterator::new(
					self.lock().unwrap(),
					|v| unsafe { (*v).playback_and_capture_buffers(&scope) }
				)
			))
		)
	}
	fn record_buffers<'a>(&'a self, scope: &'a DummyScope) -> Self::SliceIter<'a> {
		PermitDrop::new(
			permit_alloc(move || Box::new(
				OwningIterator::new(
					self.lock().unwrap(),
					|v| unsafe { (*v).record_buffers(&scope) }
				)
			)
		))
	}
}

pub struct DummyDriverData {
	pub playback_latency: u32,
	pub capture_latency: u32,
	pub sample_rate: u32,

	pub audio_devices: std::collections::HashMap<String, Arc<Mutex<DummyAudioDevice>> >,
	pub midi_devices: std::collections::HashMap<String, Arc<Mutex<DummyMidiDevice>> >,

	backend: Option<super::backend::AudioThreadState<DummyDriver>>,
	scope: DummyScope
}

pub struct DummyDriver(pub Arc<Mutex<DummyDriverData>>);

impl DriverTrait for DummyDriver {
	type MidiDev = Arc<Mutex<DummyMidiDevice>>;
	type AudioDev = Arc<Mutex<DummyAudioDevice>>;
	type ProcessScope = DummyScope;
	type Error = ();

	fn activate(&mut self, backend: super::backend::AudioThreadState<Self>) {
		self.0.lock().unwrap().backend = Some(backend);
	}

	fn new_audio_device(&mut self, n_channels: u32, name: &str) -> Result<Self::AudioDev, Self::Error> {
		println!("new audio device '{}' ({} channels)", name, n_channels);
		let mut lock = self.0.lock().unwrap();
		let arc = Arc::new(Mutex::new(DummyAudioDevice::new(n_channels as usize, lock.playback_latency, lock.capture_latency)));
		lock.audio_devices.insert(name.into(), arc.clone());
		return Ok(arc);
	}
	fn new_midi_device(&mut self, name: &str) -> Result<Self::MidiDev, Self::Error> {
		let mut lock = self.0.lock().unwrap();
		let arc = Arc::new(Mutex::new(DummyMidiDevice::new(lock.playback_latency, lock.capture_latency)));
		lock.midi_devices.insert(name.into(), arc.clone());
		return Ok(arc);
	}

	fn sample_rate(&self) -> u32 {
		self.0.lock().unwrap().sample_rate
	}
}

impl DummyDriver {
	pub fn new(playback_latency: u32, capture_latency: u32, sample_rate: u32) -> DummyDriver {
		DummyDriver(Arc::new(Mutex::new(DummyDriverData {
			playback_latency,
			capture_latency,
			sample_rate,
			audio_devices: std::collections::HashMap::new(),
			midi_devices: std::collections::HashMap::new(),
			backend: None,
			scope: DummyScope::new()
		})))
	}

	pub fn process(&self, n_frames: u32) {
		let mut inner = self.0.lock().unwrap();
		inner.scope.next(n_frames);
		let scope = inner.scope.clone();
		inner.backend.as_mut().unwrap().process_callback(&scope);
	}

	pub fn process_for(&self, n_total_frames: u32, chunksize: u32) {
		for _ in 0..(n_total_frames / chunksize) {
			self.process(chunksize);
		}
		if n_total_frames % chunksize > 0 {
			self.process(n_total_frames % chunksize)
		}
	}

	pub fn lock<'a>(&'a self) -> impl std::ops::DerefMut<Target = DummyDriverData> + 'a {
		self.0.lock().unwrap()
	}

	pub fn clone(&self) -> DummyDriver {
		DummyDriver(self.0.clone())
	}
}
