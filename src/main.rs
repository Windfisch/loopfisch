use jack;
use std::cmp::{min,max};

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
								println!("created new fragment");
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

		/// Rewind the iterator state to the beginning of the stored data.
		pub fn rewind(&mut self) {
			if self.fragments.front().get().unwrap().buf.borrow().len() > 0 {
				self.iter_cursor = self.fragments.front().get().unwrap(); // fragments is never empty
				self.iter_index = 0;
			}
			else {
				self.iter_cursor = std::ptr::null();
			}
		}

		/// Execute func() on the next item and return its result, if there is a next item.
		/// If there is none, return None.
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
						println!("got new fragment");
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
				println!("requesting new fragment");
			}

			Ok(())
		}
	}
}

use outsourced_allocation_buffer::Buffer;

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

struct Take {
	samples: Vec<Buffer<f32>>,
	record_state: RecordState,
	dev_id: usize,
	unmuted: bool,
	playing: bool
}

impl Take {
	pub fn playback(&mut self, client: &jack::Client, scope: &jack::ProcessScope, device: &mut AudioDevice, range: std::ops::Range<usize>) {
		for (channel_buffer, channel_ports) in self.samples.iter_mut().zip(device.channels.iter_mut()) {
			let buffer = &mut channel_ports.out_port.as_mut_slice(scope)[range.clone()];
			for d in buffer {
				let mut val = channel_buffer.next(|v|*v);
				if val.is_none() {
					channel_buffer.rewind();
					val = channel_buffer.next(|v|*v);
				}
				if let Some(v) = val {
					*d = v;
				}
				else {
					panic!();
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

intrusive_adapter!(TakeAdapter = Box<TakeNode>: TakeNode { link: LinkedListLink });

struct AudioThreadState {
	devices: Vec<AudioDevice>,
	takes: LinkedList<TakeAdapter>,
	new_take_channel: ringbuf::Consumer<Box<TakeNode>>,
	song_position: u32,
	song_length: u32,
}

struct FrontendThreadState {
	new_take_channel: ringbuf::Producer<Box<TakeNode>>,
	device_info: Vec<AudioDeviceInfo>
}

impl FrontendThreadState {
	fn add_take(&mut self, dev_id: usize) {
		let n_channels = self.device_info[dev_id].n_channels;
		let take = Take {
			samples: (0..n_channels).map(|_| Buffer::new(1024*8,512*8)).collect(),
			record_state: RecordState::Waiting,
			dev_id,
			unmuted: true,
			playing: false
		};
		let take_node = Box::new(TakeNode::new(take));
		self.new_take_channel.push(take_node);
	}
}

fn create_thread_states(devices: Vec<AudioDevice>, song_length: u32) -> (AudioThreadState, FrontendThreadState) {
	let (take_sender, take_receiver) = ringbuf::RingBuffer::<Box<TakeNode>>::new(10).split();
	

	let frontend_thread_state = FrontendThreadState {
		new_take_channel: take_sender,
		device_info: devices.iter().map(|d| d.info()).collect()
	};

	let audio_thread_state = AudioThreadState {
		devices,
		takes: LinkedList::<TakeAdapter>::new(TakeAdapter::new()),
		new_take_channel: take_receiver,
		song_position: 0,
		song_length
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
		use RecordState::*;
		assert_no_alloc(||{


		assert!(scope.n_frames() < self.song_length);

		let song_position = self.song_position;
		let song_position_after = song_position + scope.n_frames();
		let song_wraps = self.song_length <= song_position_after;
		let song_wraps_at = min(self.song_length - song_position, scope.n_frames()) as usize;
		
		print!("\rprocess @ {:5.1}% {:2x} {} -- {}", (song_position as f32 / self.song_length as f32) * 100.0, 256*song_position / self.song_length, song_position, song_position_after);

		use std::io::Write;
		std::io::stdout().flush().unwrap();

		// first, handle the take channel
		loop {
			match self.new_take_channel.pop() {
				Some(take) => { println!("\ngot take"); self.takes.push_back(take); }
				None => { break; }
			}
		}
		
		// then, handle all playing takes
		let mut cursor = self.takes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			
			if t.playing {
				let dev_id = t.dev_id;
				t.playback(client,scope,&mut self.devices[dev_id], 0..scope.n_frames() as usize);
			}
			else if t.record_state == Recording {
				if song_wraps {
					t.playing = true;
					println!("\nAlmost finished recording on device {}, thus starting playback now", t.dev_id);
					t.rewind();
					let dev_id = t.dev_id;
					play_silence(client,scope,&mut self.devices[dev_id], 0..song_wraps_at);
					t.playback(client,scope,&mut self.devices[dev_id], song_wraps_at..scope.n_frames() as usize);
				}
			}
			else {
				let dev_id = t.dev_id;
				play_silence(client,scope,&mut self.devices[dev_id], 0..scope.n_frames() as usize);
			}

			cursor.move_next();
		}


		// then, handle all armed takes and record into them
		let mut cursor = self.takes.front();
		while let Some(node) = cursor.get() {
			let mut t = node.take.borrow_mut();
			
			if t.record_state == Recording {
				let dev_id = t.dev_id;
				t.record(client,scope, &self.devices[dev_id], 0..song_wraps_at);

				if song_wraps {
					println!("\nFinished recording on device {}", t.dev_id);
					t.record_state = Finished;
				}
			}
			else if t.record_state == Waiting {
				if song_wraps {
					println!("\nStarted recording on device {}", t.dev_id);
					t.record_state = Recording;
					let dev_id = t.dev_id;
					t.record(client,scope, &self.devices[dev_id], song_wraps_at..scope.n_frames() as usize);
				}
			}

			cursor.move_next();
		}

		self.song_position = (self.song_position + scope.n_frames()) % self.song_length;
		});

		jack::Control::Continue
	}
}

struct Notifications;
impl jack::NotificationHandler for Notifications {
	fn thread_init(&self, _: &jack::Client) {
		println!("JACK: thread init");
	}

	fn shutdown(&mut self, status: jack::ClientStatus, reason: &str) {
		println!(
				"JACK: shutdown with status {:?} because \"{}\"",
				status, reason
				);
	}
}

fn main() {
    println!("Hello, world!");

	let (client, _status) = jack::Client::new("loopfisch", jack::ClientOptions::NO_START_SERVER).unwrap();

	println!("JACK running with sampling rate {} Hz, buffer size = {} samples", client.sample_rate(), client.buffer_size());

	let audiodev = AudioDevice::new(&client, 2, "fnord").unwrap();
	let devices = vec![audiodev];
	
	let (mut audio_thread_state, mut frontend_thread_state) = create_thread_states(devices, 48000*3);

	let process_callback = move |client: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
		audio_thread_state.process_callback(client, ps)
	};
	let process = jack::ClosureProcessHandler::new(process_callback);
	let active_client = client.activate_async(Notifications, process).unwrap();

	active_client.as_client().connect_ports_by_name("loopfisch:fnord_out1", "system:playback_1").unwrap();
	active_client.as_client().connect_ports_by_name("loopfisch:fnord_out2", "system:playback_2").unwrap();
	active_client.as_client().connect_ports_by_name("system:capture_1", "loopfisch:fnord_in1").unwrap();
	active_client.as_client().connect_ports_by_name("system:capture_2", "loopfisch:fnord_in2").unwrap();
	std::thread::sleep_ms(1000);


	println!("adding take");
	frontend_thread_state.add_take(0);

	loop {}
}
