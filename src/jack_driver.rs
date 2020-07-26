use jack;

use crate::midi_message::MidiMessage;

pub struct MidiDevice {
	pub in_port: jack::Port<jack::MidiIn>, // FIXME: these should not be public. there should be
	pub out_port: jack::Port<jack::MidiOut>, // an abstraction layer around the jack driver.

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

pub struct AudioChannel {
	pub in_port: jack::Port<jack::AudioIn>, // FIXME: these shouldn't be pub; there should be
	pub out_port: jack::Port<jack::AudioOut>, // an abstraction layer around the driver
}

impl AudioChannel {
	fn new(client: &jack::Client, name: &str, num: u32) -> Result<AudioChannel, jack::Error> {
		let in_port = client.register_port(&format!("{}_in{}", name, num), jack::AudioIn::default())?;
		let out_port = client.register_port(&format!("{}_out{}", name, num), jack::AudioOut::default())?;
		return Ok( AudioChannel { in_port, out_port });
	}
}

pub struct AudioDevice {
	pub channels: Vec<AudioChannel>
}

pub struct AudioDeviceInfo {
	pub n_channels: usize
}

impl AudioDevice {
	pub fn info(&self) -> AudioDeviceInfo {
		return AudioDeviceInfo {
			n_channels: self.channels.len()
		};
	}

	pub fn new(client: &jack::Client, n_channels: u32, name: &str) -> Result<AudioDevice, jack::Error> {
		let dev = AudioDevice {
			channels: (0..n_channels).map(|channel| AudioChannel::new(client, name, channel+1)).collect::<Result<_,_>>()?
		};
		Ok(dev)
	}
}

pub struct Notifications;
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

