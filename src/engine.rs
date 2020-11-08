use core::cmp::min;
use intrusive_collections::{intrusive_adapter, LinkedList, LinkedListLink};
use std::sync::atomic::*;
use std::sync::Arc;
use std::cell::RefCell;
use std::collections::HashMap;
use crate::id_generator::IdGenerator;

use crate::midi_message::MidiMessage;

use crate::jack_driver::*;

use crate::midi_registry::MidiNoteRegistry;
use crate::metronome::AudioMetronome;

use crate::outsourced_allocation_buffer::Buffer;

use assert_no_alloc::assert_no_alloc;
use crate::realtime_send_queue;

pub enum Event {
	AudioTakeStateChanged(usize, u32, RecordState),
	MidiTakeStateChanged(usize, u32, RecordState),
	Kill
}

#[derive(std::cmp::PartialEq, Debug)]
pub enum RecordState {
	Waiting,
	Recording,
	Finished
}

#[derive(Debug)]
struct AudioTakeNode {
	take: RefCell<AudioTake>,
	link: LinkedListLink
}

impl AudioTakeNode {
	fn new(take: AudioTake) -> AudioTakeNode {
		AudioTakeNode {
			take: RefCell::new(take),
			link: LinkedListLink::new()
		}
	}
}

#[derive(Debug)]
struct MidiTakeNode {
	take: RefCell<MidiTake>,
	link: LinkedListLink
}

impl MidiTakeNode {
	fn new(take: MidiTake) -> MidiTakeNode {
		MidiTakeNode {
			take: RefCell::new(take),
			link: LinkedListLink::new()
		}
	}
}

intrusive_adapter!(AudioTakeAdapter = Box<AudioTakeNode>: AudioTakeNode { link: LinkedListLink });
intrusive_adapter!(MidiTakeAdapter = Box<MidiTakeNode>: MidiTakeNode { link: LinkedListLink });

#[derive(Debug)]
enum Message {
	UpdateAudioDevice(usize, Option<AudioDevice>),
	UpdateMidiDevice(usize, Option<MidiDevice>),
	NewAudioTake(Box<AudioTakeNode>),
	NewMidiTake(Box<MidiTakeNode>),
	SetAudioMute(u32,bool),
	SetMidiMute(u32,bool),
	DeleteTake(u32)
}

enum DestructionRequest {
	AudioDevice(AudioDevice),
	MidiDevice(MidiDevice),
	End
}

pub struct AudioThreadState {
	devices: Vec<Option<AudioDevice>>,
	mididevices: Vec<Option<MidiDevice>>,
	metronome: AudioMetronome,
	audiotakes: LinkedList<AudioTakeAdapter>,
	miditakes: LinkedList<MidiTakeAdapter>,
	command_channel: ringbuf::Consumer<Message>,
	transport_position: u32, // does not wrap 
	song_position: u32, // wraps
	song_length: u32,
	shared: Arc<SharedThreadState>,
	event_channel: realtime_send_queue::Producer<Event>,
	destructor_thread_handle: std::thread::JoinHandle<()>,
	destructor_channel: ringbuf::Producer<DestructionRequest>
}

pub struct SharedThreadState {
	pub song_length: AtomicU32,
	pub song_position: AtomicU32,
	pub transport_position: AtomicU32,
}

pub struct GuiAudioDevice {
	info: AudioDeviceInfo,
	takes: Vec<GuiAudioTake>,
}

impl GuiAudioDevice {
	pub fn info(&self) -> &AudioDeviceInfo { &self.info }
	pub fn takes(&self) -> &Vec<GuiAudioTake> { &self.takes }
}

pub struct GuiMidiDevice {
	info: MidiDeviceInfo,
	takes: Vec<GuiMidiTake>,
}

impl GuiMidiDevice {
	pub fn info(&self) -> &MidiDeviceInfo { &self.info }
	pub fn takes(&self) -> &Vec<GuiMidiTake> { &self.takes }
}

pub struct FrontendThreadState {
	command_channel: RetryChannelPush<Message>,
	devices: HashMap<usize, GuiAudioDevice>,
	mididevices: HashMap<usize, GuiMidiDevice>,
	pub shared: Arc<SharedThreadState>,
	next_id: IdGenerator,
	async_client: Box<dyn IntoJackClient>
}

fn find_first_free_index<T>(map: &HashMap<usize, T>, max: usize) -> Option<usize> {
	for i in 0..max {
		if map.get(&i).is_none() {
			return Some(i);
		}
	}
	return None;
}

struct RetryChannelPush<T: std::fmt::Debug> (ringbuf::Producer<T>);
impl<T: std::fmt::Debug> RetryChannelPush<T> {
	pub fn send_message(&mut self, message: T) -> Result<(),()> {
		println!("Sending message {:#?}", message);
		let mut m = message;
		for _ in 0..100 {
			match self.0.push(m) {
				Ok(()) => { return Ok(()); }
				Err(undelivered_message) => { m = undelivered_message; }
			}
			std::thread::sleep( std::time::Duration::from_millis(10) );
		}
		return Err(());
	}
}

impl FrontendThreadState {
	pub fn devices(&self) -> &HashMap<usize, GuiAudioDevice> { &self.devices}
	pub fn mididevices(&self) -> &HashMap<usize, GuiMidiDevice> { &self.mididevices}

	pub fn add_device(&mut self, name: &str, channels: u32) -> Result<usize,()> {
		if let Some(id) = find_first_free_index(&self.devices, 32) {
			let dev = AudioDevice::new(self.async_client.as_client(), channels, name).map_err(|_|())?;
			let guidev = GuiAudioDevice { info: dev.info(), takes: Vec::new() };
			self.command_channel.send_message(Message::UpdateAudioDevice(id, Some(dev)))?;
			self.devices.insert(id, guidev);
			Ok(id)
		}
		else {
			Err(())
		}
	}
	pub fn add_mididevice(&mut self, name: &str) -> Result<usize,()> {
		if let Some(id) = find_first_free_index(&self.mididevices, 32) {
			let dev = MidiDevice::new(self.async_client.as_client(), name).map_err(|_|())?;
			let guidev = GuiMidiDevice { info: dev.info(), takes: Vec::new() };
			self.command_channel.send_message(Message::UpdateMidiDevice(id, Some(dev)))?;
			self.mididevices.insert(id, guidev);
			Ok(id)
		}
		else {
			Err(())
		}
	}

	pub fn add_audiotake(&mut self, audiodev_id: usize, unmuted: bool) -> Result<u32,()> {
		let id = self.next_id.gen();

		let n_channels = self.devices[&audiodev_id].info.n_channels;
		let take = AudioTake {
			samples: (0..n_channels).map(|_| Buffer::new(1024*8,512*8)).collect(),
			record_state: RecordState::Waiting,
			id,
			audiodev_id,
			unmuted,
			playing: false,
			started_recording_at: 0
		};
		let take_node = Box::new(AudioTakeNode::new(take));

		self.command_channel.send_message(Message::NewAudioTake(take_node))?;
		self.devices.get_mut(&audiodev_id).unwrap().takes.push(GuiAudioTake{id, audiodev_id, unmuted});
		Ok(id)
	}

	pub fn add_miditake(&mut self, mididev_id: usize, unmuted: bool) -> Result<u32,()> {
		let id = self.next_id.gen();

		let take = MidiTake {
			events: Buffer::new(1024, 512),
			record_state: RecordState::Waiting,
			id,
			mididev_id,
			unmuted,
			unmuted_old: unmuted,
			playing: false,
			started_recording_at: 0,
			current_position: 0,
			duration: 0,
			note_registry: RefCell::new(MidiNoteRegistry::new())
		};
		let take_node = Box::new(MidiTakeNode::new(take));

		self.command_channel.send_message(Message::NewMidiTake(take_node))?;
		self.mididevices.get_mut(&mididev_id).unwrap().takes.push(GuiMidiTake{id, mididev_id, unmuted});
		Ok(id)
	}

	pub fn toggle_audiotake_muted(&mut self, audiodev_id: usize, take_id: usize) -> Result<(),()> {
		let take = &mut self.devices.get_mut(&audiodev_id).unwrap().takes[take_id];
		let old_unmuted = take.unmuted;
		self.command_channel.send_message(Message::SetAudioMute(take.id, old_unmuted))?;
		take.unmuted = !old_unmuted;
		Ok(())
	}
	pub fn toggle_miditake_muted(&mut self, audiodev_id: usize, take_id: usize) -> Result<(),()> {
		let take = &mut self.mididevices.get_mut(&audiodev_id).unwrap().takes[take_id];
		let old_unmuted = take.unmuted;
		self.command_channel.send_message(Message::SetMidiMute(take.id, old_unmuted))?;
		take.unmuted = !old_unmuted;
		Ok(())
	}
}

fn pad_option_vec<T>(vec: Vec<T>, size: usize) -> Vec<Option<T>> {
	let n = vec.len();
	vec.into_iter().map(|v| Some(v))
		.chain( (n..size).map(|_| None) )
		.collect()
}

pub fn create_thread_states(client: jack::Client, devices: Vec<AudioDevice>, mididevices: Vec<MidiDevice>, metronome: AudioMetronome, song_length: u32) -> (FrontendThreadState, realtime_send_queue::Consumer<Event>) {
	let shared = Arc::new(SharedThreadState {
		song_length: AtomicU32::new(1),
		song_position: AtomicU32::new(0),
		transport_position: AtomicU32::new(0),
	});

	let (take_sender, take_receiver) = ringbuf::RingBuffer::<Message>::new(10).split();

	let frontend_devices = devices.iter().enumerate().map(|d| (d.0, GuiAudioDevice { info: d.1.info(), takes: Vec::new() }) ).collect();
	let frontend_mididevices = mididevices.iter().enumerate().map(|d| (d.0, GuiMidiDevice { info: d.1.info(), takes: Vec::new() }) ).collect();

	let (event_producer, event_consumer) = realtime_send_queue::new(64);

	let (destruction_sender, mut destruction_receiver) = ringbuf::RingBuffer::<DestructionRequest>::new(32).split();
	let destructor_thread_handle = std::thread::spawn(move || {
		loop {
			std::thread::park();
			println!("Handling deconstruction request");
			while let Some(request) = destruction_receiver.pop() {
				match request {
					DestructionRequest::AudioDevice(dev) => std::mem::drop(dev),
					DestructionRequest::MidiDevice(dev) => std::mem::drop(dev),
					DestructionRequest::End => {println!("destructor thread exiting..."); break;}
				}
			}
		}
	});

	let mut audio_thread_state = AudioThreadState {
		devices: pad_option_vec(devices, 32),
		mididevices: pad_option_vec(mididevices, 32),
		metronome,
		audiotakes: LinkedList::new(AudioTakeAdapter::new()),
		miditakes: LinkedList::new(MidiTakeAdapter::new()),
		command_channel: take_receiver,
		transport_position: 0,
		song_position: 0,
		song_length,
		shared: Arc::clone(&shared),
		event_channel: event_producer,
		destructor_thread_handle,
		destructor_channel: destruction_sender
	};
	
	let process_callback = move |client: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
		audio_thread_state.process_callback(client, ps)
	};
	let process = jack::ClosureProcessHandler::new(process_callback);
	let active_client = client.activate_async(Notifications, process).unwrap();


	let frontend_thread_state = FrontendThreadState {
		command_channel: RetryChannelPush(take_sender),
		devices: frontend_devices,
		mididevices: frontend_mididevices,
		shared: Arc::clone(&shared),
		next_id: IdGenerator::new(),
		async_client: Box::new(active_client)
	};


	return (frontend_thread_state, event_consumer);
}

impl Drop for AudioThreadState {
	fn drop(&mut self) {
		println!("\n\n\n############# Dropping AudioThreadState\n\n\n");
		self.event_channel.send(Event::Kill).ok();
		self.destructor_channel.push(DestructionRequest::End).ok();
	}
}

impl AudioThreadState {
	fn process_callback(&mut self, _client: &jack::Client, scope: &jack::ProcessScope) -> jack::Control {
		//println!("process from thread #{:?}", std::thread::current().id());
		use RecordState::*;
		assert_no_alloc(||{

		self.metronome.process(self.song_position, self.song_length / 8, 4, scope);


		assert!(scope.n_frames() < self.song_length);

		use std::io::Write;
		std::io::stdout().flush().unwrap();

		// first, handle the take channel
		loop {
			match self.command_channel.pop() {
				Some(msg) => {
					match msg {
						Message::UpdateAudioDevice(id, mut device) => {
							// FrontendThreadState has verified that audiodev_id isn't currently used by any take
							if cfg!(debug_assertions) {
								for take in self.audiotakes.iter() {
									debug_assert!(take.take.borrow().audiodev_id != id);
								}
							}

							std::mem::swap(&mut self.devices[id], &mut device);
							
							if let Some(old) = device {
								println!("submitting deconstruction request");
								if self.destructor_channel.push(DestructionRequest::AudioDevice(old)).is_err() {
									panic!("Failed to submit deconstruction request");
								}
								self.destructor_thread_handle.thread().unpark();
							}
						}
						Message::UpdateMidiDevice(id, mut device) => {
							// FrontendThreadState has verified that audiodev_id isn't currently used by any take
							if cfg!(debug_assertions) {
								for take in self.miditakes.iter() {
									debug_assert!(take.take.borrow().mididev_id != id);
								}
							}

							std::mem::swap(&mut self.mididevices[id], &mut device);

							if let Some(old) = device {
								println!("submitting deconstruction request");
								if self.destructor_channel.push(DestructionRequest::MidiDevice(old)).is_err() {
									panic!("Failed to submit deconstruction request");
								}
								self.destructor_thread_handle.thread().unpark();
							}
						}
						Message::NewAudioTake(take) => { println!("\ngot take"); self.audiotakes.push_back(take); }
						Message::NewMidiTake(take) => { println!("\ngot miditake"); self.miditakes.push_back(take); }
						Message::SetAudioMute(id, muted) => {
							// FIXME this is not nice...
							let mut cursor = self.audiotakes.front();
							while let Some(node) = cursor.get() {
								let mut t = node.take.borrow_mut();
								if t.id == id {
									t.unmuted = !muted;
									break;
								}
								cursor.move_next();
							}
							if cursor.get().is_none() {
								panic!("could not find take to mute");
							}
						}
						Message::SetMidiMute(id, muted) => {
							// FIXME this is not nice...
							let mut cursor = self.miditakes.front();
							while let Some(node) = cursor.get() {
								let mut t = node.take.borrow_mut();
								if t.id == id {
									t.unmuted = !muted;
									break;
								}
								cursor.move_next();
							}
							if cursor.get().is_none() {
								panic!("could not find miditake to mute");
							}
						}
						_ => { unimplemented!() }
					}
				}
				None => { break; }
			}
		}

		// then, handle all playing takes
		for dev in self.devices.iter_mut() {
			if let Some(d) = dev {
				play_silence(scope,d,0..scope.n_frames() as usize);
			}
		}
		
		let mut cursor = self.audiotakes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			let dev = self.devices[t.audiodev_id].as_mut().unwrap();
			// we assume that all channels have the same latencies.
			let playback_latency = dev.channels[0].out_port.get_latency_range(jack::LatencyType::Playback).1;

			let song_position = (self.song_position + self.song_length + playback_latency) % self.song_length;
			let song_position_after = song_position + scope.n_frames();
			let song_wraps = self.song_length <= song_position_after;
			let song_wraps_at = min(self.song_length - song_position, scope.n_frames()) as usize;

			if t.playing {
				t.playback(scope,dev, 0..scope.n_frames() as usize);
				if song_wraps { println!("\n10/10 would rewind\n"); }
			}
			else if t.record_state == Recording {
				if song_wraps {
					t.playing = true;
					println!("\nAlmost finished recording on device {}, thus starting playback now", t.audiodev_id);
					println!("Recording started at {}, now is {}", t.started_recording_at, self.transport_position + song_wraps_at as u32);
					t.rewind();
					t.playback(scope,dev, song_wraps_at..scope.n_frames() as usize);
				}
			}

			cursor.move_next();
		}

		// then, handle all playing MIDI takes
		let mut cursor = self.miditakes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			let dev = self.mididevices[t.mididev_id].as_mut().unwrap();
			let playback_latency = dev.out_port.get_latency_range(jack::LatencyType::Playback).1;

			let song_position = (self.song_position + self.song_length + playback_latency) % self.song_length;
			let song_position_after = song_position + scope.n_frames();
			let song_wraps = self.song_length <= song_position_after;
			let song_wraps_at = min(self.song_length - song_position, scope.n_frames()) as usize;
			


			
			if t.playing {
				t.playback(dev, 0..scope.n_frames() as usize);
				if song_wraps { println!("\n10/10 would rewind\n"); }
			}
			else if t.record_state == Recording {
				if song_wraps {
					t.playing = true;
					println!("\nAlmost finished recording on midi device {}, thus starting playback now", t.mididev_id);
					println!("Recording started at {}, now is {}", t.started_recording_at, self.transport_position + song_wraps_at as u32);
					t.rewind();
					t.playback(dev, song_wraps_at..scope.n_frames() as usize);
				}
			}

			cursor.move_next();
		}
		
		for dev in self.mididevices.iter_mut() {
			if let Some(d) = dev {
				d.commit_out_buffer(scope);
			}
		}
		

		// then, handle all armed takes and record into them
		let mut cursor = self.audiotakes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			let dev = self.devices[t.audiodev_id].as_ref().unwrap();
			// we assume that all channels have the same latencies.
			let capture_latency = dev.channels[0].in_port.get_latency_range(jack::LatencyType::Capture).1;
		
			let song_position = (self.song_position + self.song_length - capture_latency) % self.song_length;
			let song_position_after = song_position + scope.n_frames();
			let song_wraps = self.song_length <= song_position_after;
			let song_wraps_at = min(self.song_length - song_position, scope.n_frames());

			
			if t.record_state == Recording {
				t.record(scope,dev, 0..song_wraps_at as usize);

				if song_wraps {
					println!("\nFinished recording on device {}", t.audiodev_id);
					self.event_channel.send_or_complain(Event::AudioTakeStateChanged(t.audiodev_id, t.id, RecordState::Finished));
					t.record_state = Finished;
				}
			}
			else if t.record_state == Waiting {
				if song_wraps {
					println!("\nStarted recording on device {}", t.audiodev_id);
					self.event_channel.send_or_complain(Event::AudioTakeStateChanged(t.audiodev_id, t.id, RecordState::Recording));
					t.record_state = Recording;
					t.started_recording_at = self.transport_position + song_wraps_at;
					t.record(scope, dev, song_wraps_at as usize ..scope.n_frames() as usize);
				}
			}

			cursor.move_next();
		}

		// then, handle all armed MIDI takes and record into them
		let mut cursor = self.miditakes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			let dev = self.mididevices[t.mididev_id].as_ref().unwrap();
			// we assume that all channels have the same latencies.
			let capture_latency = dev.in_port.get_latency_range(jack::LatencyType::Capture).1;
		
			let song_position = (self.song_position + self.song_length - capture_latency) % self.song_length;
			let song_position_after = song_position + scope.n_frames();
			let song_wraps = self.song_length <= song_position_after;
			let song_wraps_at = min(self.song_length - song_position, scope.n_frames());

			
			if t.record_state == Recording {
				t.record(scope,dev, 0..song_wraps_at as usize);

				if song_wraps {
					println!("\nFinished recording on device {}", t.mididev_id);
					self.event_channel.send_or_complain(Event::MidiTakeStateChanged(t.mididev_id, t.id, RecordState::Finished));
					t.record_state = Finished;
				}
			}
			else if t.record_state == Waiting {
				if song_wraps {
					println!("\nStarted recording on device {}", t.mididev_id);
					self.event_channel.send_or_complain(Event::MidiTakeStateChanged(t.mididev_id, t.id, RecordState::Recording));
					t.record_state = Recording;
					t.started_recording_at = self.transport_position + song_wraps_at;
					t.record(scope, dev, song_wraps_at as usize ..scope.n_frames() as usize);
				}
			}

			cursor.move_next();
		}

		self.song_position = (self.song_position + scope.n_frames()) % self.song_length;
		self.transport_position += scope.n_frames();

		self.shared.song_length.store(self.song_length, std::sync::atomic::Ordering::Relaxed);
		self.shared.song_position.store(self.song_position, std::sync::atomic::Ordering::Relaxed);
		self.shared.transport_position.store(self.transport_position, std::sync::atomic::Ordering::Relaxed);
		});

		jack::Control::Continue
	}
}

pub trait IntoJackClient : Drop + Send {
	fn as_client<'a>(&'a self) -> &'a jack::Client;
	fn deactivate(self) -> Result<jack::Client, jack::Error>;
}

impl<N, P> IntoJackClient for jack::AsyncClient<N, P>
where
    N: 'static + Send + Sync + jack::NotificationHandler,
    P: 'static + Send + jack::ProcessHandler
{
	fn as_client<'a>(&'a self) -> &'a jack::Client {
		self.as_client()
	}
	fn deactivate(self) -> Result<jack::Client, jack::Error>{
		self.deactivate().map(|client_and_callbacks_tuple| client_and_callbacks_tuple.0)
	}
}

pub fn launch() -> (FrontendThreadState, realtime_send_queue::Consumer<Event>) {
	let (client, _status) = jack::Client::new("loopfisch", jack::ClientOptions::NO_START_SERVER).unwrap();

	println!("JACK running with sampling rate {} Hz, buffer size = {} samples", client.sample_rate(), client.buffer_size());

	let audiodev = AudioDevice::new(&client, 2, "fnord").unwrap();
	let audiodev2 = AudioDevice::new(&client, 2, "dronf").unwrap();
	let mididev = MidiDevice::new(&client, "midi").unwrap();
	let mididev2 = MidiDevice::new(&client, "midi2").unwrap();
	let devices = vec![audiodev, audiodev2];
	let mididevs = vec![mididev, mididev2];

	let metronome = AudioMetronome::new(&client).unwrap();

	let loop_length = client.sample_rate() as u32 * 4;
	let (frontend_thread_state, event_queue) = create_thread_states(client, devices, mididevs, metronome, loop_length);


	frontend_thread_state.async_client.as_client().connect_ports_by_name("loopfisch:fnord_out1", "system:playback_1").unwrap();
	frontend_thread_state.async_client.as_client().connect_ports_by_name("loopfisch:fnord_out2", "system:playback_2").unwrap();
	frontend_thread_state.async_client.as_client().connect_ports_by_name("system:capture_1", "loopfisch:fnord_in1").unwrap();
	frontend_thread_state.async_client.as_client().connect_ports_by_name("system:capture_2", "loopfisch:fnord_in2").unwrap();

	return (frontend_thread_state, event_queue);
}

fn play_silence(scope: &jack::ProcessScope, device: &mut AudioDevice, range: std::ops::Range<usize>) {
	for channel_ports in device.channels.iter_mut() {
		let buffer = &mut channel_ports.out_port.as_mut_slice(scope)[range.clone()];
		for d in buffer {
			*d = 0.0;
		}
	}
}

pub struct MidiTake {
	/// Sorted sequence of all events with timestamps between 0 and self.duration
	events: Buffer<MidiMessage>,
	/// Current playhead position
	current_position: u32,
	/// Number of frames after which the recorded events shall loop.
	duration: u32,
	record_state: RecordState,
	pub id: u32,
	pub mididev_id: usize,
	pub unmuted: bool,
	pub unmuted_old: bool,
	pub playing: bool,
	pub started_recording_at: u32,
	note_registry: RefCell<MidiNoteRegistry> // this SUCKS. TODO.
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

pub struct AudioTake {
	/// Sequence of all samples. The take's duration and playhead position are implicitly managed by the underlying Buffer.
	samples: Vec<Buffer<f32>>,
	record_state: RecordState,
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

pub struct GuiAudioTake {
	pub id: u32,
	pub audiodev_id: usize,
	pub unmuted: bool
}

pub struct GuiMidiTake {
	pub id: u32,
	pub mididev_id: usize,
	pub unmuted: bool
}

impl MidiTake {
	/// Enumerates all events that take place in the next `range.len()` frames and puts
	/// them into device's playback queue. The events are automatically looped every
	/// `self.duration` frames.
	pub fn playback(&mut self, device: &mut MidiDevice, range: std::ops::Range<usize>) {
		if self.unmuted != self.unmuted_old {
			if !self.unmuted {
				self.note_registry.borrow_mut().stop_playing(device);
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
					note_registry.register_event(event.data);
				}

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

	pub fn record(&mut self, scope: &jack::ProcessScope, device: &MidiDevice, range: std::ops::Range<usize>) {
		use std::convert::TryInto;
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

impl AudioTake {
	pub fn playback(&mut self, scope: &jack::ProcessScope, device: &mut AudioDevice, range: std::ops::Range<usize>) {
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

	pub fn record(&mut self, scope: &jack::ProcessScope, device: &AudioDevice, range: std::ops::Range<usize>) {
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

