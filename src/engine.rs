use core::cmp::min;
use intrusive_collections::{intrusive_adapter, LinkedList, LinkedListLink};
use std::sync::atomic::*;
use std::sync::Arc;
use std::cell::RefCell;

use crate::midi_message::MidiMessage;

use crate::jack_driver::*;

use crate::midi_registry::MidiNoteRegistry;
use crate::metronome::AudioMetronome;

use crate::outsourced_allocation_buffer::Buffer;

use assert_no_alloc::assert_no_alloc;

#[derive(std::cmp::PartialEq)]
enum RecordState {
	Waiting,
	Recording,
	Finished
}

struct TakeNode {
	take: RefCell<Take>,
	link: LinkedListLink
}

impl TakeNode {
	fn new(take: Take) -> TakeNode {
		TakeNode {
			take: RefCell::new(take),
			link: LinkedListLink::new()
		}
	}
}

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

intrusive_adapter!(TakeAdapter = Box<TakeNode>: TakeNode { link: LinkedListLink });
intrusive_adapter!(MidiTakeAdapter = Box<MidiTakeNode>: MidiTakeNode { link: LinkedListLink });

enum Message {
	NewTake(Box<TakeNode>),
	NewMidiTake(Box<MidiTakeNode>),
	SetMute(u32,bool),
	SetMidiMute(u32,bool),
	DeleteTake(u32)
}

pub struct AudioThreadState {
	devices: Vec<AudioDevice>,
	mididevices: Vec<MidiDevice>,
	metronome: AudioMetronome,
	takes: LinkedList<TakeAdapter>,
	miditakes: LinkedList<MidiTakeAdapter>,
	new_take_channel: ringbuf::Consumer<Message>,
	transport_position: u32, // does not wrap 
	song_position: u32, // wraps
	song_length: u32,
	shared: Arc<SharedThreadState>
}

pub struct SharedThreadState {
	pub song_length: AtomicU32,
	pub song_position: AtomicU32,
	pub transport_position: AtomicU32,
}

pub struct GuiAudioDevice {
	info: AudioDeviceInfo,
	takes: Vec<GuiTake>,
}

impl GuiAudioDevice {
	pub fn info(&self) -> &AudioDeviceInfo { &self.info }
	pub fn takes(&self) -> &Vec<GuiTake> { &self.takes }
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
	new_take_channel: ringbuf::Producer<Message>,
	devices: Vec<GuiAudioDevice>,
	mididevices: Vec<GuiMidiDevice>,
	pub shared: Arc<SharedThreadState>,
	id_counter: u32,
	async_client: Box<dyn IntoJackClient>
}

impl FrontendThreadState {
	pub fn devices(&self) -> &Vec<GuiAudioDevice> { &self.devices}
	pub fn mididevices(&self) -> &Vec<GuiMidiDevice> { &self.mididevices}

	pub fn add_device(&mut self, name: &str, channels: u32, audio: bool, midi: bool) {
		//AudioDevice::new(channels, &format!("{}_audio", name));
	}

	pub fn add_take(&mut self, dev_id: usize) -> Result<(),()> {
		let id = self.id_counter;

		let n_channels = self.devices[dev_id].info.n_channels;
		let take = Take {
			samples: (0..n_channels).map(|_| Buffer::new(1024*8,512*8)).collect(),
			record_state: RecordState::Waiting,
			id,
			dev_id,
			unmuted: true,
			playing: false,
			started_recording_at: 0
		};
		let take_node = Box::new(TakeNode::new(take));

		if self.new_take_channel.push(Message::NewTake(take_node)).is_ok() {
			self.devices[dev_id].takes.push(GuiTake{id, dev_id, unmuted: true});
			self.id_counter += 1;
			Ok(())
		}
		else {
			Err(())
		}
	}

	pub fn add_miditake(&mut self, mididev_id: usize) -> Result<(),()> {
		let id = self.id_counter;

		let take = MidiTake {
			events: Buffer::new(1024, 512),
			record_state: RecordState::Waiting,
			id,
			mididev_id,
			unmuted: true,
			unmuted_old: true,
			playing: false,
			started_recording_at: 0,
			current_position: 0,
			duration: 0,
			note_registry: RefCell::new(MidiNoteRegistry::new())
		};
		let take_node = Box::new(MidiTakeNode::new(take));

		if self.new_take_channel.push(Message::NewMidiTake(take_node)).is_ok() {
			self.mididevices[mididev_id].takes.push(GuiMidiTake{id, mididev_id, unmuted: true});
			self.id_counter += 1;
			Ok(())
		}
		else {
			Err(())
		}
	}

	pub fn toggle_take_muted(&mut self, dev_id: usize, take_id: usize) -> Result<(),()> {
		let take = &mut self.devices[dev_id].takes[take_id];
		let old_unmuted = take.unmuted;
		if self.new_take_channel.push(Message::SetMute(take.id, old_unmuted)).is_ok() {
			take.unmuted = !old_unmuted;
			Ok(())
		}
		else {
			Err(())
		}
	}
	pub fn toggle_miditake_muted(&mut self, dev_id: usize, take_id: usize) -> Result<(),()> {
		let take = &mut self.mididevices[dev_id].takes[take_id];
		let old_unmuted = take.unmuted;
		if self.new_take_channel.push(Message::SetMidiMute(take.id, old_unmuted)).is_ok() {
			take.unmuted = !old_unmuted;
			Ok(())
		}
		else {
			Err(())
		}
	}
}

pub fn create_thread_states(client: jack::Client, devices: Vec<AudioDevice>, mididevices: Vec<MidiDevice>, metronome: AudioMetronome, song_length: u32) -> FrontendThreadState {

	let shared = Arc::new(SharedThreadState {
		song_length: AtomicU32::new(1),
		song_position: AtomicU32::new(0),
		transport_position: AtomicU32::new(0),
	});

	let (take_sender, take_receiver) = ringbuf::RingBuffer::<Message>::new(10).split();

	let frontend_devices = devices.iter().map(|d| GuiAudioDevice { info: d.info(), takes: Vec::new() } ).collect();
	let frontend_mididevices = mididevices.iter().map(|d| GuiMidiDevice { info: d.info(), takes: Vec::new() } ).collect();

	let mut audio_thread_state = AudioThreadState {
		devices,
		mididevices,
		metronome,
		takes: LinkedList::new(TakeAdapter::new()),
		miditakes: LinkedList::new(MidiTakeAdapter::new()),
		new_take_channel: take_receiver,
		transport_position: 0,
		song_position: 0,
		song_length,
		shared: Arc::clone(&shared)
	};
	
	let process_callback = move |client: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
		audio_thread_state.process_callback(client, ps)
	};
	let process = jack::ClosureProcessHandler::new(process_callback);
	let active_client = client.activate_async(Notifications, process).unwrap();


	let frontend_thread_state = FrontendThreadState {
		new_take_channel: take_sender,
		devices: frontend_devices,
		mididevices: frontend_mididevices,
		shared: Arc::clone(&shared),
		id_counter: 0,
		async_client: Box::new(active_client)
	};


	return frontend_thread_state;
}

impl AudioThreadState {
	fn process_callback(&mut self, client: &jack::Client, scope: &jack::ProcessScope) -> jack::Control {
		//println!("process from thread #{:?}", std::thread::current().id());
		use RecordState::*;
		assert_no_alloc(||{

		self.metronome.process(self.song_position, self.song_length / 8, 4, scope);


		assert!(scope.n_frames() < self.song_length);

		use std::io::Write;
		std::io::stdout().flush().unwrap();

		// first, handle the take channel
		loop {
			match self.new_take_channel.pop() {
				Some(msg) => {
					match msg {
						Message::NewTake(take) => { println!("\ngot take"); self.takes.push_back(take); }
						Message::NewMidiTake(take) => { println!("\ngot miditake"); self.miditakes.push_back(take); }
						Message::SetMute(id, muted) => {
							// FIXME this is not nice...
							let mut cursor = self.takes.front();
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
			play_silence(scope,dev,0..scope.n_frames() as usize);
		}
		
		let mut cursor = self.takes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			let dev = &mut self.devices[t.dev_id];
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
					println!("\nAlmost finished recording on device {}, thus starting playback now", t.dev_id);
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
			let dev = &mut self.mididevices[t.mididev_id];
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
			dev.commit_out_buffer(scope);
		}
		

		// then, handle all armed takes and record into them
		let mut cursor = self.takes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			let dev = &self.devices[t.dev_id];
			// we assume that all channels have the same latencies.
			let capture_latency = dev.channels[0].in_port.get_latency_range(jack::LatencyType::Capture).1;
		
			let song_position = (self.song_position + self.song_length - capture_latency) % self.song_length;
			let song_position_after = song_position + scope.n_frames();
			let song_wraps = self.song_length <= song_position_after;
			let song_wraps_at = min(self.song_length - song_position, scope.n_frames());

			
			if t.record_state == Recording {
				t.record(scope,dev, 0..song_wraps_at as usize);

				if song_wraps {
					println!("\nFinished recording on device {}", t.dev_id);
					t.record_state = Finished;
				}
			}
			else if t.record_state == Waiting {
				if song_wraps {
					println!("\nStarted recording on device {}", t.dev_id);
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
			let dev = &self.mididevices[t.mididev_id];
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
					t.record_state = Finished;
				}
			}
			else if t.record_state == Waiting {
				if song_wraps {
					println!("\nStarted recording on device {}", t.mididev_id);
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

pub fn launch() -> FrontendThreadState {
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
	let frontend_thread_state = create_thread_states(client, devices, mididevs, metronome, loop_length);


	frontend_thread_state.async_client.as_client().connect_ports_by_name("loopfisch:fnord_out1", "system:playback_1").unwrap();
	frontend_thread_state.async_client.as_client().connect_ports_by_name("loopfisch:fnord_out2", "system:playback_2").unwrap();
	frontend_thread_state.async_client.as_client().connect_ports_by_name("system:capture_1", "loopfisch:fnord_in1").unwrap();
	frontend_thread_state.async_client.as_client().connect_ports_by_name("system:capture_2", "loopfisch:fnord_in2").unwrap();

	return frontend_thread_state;
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

pub struct Take {
	/// Sequence of all samples. The take's duration and playhead position are implicitly managed by the underlying Buffer.
	samples: Vec<Buffer<f32>>,
	record_state: RecordState,
	pub id: u32,
	pub dev_id: usize,
	pub unmuted: bool,
	pub playing: bool,
	pub started_recording_at: u32,
}

pub struct GuiTake {
	pub id: u32,
	pub dev_id: usize,
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

impl Take {
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

