// loopfisch -- A loop machine written in rust.
// Copyright (C) 2020 Florian Jung <flo@windfis.ch>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License verion 3 as
// published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use jack;
use smallvec;
use std::cmp::min;

use crossterm;

mod bit_array;
use bit_array::BitArray2048;

mod jack_driver;
use jack_driver::*; // FIXME


use assert_no_alloc::assert_no_alloc;

#[cfg(debug_assertions)]
#[global_allocator]
static A: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

mod outsourced_allocation_buffer;
use outsourced_allocation_buffer::Buffer;

mod metronome;
use metronome::AudioMetronome;

mod midi_message;
use midi_message::{MidiEvent,MidiMessage};

mod frontend_data;
use frontend_data::AudioDeviceInfo;

#[derive(std::cmp::PartialEq)]
enum RecordState {
	Waiting,
	Recording,
	Finished
}

use intrusive_collections::{intrusive_adapter, LinkedList, LinkedListLink};

struct MidiTake {
	/// Sorted sequence of all events with timestamps between 0 and self.duration
	events: Buffer<MidiMessage>,
	/// Current playhead position
	current_position: u32,
	/// Number of frames after which the recorded events shall loop.
	duration: u32,
	record_state: RecordState,
	id: u32,
	mididev_id: usize,
	unmuted: bool,
	unmuted_old: bool,
	playing: bool,
	started_recording_at: u32,
	note_registry: RefCell<MidiNoteRegistry> // this SUCKS. TODO.
}

struct Take {
	/// Sequence of all samples. The take's duration and playhead position are implicitly managed by the underlying Buffer.
	samples: Vec<Buffer<f32>>,
	record_state: RecordState,
	id: u32,
	dev_id: usize,
	unmuted: bool,
	playing: bool,
	started_recording_at: u32,

}

struct GuiTake {
	id: u32,
	dev_id: usize,
	unmuted: bool
}

struct GuiMidiTake {
	id: u32,
	mididev_id: usize,
	unmuted: bool
}

struct MidiNoteRegistry {
	playing_notes: BitArray2048
}

impl MidiNoteRegistry {
	pub fn new() -> MidiNoteRegistry {
		MidiNoteRegistry { playing_notes: BitArray2048::new() }
	}

	fn register_event(&mut self, data: [u8; 3]) {
		use MidiEvent::*;
		match MidiEvent::parse(&data) {
			NoteOn(channel, note, _) => {
				self.playing_notes.set(note as u32 + 128*channel as u32, true);
			}
			NoteOff(channel, note, _) => {
				self.playing_notes.set(note as u32 + 128*channel as u32, false);
			}
			_ => {}
		}
	}

	pub fn stop_playing(&mut self, device: &mut MidiDevice) {
		// FIXME: queue_event could fail; better allow for a "second chance"
		for channel in 0..16 {
			for note in 0..128 {
				if self.playing_notes.get(note as u32 + 128*channel as u32) {
					device.queue_event( MidiMessage {
						timestamp: 0,
						data: [0x80 | channel, note, 64]
					}).unwrap();
				}
			}
		}
		self.playing_notes = BitArray2048::new(); // clear the array
	}
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
					self.events.push( MidiMessage {
						timestamp,
						data
					});
					// TODO: assert that this is monotonic
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
				channel_buffer.push(*d);
			}
		}
	}
}

fn play_silence(scope: &jack::ProcessScope, device: &mut AudioDevice, range: std::ops::Range<usize>) {
	for channel_ports in device.channels.iter_mut() {
		let buffer = &mut channel_ports.out_port.as_mut_slice(scope)[range.clone()];
		for d in buffer {
			*d = 0.0;
		}
	}
}


use std::cell::RefCell;

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

struct AudioThreadState {
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

use std::sync::atomic::*;
use std::sync::Arc;

struct SharedThreadState {
	song_length: AtomicU32,
	song_position: AtomicU32,
	transport_position: AtomicU32,
}

struct FrontendThreadState {
	new_take_channel: ringbuf::Producer<Message>,
	device_info: Vec<AudioDeviceInfo>,
	shared: Arc<SharedThreadState>,
	takes: Vec<GuiTake>,
	miditakes: Vec<GuiMidiTake>,
	id_counter: u32
}

impl FrontendThreadState {
	fn add_take(&mut self, dev_id: usize) -> Result<(),()> {
		let id = self.id_counter;

		let n_channels = self.device_info[dev_id].n_channels;
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
			self.takes.push(GuiTake{id, dev_id, unmuted: true});
			self.id_counter += 1;
			Ok(())
		}
		else {
			Err(())
		}
	}

	fn add_miditake(&mut self, mididev_id: usize) -> Result<(),()> {
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
			self.miditakes.push(GuiMidiTake{id, mididev_id, unmuted: true});
			self.id_counter += 1;
			Ok(())
		}
		else {
			Err(())
		}
	}

	fn toggle_take_muted(&mut self, take_id: usize) -> Result<(),()> {
		let old_unmuted = self.takes[take_id].unmuted;
		if self.new_take_channel.push(Message::SetMute(self.takes[take_id].id, old_unmuted)).is_ok() {
			self.takes[take_id].unmuted = !old_unmuted;
			Ok(())
		}
		else {
			Err(())
		}
	}
	fn toggle_miditake_muted(&mut self, take_id: usize) -> Result<(),()> {
		let old_unmuted = self.miditakes[take_id].unmuted;
		if self.new_take_channel.push(Message::SetMidiMute(self.miditakes[take_id].id, old_unmuted)).is_ok() {
			self.miditakes[take_id].unmuted = !old_unmuted;
			Ok(())
		}
		else {
			Err(())
		}
	}
}

fn create_thread_states(devices: Vec<AudioDevice>, mididevices: Vec<MidiDevice>, metronome: AudioMetronome, song_length: u32) -> (AudioThreadState, FrontendThreadState) {

	let shared = Arc::new(SharedThreadState {
		song_length: AtomicU32::new(1),
		song_position: AtomicU32::new(0),
		transport_position: AtomicU32::new(0),
	});

	let (take_sender, take_receiver) = ringbuf::RingBuffer::<Message>::new(10).split();

	let frontend_thread_state = FrontendThreadState {
		new_take_channel: take_sender,
		device_info: devices.iter().map(|d| d.info()).collect(),
		shared: Arc::clone(&shared),
		takes: Vec::new(),
		miditakes: Vec::new(),
		id_counter: 0
	};

	let audio_thread_state = AudioThreadState {
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

	return (audio_thread_state, frontend_thread_state);
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

struct Notifications;
impl jack::NotificationHandler for Notifications {
	fn thread_init(&self, _: &jack::Client) {
		println!("JACK: thread init");
	}

	fn latency(&mut self, _: &jack::Client, _mode: jack::LatencyType) {
		println!("latency callback from thread #{:?}", std::thread::current().id());
	}

	fn shutdown(&mut self, status: jack::ClientStatus, reason: &str) {
		println!(
				"JACK: shutdown with status {:?} because \"{}\"",
				status, reason
				);
	}
}


static LETTERS: [char;26] = ['q','w','e','r','t','y','u','i','o','p','a','s','d','f','g','h','j','k','l','z','x','c','v','b','n','m'];
fn letter2id(c: char) -> u32 {
	for (i,l) in LETTERS.iter().enumerate() {
		if *l==c { return i as u32; }
	}
	panic!("letter2id must be called with a letter");
}
//fn id2letter(i: u32) -> char {
//	return LETTERS[i as usize];
//}

struct UserInterface {
	dev_id: usize
}

impl UserInterface {
	fn new() -> UserInterface {
		UserInterface {
			dev_id: 0
		}
	}

	fn redraw(&self, frontend_thread_state: &FrontendThreadState) {
		use std::io::{Write, stdout};
		use crossterm::cursor::*;
		use crossterm::terminal::*;
		use crossterm::*;
		execute!( stdout(),
			Clear(ClearType::All),
			MoveTo(0,0)
		).unwrap();

		let song_length = frontend_thread_state.shared.song_length.load(std::sync::atomic::Ordering::Relaxed);
		let song_position = frontend_thread_state.shared.song_position.load(std::sync::atomic::Ordering::Relaxed);
		let transport_position = frontend_thread_state.shared.transport_position.load(std::sync::atomic::Ordering::Relaxed);
		print!("Transport position: {}     \r\n", transport_position);
		print!("Song position: {:5.1}% {:2x} {:1} {}      \r\n", (song_position as f32 / song_length as f32) * 100.0, 256*song_position / song_length, 8 * song_position / song_length, song_position);
		print!("Selected device: {}    \r\n", self.dev_id);
	}

	fn handle_input(&mut self, frontend_thread_state: &mut FrontendThreadState) -> crossterm::Result<bool> {
		use std::time::Duration;
		use crossterm::event::{Event,KeyModifiers,KeyEvent,KeyCode};
		while crossterm::event::poll(Duration::from_millis(16))? {
			let ev = crossterm::event::read()?;
			match ev {
				crossterm::event::Event::Key(kev) => {
					//println!("key: {:?}", kev);

					if kev.code == KeyCode::Char('c') && kev.modifiers == KeyModifiers::CONTROL {
						return Ok(true);
					}

					match kev.code {
						KeyCode::Char(c) => {
							match c {
								'0'..='9' => {
									self.dev_id = c as usize - '0' as usize;
								}
								'a'..='z' => {
									let num = letter2id(c);
									if num <= letter2id('p') {
										frontend_thread_state.toggle_take_muted(num as usize).unwrap();
									}
									else if num <= letter2id('l') {
										frontend_thread_state.toggle_miditake_muted((num - letter2id('a')) as usize).unwrap();
									}
									else {
										match c {
											'z' => {frontend_thread_state.add_take(self.dev_id).unwrap();}
											'x' => {frontend_thread_state.add_miditake(self.dev_id).unwrap();}
											_ => {}
										}
									}
								}
								_ => {}
							}
						}
						_ => {}
					}
				}
				_ => {}
			}
		}
		Ok(false)
	}
	
	fn spin(&mut self, frontend_thread_state: &mut FrontendThreadState) -> crossterm::Result<bool> {
		self.redraw(frontend_thread_state);
		self.handle_input(frontend_thread_state)
	}
}

fn main() {
    println!("Hello, world!");

	let (client, _status) = jack::Client::new("loopfisch", jack::ClientOptions::NO_START_SERVER).unwrap();

	println!("JACK running with sampling rate {} Hz, buffer size = {} samples", client.sample_rate(), client.buffer_size());

	let audiodev = AudioDevice::new(&client, 2, "fnord").unwrap();
	let audiodev2 = AudioDevice::new(&client, 2, "dronf").unwrap();
	let mididev = MidiDevice::new(&client, "midi").unwrap();
	let mididev2 = MidiDevice::new(&client, "midi2").unwrap();
	let devices = vec![audiodev, audiodev2];
	let mididevs = vec![mididev, mididev2];

	let metronome = AudioMetronome::new(&client).unwrap();
	
	let (mut audio_thread_state, mut frontend_thread_state) = create_thread_states(devices, mididevs, metronome, client.sample_rate() as u32 * 4);

	let process_callback = move |client: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
		audio_thread_state.process_callback(client, ps)
	};
	let process = jack::ClosureProcessHandler::new(process_callback);
	let active_client = client.activate_async(Notifications, process).unwrap();

	active_client.as_client().connect_ports_by_name("loopfisch:fnord_out1", "system:playback_1").unwrap();
	active_client.as_client().connect_ports_by_name("loopfisch:fnord_out2", "system:playback_2").unwrap();
	active_client.as_client().connect_ports_by_name("system:capture_1", "loopfisch:fnord_in1").unwrap();
	active_client.as_client().connect_ports_by_name("system:capture_2", "loopfisch:fnord_in2").unwrap();
	
	let mut ui = UserInterface::new();
	crossterm::terminal::enable_raw_mode().unwrap();
	loop {
		if ui.spin(&mut frontend_thread_state).unwrap() {
			break;
		}
	}
	crossterm::terminal::disable_raw_mode().unwrap();
}
