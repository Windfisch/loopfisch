use std::os::unix::io::AsRawFd;
use std::convert::TryFrom;
use tokio::io::AsyncReadExt;
use tokio_fd::AsyncFd;
use eventfd::EventFD;
use std::sync::Arc;
use ringbuf;

pub struct Producer<T> {
	buffer: ringbuf::Producer<T>,
	eventfd: Arc<EventFD>,
}
pub struct Consumer<T> {
	buffer: ringbuf::Consumer<T>,
	async_eventfd: AsyncFd,
	_eventfd: Arc<EventFD>, // just for the ownership
}

impl<T> Producer<T> {
	pub fn send_or_complain(&mut self, message: T) {
		if self.send(message).is_err() {
			println!("Failed to send message in realtime_safe_queue. Message is lost.");
		}
	}
	pub fn send(&mut self, message: T) -> Result<(),T> {
		self.buffer.push(message)?;
		self.eventfd.write(1).unwrap();
		Ok(())
	}
}

impl<T> Consumer<T> {
	pub async fn receive(&mut self) -> T {
		self.async_eventfd.read_u64().await.unwrap();
		self.buffer.pop().unwrap()
	}
}

pub fn new<T>(capacity: usize) -> (Producer<T>, Consumer<T>) {
	let (ringbuf_producer, ringbuf_consumer) = ringbuf::RingBuffer::new(capacity).split();
	let eventfd = Arc::new(EventFD::new(0, eventfd::EfdFlags::EFD_SEMAPHORE).unwrap());
	let async_eventfd = AsyncFd::try_from(eventfd.as_raw_fd()).unwrap();
	let producer = Producer {
		buffer: ringbuf_producer,
		eventfd: eventfd.clone()
	};
	let consumer = Consumer {
		buffer: ringbuf_consumer,
		async_eventfd,
		_eventfd: eventfd
	};

	return (producer, consumer);
}

