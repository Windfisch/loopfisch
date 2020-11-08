use ringbuf;

pub struct RetryChannelPush<T: std::fmt::Debug> (pub ringbuf::Producer<T>);

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

