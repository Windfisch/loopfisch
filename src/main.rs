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

use assert_no_alloc::assert_no_alloc;

#[cfg(debug_assertions)]
#[global_allocator]
static A: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

mod outsourced_allocation_buffer {
	use intrusive_collections::{intrusive_adapter, LinkedList, LinkedListLink};
	use std::cell::RefCell;
	use ringbuf::RingBuffer;
	use std::thread;

	struct BufferFragment<T> {
		link: LinkedListLink,
		buf: RefCell<Vec<T>>,
	}
	intrusive_adapter!(BufferFragmentAdapter<T> = Box<BufferFragment<T>>: BufferFragment<T> { link: LinkedListLink });

	enum ThreadRequest {
		Fragment,
		End
	}

	pub struct Buffer<T> {
		fragments: LinkedList<BufferFragmentAdapter<T>>,
		remaining_threshold: usize,

		request_pending: bool,
		incoming_fragment_ringbuf: ringbuf::Consumer<std::boxed::Box<BufferFragment<T>>>,
		new_fragment_request_ringbuf: ringbuf::Producer<ThreadRequest>,
		thread_handle: std::thread::JoinHandle<()>,

		iter_cursor: *const BufferFragment<T>,
		iter_index: usize
	}
	
	// instruct the helper thread to exit when this buffer goes out of scope
	impl<T> Drop for Buffer<T> {
		fn drop(&mut self) {
			println!("dropping");
			self.new_fragment_request_ringbuf.push(ThreadRequest::End).map_err(|_|()).unwrap();
			self.thread_handle.thread().unpark();
		}
	}

	unsafe impl<T: Send> Send for Buffer<T> {}
	
	impl<T: 'static + Send> Buffer<T> {
		/// Create a new buffer and launch the associated helper thread.
		/// This function is not real-time-safe and will allocate memory.
		///
		/// # Arguments
		///   * The capacity is increased in steps of `capacity_increment`. This should be
		///     a power of two, and should be at least twice as large as the largest push
		///     size.
		///   * `remaining_threshold` specifies the threshold. If less space is available,
		///     a new fragment is requested from the helper thread.
		pub fn new(capacity_increment: usize, remaining_threshold: usize) -> Buffer<T> {
			if capacity_increment < 1 {
				panic!("capacity_increment must be > 0");
			}
			let node = Box::new(BufferFragment {
				link: LinkedListLink::new(),
				buf: RefCell::new(Vec::with_capacity(capacity_increment))
			});
			let mut list = LinkedList::new(BufferFragmentAdapter::new());
			list.push_back(node);

			// 1 slot is enough because we will never have more than one pending request.
			let incoming_ringbuf = RingBuffer::<Box<BufferFragment<T>>>::new(1);
			let (mut incoming_producer, incoming_consumer) = incoming_ringbuf.split();

			// we can at most have one pending allocation request that wasn't handled yet plus
			// one "End" request. -> 2
			let request_ringbuf = RingBuffer::<ThreadRequest>::new(2);
			let (request_producer, mut request_consumer) = request_ringbuf.split();

			let thread_handle = thread::spawn(move || {
				loop {
					thread::park();
					while let Some(request) = request_consumer.pop() {
						match request {
							ThreadRequest::Fragment => {
								let fragment = Box::new(BufferFragment {
									link: LinkedListLink::new(),
									buf: RefCell::new(Vec::with_capacity(capacity_increment))
								});
								incoming_producer.push(fragment).map_err(|_|()).unwrap();
								//println!("created new fragment");
							}
							ThreadRequest::End => {
								println!("helper thread exiting");
								return;
							}
						}
					}
				}
			});

			Buffer {
				fragments: list,
				remaining_threshold,
				request_pending: false,
				incoming_fragment_ringbuf: incoming_consumer,
				new_fragment_request_ringbuf: request_producer,
				thread_handle,
				iter_cursor: std::ptr::null(),
				iter_index: 0
			}
		}

		/// Checks if the buffer is empty
		pub fn empty(&self) -> bool {
			// fragments is never empty, but the Vec in fragments.front() may be
			self.fragments.front().get().unwrap().buf.borrow().len() == 0
		}

		/// Rewind the iterator state to the beginning of the stored data.
		pub fn rewind(&mut self) {
			if !self.empty() {
				self.iter_cursor = self.fragments.front().get().unwrap(); 
				self.iter_index = 0;
			}
			else {
				self.iter_cursor = std::ptr::null();
			}
		}

		/// Execute func() on the current item (if there is one) and return the result.
		/// If there is none, return None. Then advances the cursor to the next item
		pub fn next<S,F: FnOnce(&T) -> S>(&mut self, func: F) -> Option<S> {
			if self.iter_cursor.is_null() {
				return None;
			}

			// Get a cursor from the pointer. This places a borrow on self.fragments
			// This is safe iif iter_cursor points to an element current in the list.
			// Since list elements are only added, but never removed, and since iter_cursor
			// has already belonged to the list when it was set, this is fine.
			let mut cursor = unsafe{ self.fragments.cursor_from_ptr(self.iter_cursor) };
			let buf = cursor.get().unwrap().buf.borrow();
		
			// Perform the actual access. This is always a valid element because no elements can
			// be deleted.
			let result = func(&buf[self.iter_index]);
		
			// Now advance the iterator
			if self.iter_index + 1 < buf.len() {
				self.iter_index += 1;
			}
			else {
				self.iter_index = 0;
				cursor.move_next();
			};

			// And turn the borrowed cursor into a borrow-free pointer again
			self.iter_cursor = 
				match cursor.get() {
					Some(frag) => {
						assert!(frag.buf.borrow().len() > 0);
						frag
					}
					None => {
						std::ptr::null()
					}
				};

			return Some(result);
		}

		pub fn peek<S,F:FnOnce(&T) -> S>(&mut self, func: F) -> Option<S> {
			if self.iter_cursor.is_null() {
				return None;
			}

			// Get a cursor from the pointer. This places a borrow on self.fragments
			// This is safe iif iter_cursor points to an element current in the list.
			// Since list elements are only added, but never removed, and since iter_cursor
			// has already belonged to the list when it was set, this is fine.
			let mut cursor = unsafe{ self.fragments.cursor_from_ptr(self.iter_cursor) };
			let buf = cursor.get().unwrap().buf.borrow();
		
			// Perform the actual access. This is always a valid element because no elements can
			// be deleted.
			let result = func(&buf[self.iter_index]);

			return Some(result);
		}

		/// Tries to push elem into the buffer. Fails if no capacity is available, usually
		/// because the manager thread was too slow in adding new capacity.
		pub fn push(&mut self, elem: T) -> Result<(), T> {
			let remaining = {
				let frag = self.fragments.back_mut();
				let buf = frag.get().unwrap().buf.borrow();
				buf.capacity() - buf.len()
			};

			if remaining < 1 {
				// we can't fit the data into the current fragment, let's check whether
				// a new fragment has been queued already
				match self.incoming_fragment_ringbuf.pop() {
					Some(fragment) => {
						//println!("got new fragment");
						self.fragments.push_back(fragment);
						self.request_pending = false;
					}
					None => {
						return Err(elem);
					}
				}
			}
			
			self.fragments.back_mut().get().unwrap().buf.borrow_mut().push(elem);

			if remaining < self.remaining_threshold && !self.request_pending {
				self.new_fragment_request_ringbuf.push(ThreadRequest::Fragment).map_err(|_|()).unwrap();
				self.request_pending = true;
				self.thread_handle.thread().unpark();
				//println!("requesting new fragment");
			}

			Ok(())
		}
	}
}

use outsourced_allocation_buffer::Buffer;

struct MidiMessage {
	timestamp: jack::Frames,
	data: [u8; 3]
}

struct MidiDevice {
	in_port: jack::Port<jack::MidiIn>,
	out_port: jack::Port<jack::MidiOut>,

	out_buffer: smallvec::SmallVec<[(MidiMessage, usize); 128]>,
}

impl MidiDevice {
	/// sorts the events in the out_buffer, commits them to the out_port and clears the out_buffer.
	/// FIXME: deduping
	pub fn commit_out_buffer(&mut self, scope: &jack::ProcessScope) {
		// sort
		self.out_buffer.sort_unstable_by( |a,b| a.0.timestamp.cmp(&b.0.timestamp).then(a.1.cmp(&b.1)) );

		// write
		let mut writer = self.out_port.writer(scope);
		for (msg,_idx) in self.out_buffer.iter() {
			// FIXME: do the deduping here
			writer.write(&jack::RawMidi {
				time: msg.timestamp,
				bytes: &msg.data
			}).unwrap();
		}

		// clear
		self.out_buffer.clear();
	}
	pub fn queue_event(&mut self, msg: MidiMessage) -> Result<(), ()> {
		if self.out_buffer.len() < self.out_buffer.inline_size() {
			self.out_buffer.push((msg, self.out_buffer.len()));
			Ok(())
		}
		else {
			Err(())
		}
	}
}

impl MidiDevice {
	pub fn new(client: &jack::Client, name: &str) -> Result<MidiDevice, jack::Error> {
		let in_port = client.register_port(&format!("{}_in", name), jack::MidiIn::default())?;
		let out_port = client.register_port(&format!("{}_out", name), jack::MidiOut::default())?;
		let dev = MidiDevice {
			in_port,
			out_port,
			out_buffer: smallvec::SmallVec::new()
		};
		Ok(dev)
	}
}

struct AudioChannel {
	in_port: jack::Port<jack::AudioIn>,
	out_port: jack::Port<jack::AudioOut>,
}

impl AudioChannel {
	fn new(client: &jack::Client, name: &str, num: u32) -> Result<AudioChannel, jack::Error> {
		let in_port = client.register_port(&format!("{}_in{}", name, num), jack::AudioIn::default())?;
		let out_port = client.register_port(&format!("{}_out{}", name, num), jack::AudioOut::default())?;
		//client.connect_ports(&in_port, &client.port_by_name(&format!("system:capture_{}", num)).expect("could not find port")).expect("could not connect capture port");
		//client.connect_ports(&out_port, &client.port_by_name(&format!("system:playback_{}", num)).expect("could not find port")).expect("could not connect playback port");
		return Ok( AudioChannel { in_port, out_port });
	}
}

struct AudioDevice {
	pub channels: Vec<AudioChannel>
}

impl AudioDevice {
	fn info(&self) -> AudioDeviceInfo {
		return AudioDeviceInfo {
			n_channels: self.channels.len()
		};
	}
}

struct AudioDeviceInfo {
	pub n_channels: usize
}

impl AudioDevice {
	pub fn new(client: &jack::Client, n_channels: u32, name: &str) -> Result<AudioDevice, jack::Error> {
		let dev = AudioDevice {
			channels: (0..n_channels).map(|channel| AudioChannel::new(client, name, channel+1)).collect::<Result<_,_>>()?
		};
		Ok(dev)
	}
}

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
	playing: bool,
	started_recording_at: u32,
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

impl MidiTake {
	/// Enumerates all events that take place in the next `range.len()` frames and puts
	/// them into device's playback queue. The events are automatically looped every
	/// `self.duration` frames.
	pub fn playback(&mut self, device: &mut MidiDevice, range: std::ops::Range<usize>) {
		assert!(!self.events.empty());

		let position_after = self.current_position + range.len() as u32;

		// iterate through the events until either a) we've reached the end or b) we've reached
		// an event which is past the current period.
		let curr_pos = self.current_position;
		let mut rewind_offset = 0;
		loop {
			let result = self.events.peek( |event| {
				assert!(event.timestamp >= curr_pos);
				let relative_timestamp = event.timestamp + rewind_offset - curr_pos + range.start as u32;
				assert!(relative_timestamp >= range.start as u32);
				if relative_timestamp >= range.end as u32 {
					return false; // signify "please break the loop"
				}

				device.queue_event(
					MidiMessage {
						timestamp: relative_timestamp,
						data: event.data
					}
				).unwrap();

				return true; // signify "please continue the loop"
			});

			match result {
				None => { // we hit the end of the event list
					self.events.rewind();
					rewind_offset += self.duration;
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
}

impl Take {
	pub fn playback(&mut self, client: &jack::Client, scope: &jack::ProcessScope, device: &mut AudioDevice, range: std::ops::Range<usize>) {
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

	pub fn record(&mut self, client: &jack::Client, scope: &jack::ProcessScope, device: &AudioDevice, range: std::ops::Range<usize>) {
		for (channel_buffer, channel_ports) in self.samples.iter_mut().zip(device.channels.iter()) {
			let data = &channel_ports.in_port.as_slice(scope)[range.clone()];
			for d in data {
				channel_buffer.push(*d);
			}
		}
	}
}

fn play_silence(client: &jack::Client, scope: &jack::ProcessScope, device: &mut AudioDevice, range: std::ops::Range<usize>) {
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
	SetMute(u32,bool),
	DeleteTake(u32)
}

struct AudioThreadState {
	devices: Vec<AudioDevice>,
	mididevices: Vec<MidiDevice>,
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
	id_counter: u32
}

impl FrontendThreadState {
	fn add_take(&mut self, dev_id: usize) {
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
		self.new_take_channel.push(Message::NewTake(take_node));

		self.takes.push(GuiTake{id, dev_id, unmuted: true});

		self.id_counter += 1;
	}

	fn toggle_take_muted(&mut self, take_id: usize) {
		let old_unmuted = self.takes[take_id].unmuted;
		self.takes[take_id].unmuted = !old_unmuted;
		self.new_take_channel.push(Message::SetMute(self.takes[take_id].id, old_unmuted));
	}
}

fn create_thread_states(devices: Vec<AudioDevice>, mididevices: Vec<MidiDevice>, song_length: u32) -> (AudioThreadState, FrontendThreadState) {

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
		id_counter: 0
	};

	let audio_thread_state = AudioThreadState {
		devices,
		mididevices,
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

struct RemoveUnorderedIter<'a,T,F: FnMut(&mut T) -> bool> {
	vec: &'a mut Vec<T>,
	i: usize,
	filter: F
}

impl<T,F: FnMut(&mut T) -> bool> Iterator for RemoveUnorderedIter<'_,T,F> {
	type Item = T;

	fn next(&mut self) -> Option<T> {
		while self.i < self.vec.len() {
			if (self.filter)(&mut self.vec[self.i]) {
				let item = self.vec.swap_remove(self.i);
				return Some(item);
			}
			else {
				self.i += 1;
			}
		}
		return None;
	}
}

fn remove_unordered_iter<T,F: FnMut(&mut T) -> bool>(vec: &mut Vec<T>, filter: F) -> RemoveUnorderedIter<T,F> {
	RemoveUnorderedIter {
		vec,
		i: 0,
		filter
	}
}

impl AudioThreadState {
	fn process_callback(&mut self, client: &jack::Client, scope: &jack::ProcessScope) -> jack::Control {
		//println!("process from thread #{:?}", std::thread::current().id());
		use RecordState::*;
		assert_no_alloc(||{


		assert!(scope.n_frames() < self.song_length);

		use std::io::Write;
		std::io::stdout().flush().unwrap();

		// first, handle the take channel
		loop {
			match self.new_take_channel.pop() {
				Some(msg) => {
					match msg {
						Message::NewTake(take) => { println!("\ngot take"); self.takes.push_back(take); }
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
						_ => { unimplemented!() }
					}
				}
				None => { break; }
			}
		}

		// then, handle all playing takes
		for dev in self.devices.iter_mut() {
			play_silence(client,scope,dev,0..scope.n_frames() as usize);
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
				t.playback(client,scope,dev, 0..scope.n_frames() as usize);
				if song_wraps { println!("\n10/10 would rewind\n"); }
			}
			else if t.record_state == Recording {
				if song_wraps {
					t.playing = true;
					println!("\nAlmost finished recording on device {}, thus starting playback now", t.dev_id);
					println!("Recording started at {}, now is {}", t.started_recording_at, self.transport_position + song_wraps_at as u32);
					t.rewind();
					t.playback(client,scope,dev, song_wraps_at..scope.n_frames() as usize);
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
				t.record(client,scope,dev, 0..song_wraps_at as usize);

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
					t.record(client,scope, dev, song_wraps_at as usize ..scope.n_frames() as usize);
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
fn id2letter(i: u32) -> char {
	return LETTERS[i as usize];
}

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
									if num <= letter2id('l') {
										frontend_thread_state.toggle_take_muted(num as usize);
									}
									else {
										frontend_thread_state.add_take(self.dev_id);
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
	let devices = vec![audiodev, audiodev2];
	let mididevs = vec![mididev];
	
	let (mut audio_thread_state, mut frontend_thread_state) = create_thread_states(devices, mididevs, 48000*3);

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

	
	std::thread::sleep(std::time::Duration::from_millis(1000));
	println!("adding take");
	frontend_thread_state.add_take(0);
	std::thread::sleep(std::time::Duration::from_millis(10000));
	println!("adding take");
	frontend_thread_state.add_take(0);


}
