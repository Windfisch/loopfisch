use jack;

use crate::midi_message::MidiMessage;

pub struct MidiDevice {
	in_port: jack::Port<jack::MidiIn>, // FIXME: these should not be public. there should be
	out_port: jack::Port<jack::MidiOut>, // an abstraction layer around the jack driver.

	out_buffer: smallvec::SmallVec<[(MidiMessage, usize); 128]>,
	registry: super::midi_registry::MidiNoteRegistry, // FIXME this belongs in the engine, not the driver

	name: String
}

impl std::fmt::Debug for MidiDevice {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("MidiDevice")
			.field("name", &self.name)
			.finish()
	}
}

pub trait TimestampedMidiEvent {
	fn time(&self) -> u32;
	fn bytes(&self) -> &[u8];
}

impl TimestampedMidiEvent for jack::RawMidi<'_> {
	fn time(&self) -> u32 { self.time }
	fn bytes(&self) -> &[u8] { self.bytes }
}

impl MidiDevice {
	pub fn incoming_events<'a>(&'a self, scope: &'a jack::ProcessScope) -> impl Iterator<Item=impl TimestampedMidiEvent + 'a> + 'a {
		self.in_port.iter(scope)
	}

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

	pub fn update_registry(&mut self, scope: &jack::ProcessScope) {
		use std::convert::TryInto;
		for event in self.in_port.iter(scope) {
			if event.bytes.len() == 3 {
				let data: [u8;3] = event.bytes.try_into().unwrap();
				self.registry.register_event(data);
			}
		}
	}

	pub fn clone_registry(&self) -> super::midi_registry::MidiNoteRegistry {
		self.registry.clone()
	}
	
	pub fn new(client: &jack::Client, name: &str) -> Result<MidiDevice, jack::Error> {
		let in_port = client.register_port(&format!("{}_in", name), jack::MidiIn::default())?;
		let out_port = client.register_port(&format!("{}_out", name), jack::MidiOut::default())?;
		let dev = MidiDevice {
			in_port,
			out_port,
			out_buffer: smallvec::SmallVec::new(),
			registry: super::midi_registry::MidiNoteRegistry::new(),
			name: name.into()
		};
		Ok(dev)
	}

	pub fn info(&self) -> MidiDeviceInfo {
		MidiDeviceInfo {
			name: self.name.clone()
		}
	}

	pub fn playback_latency(&self) -> u32 {
		self.out_port.get_latency_range(jack::LatencyType::Playback).1
	}

	pub fn capture_latency(&self) -> u32 {
		self.in_port.get_latency_range(jack::LatencyType::Capture).1
	}
}

#[derive(Debug)]
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

#[derive(Debug)]
pub struct AudioDevice {
	channels: Vec<AudioChannel>,
	name: String
}

pub struct AudioDeviceInfo {
	pub n_channels: usize,
	pub name: String
}

pub struct MidiDeviceInfo {
	pub name: String
}

impl AudioDevice {
	pub fn info(&self) -> AudioDeviceInfo {
		return AudioDeviceInfo {
			n_channels: self.channels.len(),
			name: self.name.clone()
		};
	}

	pub fn new(client: &jack::Client, n_channels: u32, name: &str) -> Result<AudioDevice, jack::Error> {
		let dev = AudioDevice {
			channels: (0..n_channels).map(|channel| AudioChannel::new(client, name, channel+1)).collect::<Result<_,_>>()?,
			name: name.into()
		};
		Ok(dev)
	}

	pub fn playback_latency(&self) -> u32 {
		self.channels[0].out_port.get_latency_range(jack::LatencyType::Playback).1
	}

	pub fn capture_latency(&self) -> u32 {
		self.channels[0].in_port.get_latency_range(jack::LatencyType::Capture).1
	}

	pub fn playback_buffers<'a>(&'a mut self, scope: &'a jack::ProcessScope) -> impl Iterator<Item = &'a mut [f32]> + 'a {
		self.channels.iter_mut().map(move |channel| channel.out_port.as_mut_slice(scope))
	}

	pub fn record_buffers<'a>(&'a self, scope: &'a jack::ProcessScope) -> impl Iterator<Item = &'a [f32]> + 'a {
		self.channels.iter().map(move |channel| channel.in_port.as_slice(scope))
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

