use jack;
use super::driver_traits::*;

use super::backend::AudioThreadState;

use crate::midi_message::MidiMessage;

struct ProcessHandler(AudioThreadState<JackDriver>);

impl jack::ProcessHandler for ProcessHandler {
	fn process(&mut self, _client: &jack::Client, process_scope: &jack::ProcessScope) -> jack::Control {
		self.0.process_callback(process_scope);
		return jack::Control::Continue;
	}
}

enum JackClientState {
	Activated(jack::AsyncClient<Notifications, ProcessHandler>),
	NotActivated(jack::Client),
	OhGodWhyRust
}

impl JackClientState {
	fn as_jack_client(&self) -> &jack::Client {
		match self {
			Self::Activated(async_client) => async_client.as_client(),
			Self::NotActivated(client) => client,
			Self::OhGodWhyRust => panic!("cannot happen")
		}
	}
}

pub struct JackDriver {
	client: JackClientState
}

impl JackDriver {
	pub fn new() -> JackDriver {
		let (client, _status) = jack::Client::new("loopfisch", jack::ClientOptions::NO_START_SERVER).unwrap();
		println!("JACK running with sampling rate {} Hz, buffer size = {} samples", client.sample_rate(), client.buffer_size());
		JackDriver {
			client: JackClientState::NotActivated(client)
		}
	}
}

impl DriverTrait for JackDriver {
	type MidiDev = MidiDevice;
	type AudioDev = AudioDevice;
	type ProcessScope = jack::ProcessScope;
	type Error = jack::Error;

	fn activate(&mut self, audio_thread_state: AudioThreadState<JackDriver>) {
		self.client = JackClientState::Activated (
			match std::mem::replace(&mut self.client, JackClientState::OhGodWhyRust) {
				JackClientState::Activated(_) => panic!("Client is already activated"),
				JackClientState::NotActivated(client) =>
					client.activate_async(Notifications, ProcessHandler(audio_thread_state)).unwrap(),
				JackClientState::OhGodWhyRust => panic!("Cannot happen")
			}
		);
	}

	fn new_audio_device(&mut self, n_channels: u32, name: &str) -> Result<AudioDevice, jack::Error> {
		let client = self.client.as_jack_client();
		Ok(AudioDevice {
			channels: (0..n_channels).map(|channel| AudioChannel::new(client, name, channel+1)).collect::<Result<_,_>>()?,
			name: name.into()
		})
	}

	fn new_midi_device(&mut self, name: &str) -> Result<MidiDevice, jack::Error> {
		let client = self.client.as_jack_client();
		let in_port = client.register_port(&format!("{}_in", name), jack::MidiIn::default())?;
		let out_port = client.register_port(&format!("{}_out", name), jack::MidiOut::default())?;
		Ok(MidiDevice {
			in_port,
			out_port,
			out_buffer: smallvec::SmallVec::new(),
			name: name.into()
		})
	}

	fn sample_rate(&self) -> u32 {
		self.client.as_jack_client().sample_rate() as u32
	}
}

pub struct MidiDevice {
	in_port: jack::Port<jack::MidiIn>,
	out_port: jack::Port<jack::MidiOut>,

	out_buffer: smallvec::SmallVec<[(MidiMessage, usize); 128]>,

	name: String
}

impl std::fmt::Debug for MidiDevice {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("MidiDevice")
			.field("name", &self.name)
			.finish()
	}
}

impl ProcessScopeTrait for jack::ProcessScope {
	fn n_frames(&self) -> u32 { self.n_frames() }
}

impl<'a> TimestampedMidiEvent for jack::RawMidi<'a> {
	fn time(&self) -> u32 { self.time }
	fn bytes(&self) -> &[u8] { self.bytes }
}

impl MidiDeviceTrait for MidiDevice {
	type Event<'a> = jack::RawMidi<'a>;
	type EventIterator<'a> = jack::MidiIter<'a>;
	type Scope = jack::ProcessScope;

	fn incoming_events(&'a self, scope: &'a jack::ProcessScope) -> Self::EventIterator<'a> {
		self.in_port.iter(scope)
	}

	/// sorts the events in the out_buffer, commits them to the out_port and clears the out_buffer.
	/// FIXME: deduping
	fn commit_out_buffer(&mut self, scope: &jack::ProcessScope) {
		// sort
		self.out_buffer.sort_unstable_by( |a,b| a.0.timestamp.cmp(&b.0.timestamp).then(a.1.cmp(&b.1)) );

		// write
		let mut writer = self.out_port.writer(scope);
		for (msg,_idx) in self.out_buffer.iter() {
			// FIXME: do the deduping here
			writer.write(&jack::RawMidi {
				time: msg.timestamp,
				bytes: &msg.data[0..msg.datalen as usize]
			}).unwrap();
		}

		// clear
		self.out_buffer.clear();
	}
	fn queue_event(&mut self, msg: MidiMessage) -> Result<(), ()> {
		if self.out_buffer.len() < self.out_buffer.inline_size() {
			self.out_buffer.push((msg, self.out_buffer.len()));
			Ok(())
		}
		else {
			Err(())
		}
	}

	fn info(&self) -> MidiDeviceInfo {
		MidiDeviceInfo {
			name: self.name.clone()
		}
	}

	fn playback_latency(&self) -> u32 {
		self.out_port.get_latency_range(jack::LatencyType::Playback).1
	}

	fn capture_latency(&self) -> u32 {
		self.in_port.get_latency_range(jack::LatencyType::Capture).1
	}
}

#[derive(Debug)]
struct AudioChannel {
	in_port: jack::Port<jack::AudioIn>,
	out_port: jack::Port<jack::AudioOut>,
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

pub struct CaptureIter<'a>(&'a jack::ProcessScope, std::slice::Iter<'a, AudioChannel>);
impl<'a> Iterator for CaptureIter<'a> {
	type Item = &'a [f32];
	fn next(&mut self) -> Option<Self::Item> {
		self.1.next().map(|channel| channel.in_port.as_slice(self.0))
	}
}

pub struct PlaybackCaptureIter<'a>(&'a jack::ProcessScope, std::slice::IterMut<'a, AudioChannel>);
impl<'a> Iterator for PlaybackCaptureIter<'a> {
	type Item = (&'a mut [f32], &'a [f32]);
	fn next(&mut self) -> Option<Self::Item> {
		self.1.next().map(|channel| (channel.out_port.as_mut_slice(self.0), channel.in_port.as_slice(self.0)) )
	}
}

impl AudioDeviceTrait for AudioDevice {
	type SliceIter<'a> = CaptureIter<'a>;
	type MutSliceIter<'a> = PlaybackCaptureIter<'a>;
	type Scope = jack::ProcessScope;

	fn info(&self) -> AudioDeviceInfo {
		return AudioDeviceInfo {
			n_channels: self.channels.len(),
			name: self.name.clone()
		};
	}

	fn playback_latency(&self) -> u32 {
		self.channels[0].out_port.get_latency_range(jack::LatencyType::Playback).1
	}

	fn capture_latency(&self) -> u32 {
		self.channels[0].in_port.get_latency_range(jack::LatencyType::Capture).1
	}

	fn playback_and_capture_buffers(&'a mut self, scope: &'a jack::ProcessScope) -> Self::MutSliceIter<'a> {
		PlaybackCaptureIter(scope, self.channels.iter_mut())
	}

	fn record_buffers(&'a self, scope: &'a jack::ProcessScope) -> Self::SliceIter<'a> {
		CaptureIter(scope, self.channels.iter())
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

