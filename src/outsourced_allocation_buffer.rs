use intrusive_collections::{intrusive_adapter, LinkedList, LinkedListLink};
use std::cell::UnsafeCell;
use ringbuf::RingBuffer;
use std::thread;

struct BufferFragment<T> {
	link: LinkedListLink,

	/** Accesses to this UnsafeCell value are safe, as long either
	  * a) no reference to contained data is handed out or
	  * b) any reference handed out borrows on `BufferFragment` or
	  * c) any mutable reference handed out borrows mutably
	  *
	  * Reason: `Buffer` is Send, but not Sync, so no concurrent
	  * accesses from multiple threads can happen. The allocator
	  * thread will never access data in the BufferFragments as soon
	  * as they are enqueued in the actual Buffer. Any borrowing rules
	  * violation of a) - c) would require a similar violation on
	  * the `Buffer` object.
	  */
	buf: UnsafeCell<Vec<T>>,
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
		// there is always enough space for the End request.
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
			buf: UnsafeCell::new(Vec::with_capacity(capacity_increment))
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
								buf: UnsafeCell::new(Vec::with_capacity(capacity_increment))
							});
							// there is always enough space for pushing the fragment
							incoming_producer.push(fragment).map_err(|_|()).unwrap();
						}
						ThreadRequest::End => {
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
		unsafe { (*self.fragments.front().get().unwrap().buf.get()).len() == 0 }
	}

	/// Rewind the iterator state to the beginning of the stored data.
	pub fn rewind(&mut self) {
		if !self.empty() {
			// fragments is never empty, hence the unwrap()
			self.iter_cursor = self.fragments.front().get().unwrap(); 
			self.iter_index = 0;
		}
		else {
			self.iter_cursor = std::ptr::null();
		}
	}

	/// Returns a reference to the current item, if one exists, and advances the cursor to the next item.
	/// Returns None if none exists.
	pub fn next<'a>(&mut self) -> Option<&'a T> {
		if self.iter_cursor.is_null() {
			return None;
		}

		// Get a cursor from the pointer. This places a borrow on self.fragments
		// This is safe iif iter_cursor points to an element current in the list.
		// Since list elements are only added, but never removed, and since iter_cursor
		// has already belonged to the list when it was set, this is fine.
		let mut cursor = unsafe{ self.fragments.cursor_from_ptr(self.iter_cursor) };
		let buf = unsafe {&*cursor.get().unwrap().buf.get() };
	
		// Perform the actual access. This is always a valid element because no elements can
		// be deleted.
		let result = &buf[self.iter_index];
	
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
					unsafe { assert!((*frag.buf.get()).len() > 0); }
					frag
				}
				None => {
					std::ptr::null()
				}
			};

		return Some(result);
	}

	pub fn peek<'a>(&'a mut self) -> Option<&'a T> {
		if self.iter_cursor.is_null() {
			return None;
		}

		// Get a cursor from the pointer. This places a borrow on self.fragments
		// This is safe iif iter_cursor points to an element current in the list.
		// Since list elements are only added, but never removed, and since iter_cursor
		// has already belonged to the list when it was set, this is fine.
		let cursor = unsafe{ self.fragments.cursor_from_ptr(self.iter_cursor) };
		let buf = unsafe { &*cursor.get().unwrap().buf.get() };
	
		// Perform the actual access. This is always a valid element because no elements can
		// be deleted.
		return Some(&buf[self.iter_index]);
	}

	/// Tries to push elem into the buffer. Fails if no capacity is available, usually
	/// because the manager thread was too slow in adding new capacity.
	pub fn push(&mut self, elem: T) -> Result<(), T> {
		let remaining = {
			let frag = self.fragments.back_mut();
			let buf = unsafe { &*frag.get().unwrap().buf.get() };
			buf.capacity() - buf.len()
		};

		if remaining < 1 {
			// we can't fit the data into the current fragment, let's check whether
			// a new fragment has been queued already
			match self.incoming_fragment_ringbuf.pop() {
				Some(fragment) => {
					self.fragments.push_back(fragment);
					self.request_pending = false;
				}
				None => {
					return Err(elem);
				}
			}
		}
		
		unsafe {
			(*self.fragments.back_mut().get().unwrap().buf.get()).push(elem);
		}

		if remaining <= self.remaining_threshold && !self.request_pending {
			self.new_fragment_request_ringbuf.push(ThreadRequest::Fragment).map_err(|_|()).unwrap();
			self.request_pending = true;
			self.thread_handle.thread().unpark();
		}

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use assert_no_alloc::assert_no_alloc;

	macro_rules! rt_assert {
		($e:expr) => { assert!( assert_no_alloc(|| $e) ); }
	}

	fn wait() {
		std::thread::sleep(std::time::Duration::from_millis(10));
	}

	#[test]
	pub fn only_empty_buffers_report_empty() {
		let mut buffer = Buffer::<u32>::new(8, 4);
		rt_assert!(buffer.empty());
		buffer.push(42).expect("push failed");
		rt_assert!(!buffer.empty());
		for _ in 1..7 {
			buffer.push(42).expect("push failed");
		}
		wait();
		for _ in 7..12 {
			buffer.push(42).expect("push failed");
		}
		assert!( assert_no_alloc(|| !buffer.empty() ));
		buffer.rewind();
		assert!( assert_no_alloc(|| !buffer.empty() ));
	}

	#[test]
	pub fn next_empty_buffer_returns_none() {
		let mut buffer = Buffer::<u32>::new(8, 4);
		rt_assert!( buffer.next().is_none() );
		rt_assert!( buffer.next().is_none() );
		rt_assert!( buffer.next().is_none() );
	}

	#[test]
	pub fn peek_empty_buffer_returns_none() {
		let mut buffer = Buffer::<u32>::new(8, 4);
		rt_assert!( buffer.peek().is_none() );
		rt_assert!( buffer.peek().is_none() );
		rt_assert!( buffer.peek().is_none() );
	}

	#[test]
	pub fn buffer_must_be_rewound_prior_to_first_read() {
		let mut buffer = Buffer::<u32>::new(8, 4);
		for i in 0..6 {
			rt_assert!( buffer.push(i).is_ok() );
		}

		assert!( assert_no_alloc(|| buffer.next()).is_none() );
	}

	#[test]
	pub fn rewinding_empty_buffer_does_nothing() {
		let mut buffer = Buffer::<u32>::new(8, 4);
		assert_no_alloc(|| buffer.rewind() );
		rt_assert!( buffer.peek().is_none() );
	}

	#[test]
	pub fn rewinding_nonempty_buffer_rewinds() {
		let mut buffer = Buffer::<u32>::new(8, 4);
		for i in 0..6 {
			rt_assert!( buffer.push(i).is_ok() );
		}
		
		assert_no_alloc(|| buffer.rewind());
		assert!( *assert_no_alloc(|| buffer.next()).unwrap() == 0 );
		assert!( *assert_no_alloc(|| buffer.next()).unwrap() == 1 );
		assert!( *assert_no_alloc(|| buffer.next()).unwrap() == 2 );
		assert_no_alloc(|| buffer.rewind());
		assert!( *assert_no_alloc(|| buffer.next()).unwrap() == 0 );
		assert!( *assert_no_alloc(|| buffer.next()).unwrap() == 1 );
		assert!( *assert_no_alloc(|| buffer.next()).unwrap() == 2 );

		wait();
		for i in 6..12 {
			rt_assert!( buffer.push(i).is_ok() );
		}
		assert_no_alloc(|| buffer.rewind());
		for i in 0..10 {
			assert!( *assert_no_alloc(|| buffer.next()).unwrap() == i );
		}
		assert_no_alloc(|| buffer.rewind());
		for i in 0..10 {
			assert!( *assert_no_alloc(|| buffer.next()).unwrap() == i );
		}
	}

	#[test]
	pub fn next_returns_pushed_items() {
		let mut buffer = Buffer::<u32>::new(8, 4);
		rt_assert!( buffer.push(0).is_ok() );
		assert_no_alloc(|| buffer.rewind());

		for i in 1..1024 {
			if i % 8 == 6 {
				wait();
			}
			rt_assert!( buffer.push(i).is_ok() );
			assert!( *assert_no_alloc(|| buffer.next()).unwrap() == i-1 );
		}
	}

	#[test]
	pub fn next_beyond_end_returns_none() {
		let mut buffer = Buffer::<u32>::new(8, 4);
		for i in 0..6 {
			rt_assert!( buffer.push(i).is_ok() );
		}
		wait();
		for i in 6..10 {
			rt_assert!( buffer.push(i).is_ok() );
		}
		
		assert_no_alloc(|| buffer.rewind());
		for i in 0..10 {
			assert!( *assert_no_alloc(|| buffer.next()).unwrap() == i);
		}
		assert!(assert_no_alloc(|| buffer.next()).is_none());
	}

	#[test]
	pub fn peek_does_not_advance() {
		let mut buffer = Buffer::<u32>::new(8, 4);
		for i in 0..3 {
			rt_assert!( buffer.push(i).is_ok() );
		}
		
		assert_no_alloc(|| buffer.rewind());
		for i in 0..3 {
			assert!( *assert_no_alloc(|| buffer.peek()).unwrap() == i);
			assert!( *assert_no_alloc(|| buffer.peek()).unwrap() == i);
			assert!( *assert_no_alloc(|| buffer.next()).unwrap() == i);
		}
		assert!(assert_no_alloc(|| buffer.peek()).is_none());
		assert!(assert_no_alloc(|| buffer.peek()).is_none());
		assert!(assert_no_alloc(|| buffer.next()).is_none());
	}

	#[test]
	pub fn push_fails_gracefully_when_too_fast() {
		let mut buffer = Buffer::<u32>::new(2, 1);
		for _ in 0..100 {
			if buffer.push(42).is_err() {
				return;
			}
		}

		panic!("No error occurred when one should have occurred");
	}
}
