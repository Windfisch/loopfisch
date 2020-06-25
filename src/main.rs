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
				remaining_threshold: capacity_increment/2,
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



struct Take {
	samples: Vec<Vec<f32>>,
	record_state: RecordState,
	dev_id: u32,
	unmuted: bool
}

struct AudioChannel {
	in_port: jack::Port<jack::AudioIn>,
	out_port: jack::Port<jack::AudioOut>,
}

impl AudioChannel {
	fn new(client: &jack::Client, name: &str, num: u32) -> Result<AudioChannel, jack::Error> {
		return Ok( AudioChannel {
			in_port: client.register_port(&format!("{}_in{}", name, num), jack::AudioIn::default())?,
			out_port: client.register_port(&format!("{}_out{}", name, num), jack::AudioOut::default())?
		});
	}
}

struct AudioDevice {
	pub channels: Vec<AudioChannel>
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

struct AudioThreadState {
	devices: Vec<AudioDevice>,
	//takes: Vec<ArmedTake>,
	//new_take_channel: lockfree::channel::spsc::Receiver<ArmedTake>,
	song_position: u32,
	song_length: u32,
}

/*struct FrontendThreadState {
	new_take_channel: lockfree::channel::spsc::Sender<ArmedTake>
}

fn create_thread_states(devices: Vec<AudioDevice>, song_length: u32) -> (AudioThreadState, FrontendThreadState) {
	let (take_sender, take_receiver) = lockfree::channel::spsc::create();
	
	let audio_thread_state = AudioThreadState {
		devices,
		armed_takes: Vec::new(), // TODO should have a capacity
		playable_takes: Vec::new(), // TODO should have a capacity
		new_take_channel: take_receiver,
		song_position: 0,
		song_length
	};

	let frontend_thread_state = FrontendThreadState {
		new_take_channel: take_sender,
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
		use lockfree::channel::spsc::*;
		use RecordState::*;

		assert!(scope.n_frames() < self.song_length);

		let song_position_after = self.song_position + scope.n_frames();
		let song_wraps = self.song_length < song_position_after;
		let song_wraps_at = min(self.song_length - self.song_position, scope.n_frames()) as usize;

		// first, handle the take channel
		loop {
			match self.new_take_channel.recv() {
				Ok(armed_take) => { self.armed_takes.push(armed_take); }
				Err(RecvErr::NoMessage) | Err(RecvErr::NoSender) => { break; }
			}
		}

		// then, handle all armed takes and record into them
		for t in self.armed_takes.iter_mut() {
			if t.state == Recording {
				t.record(client,scope, &self.devices[t.dev_id], 0..song_wraps_at);

				if song_wraps {
					println!("Finished recording on device {}", t.dev_id);
					t.state = Finished;
				}
			}
			else if t.state == Waiting {
				if song_wraps {
					println!("Started recording on device {}", t.dev_id);
					t.state = Recording;
					t.record(client,scope, &self.devices[t.dev_id], song_wraps_at..scope.n_frames() as usize);
				}
			}
		}

		// remove all finished takes from armed_takes
		for take in remove_unordered_iter(&mut self.armed_takes, |t| t.state != Finished) {
			self.playable_takes.push(PlayableTake{
				take: take.take,
				dev_id: take.dev_id,
				active: true
			});
		}

		jack::Control::Continue
	}
}
*/

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
	
	//let (mut audio_thread_state, mut frontend_thread_state) = create_thread_states(devices,42);
	{

	let mut hass : outsourced_allocation_buffer::Buffer<u32> = outsourced_allocation_buffer::Buffer::new(32);

	for i in 0..70 {
		std::thread::sleep_ms(10);
		assert_no_alloc(||{
			hass.push(i);
		});
	}
	
	for i in 0..2 {
		print!("rewinding...");
		hass.rewind();
		let mut i=0;
		while let Some(val) = hass.next(|x|*x) {
			print!(" {}", val);
			assert_eq!(val,i);
			i+=1;
		}
		println!();
	}
}

	let process_callback = move |client: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
		//audio_thread_state.process_callback(client, ps)
		jack::Control::Continue
	};
	let process = jack::ClosureProcessHandler::new(process_callback);
	let active_client = client.activate_async(Notifications, process).unwrap();

	loop {}
}
