use super::shared::SharedThreadState;
use super::data::RecordState;
use super::takes::{MidiTake,MidiTakeNode,AudioTake,AudioTakeNode};
use super::retry_channel::RetryChannelPush;
use super::messages::Message;
use super::driver_traits::*;
use std::sync::Arc;
use std::cell::RefCell;
use std::collections::HashMap;
use crate::id_generator::IdGenerator;
use super::jack_driver::*;
use super::midi_registry::MidiNoteRegistry;
use crate::outsourced_allocation_buffer::Buffer;

const CHUNKSIZE: usize = 8*1024;

pub struct GuiAudioTake {
	pub id: u32,
	pub audiodev_id: usize,
	pub unmuted: bool,
	pub length: Option<u32> // None means "not yet finished"
}

pub struct GuiMidiTake {
	pub id: u32,
	pub mididev_id: usize,
	pub unmuted: bool,
	pub length: Option<u32> // None means "not yet finished"
}

pub struct GuiAudioDevice {
	pub info: AudioDeviceInfo,
	pub takes: HashMap<u32, GuiAudioTake>,
}

impl GuiAudioDevice {
	pub fn info(&self) -> &AudioDeviceInfo { &self.info }
	pub fn takes(&self) -> &HashMap<u32, GuiAudioTake> { &self.takes }
}

pub struct GuiMidiDevice {
	pub info: MidiDeviceInfo,
	pub takes: HashMap<u32, GuiMidiTake>,
}

impl GuiMidiDevice {
	pub fn info(&self) -> &MidiDeviceInfo { &self.info }
	pub fn takes(&self) -> &HashMap<u32, GuiMidiTake> { &self.takes }
}


pub trait IntoJackClient : Drop + Send {
	fn as_client<'a>(&'a self) -> &'a jack::Client;
	fn deactivate(self) -> Result<jack::Client, jack::Error>;
}

impl<N, P> IntoJackClient for jack::AsyncClient<N, P>
where
    N: 'static + Send + Sync + jack::NotificationHandler,
    P: 'static + Send + jack::ProcessHandler
{
	fn as_client<'a>(&'a self) -> &'a jack::Client {
		self.as_client()
	}
	fn deactivate(self) -> Result<jack::Client, jack::Error>{
		self.deactivate().map(|client_and_callbacks_tuple| client_and_callbacks_tuple.0)
	}
}

pub struct FrontendThreadState {
	pub command_channel: RetryChannelPush<Message>,
	pub devices: HashMap<usize, GuiAudioDevice>,
	pub mididevices: HashMap<usize, GuiMidiDevice>,
	pub shared: Arc<SharedThreadState>,
	pub next_id: IdGenerator,
	pub async_client: Box<dyn IntoJackClient>
}

impl FrontendThreadState {
	pub fn sample_rate(&self) -> u32 {
		self.async_client.as_client().sample_rate() as u32
	}

	pub fn loop_length(&self) -> u32 {
		// FIXME we should store this variable on our own...
		self.shared.song_length.load(std::sync::atomic::Ordering::Relaxed)
	}

	pub fn set_loop_length(&mut self, loop_length_samples: u32, n_beats: u32) -> Result<(),()> {
		// FIXME TODO: keep track if takes exist, refuse to set the loop length in that case
		// FIXME TODO: reject song lengths that are smaller than the maximum latency.
		self.command_channel.send_message(Message::SetSongLength(loop_length_samples, n_beats))?;
		Ok(())
	}

	pub fn song_position(&self) -> u32 {
		self.shared.song_position.load(std::sync::atomic::Ordering::Relaxed)
	}

	pub fn transport_position(&self) -> u32 {
		self.shared.transport_position.load(std::sync::atomic::Ordering::Relaxed)
	}

	pub fn devices(&self) -> &HashMap<usize, GuiAudioDevice> { &self.devices}
	pub fn mididevices(&self) -> &HashMap<usize, GuiMidiDevice> { &self.mididevices}

	pub fn add_device(&mut self, name: &str, channels: u32) -> Result<usize,()> {
		if let Some(id) = find_first_free_index(&self.devices, 32) {
			let dev = AudioDevice::new(self.async_client.as_client(), channels, name).map_err(|_|())?;
			let guidev = GuiAudioDevice { info: dev.info(), takes: HashMap::new() };
			self.command_channel.send_message(Message::UpdateAudioDevice(id, Some(dev)))?;
			self.devices.insert(id, guidev);
			Ok(id)
		}
		else {
			Err(())
		}
	}
	pub fn add_mididevice(&mut self, name: &str) -> Result<usize,()> {
		if let Some(id) = find_first_free_index(&self.mididevices, 32) {
			let dev = MidiDevice::new(self.async_client.as_client(), name).map_err(|_|())?;
			let guidev = GuiMidiDevice { info: dev.info(), takes: HashMap::new() };
			self.command_channel.send_message(Message::UpdateMidiDevice(id, Some(dev)))?;
			self.mididevices.insert(id, guidev);
			Ok(id)
		}
		else {
			Err(())
		}
	}

	pub fn restart_midi_transport(&mut self, mididev_id: usize) -> Result<(),()> {
		self.command_channel.send_message(Message::RestartMidiTransport(mididev_id))?;
		Ok(())
	}

	pub fn set_audiodevice_echo(&mut self, audiodev_id: usize, echo: bool) -> Result<(),()> {
		self.command_channel.send_message(Message::SetAudioEcho(audiodev_id, echo))?;
		Ok(())
	}

	pub fn add_audiotake(&mut self, audiodev_id: usize, unmuted: bool) -> Result<u32,()> {
		let id = self.next_id.gen();

		let n_channels = self.devices[&audiodev_id].info.n_channels;
		let take = AudioTake::new(id, audiodev_id, unmuted, n_channels, CHUNKSIZE);
		let take_node = Box::new(AudioTakeNode::new(take));

		self.command_channel.send_message(Message::NewAudioTake(take_node))?;
		self.devices.get_mut(&audiodev_id).unwrap().takes.insert(id, GuiAudioTake{id, audiodev_id, unmuted, length: None});

		Ok(id)
	}

	pub fn add_miditake(&mut self, mididev_id: usize, unmuted: bool) -> Result<u32,()> {
		let id = self.next_id.gen();

		let take = MidiTake::new(id, mididev_id, unmuted);
		let take_node = Box::new(MidiTakeNode::new(take));

		self.command_channel.send_message(Message::NewMidiTake(take_node))?;
		self.mididevices.get_mut(&mididev_id).unwrap().takes.insert(id, GuiMidiTake{id, mididev_id, unmuted, length: None});
		Ok(id)
	}

	pub fn finish_audiotake(&mut self, audiodev_id: usize, take_id: u32, take_length: u32) -> Result<(),()> {
		let take = &mut self.devices.get_mut(&audiodev_id).unwrap().takes.get_mut(&take_id).unwrap(); // TODO propagate error
		if take.length.is_some() {
			return Err(());
		}
		take.length = Some(take_length);
		self.command_channel.send_message(Message::FinishAudioTake(take.id, take_length)).unwrap(); // TODO
		Ok(())
	}

	pub fn finish_miditake(&mut self, mididev_id: usize, take_id: u32, take_length: u32) -> Result<(),()> {
		let take = &mut self.mididevices.get_mut(&mididev_id).unwrap().takes.get_mut(&take_id).unwrap(); // TODO propagate error
		if take.length.is_some() {
			return Err(());
		}
		take.length = Some(take_length);
		self.command_channel.send_message(Message::FinishMidiTake(take.id, take_length)).unwrap(); // TODO
		Ok(())
	}

	pub fn set_audiotake_unmuted(&mut self, audiodev_id: usize, take_id: u32, unmuted: bool) -> Result<(),()> {
		let take = &mut self.devices.get_mut(&audiodev_id).unwrap().takes.get_mut(&take_id).unwrap(); // TODO propagate error
		if take.unmuted == unmuted { return Ok(()); }
		self.command_channel.send_message(Message::SetAudioMute(take.id, unmuted))?;
		take.unmuted = unmuted;
		Ok(())
	}
	pub fn set_miditake_unmuted(&mut self, mididev_id: usize, take_id: u32, unmuted: bool) -> Result<(),()> {
		let take = &mut self.mididevices.get_mut(&mididev_id).unwrap().takes.get_mut(&take_id).unwrap(); // TODO propagate error
		if take.unmuted == unmuted { return Ok(()); }
		self.command_channel.send_message(Message::SetMidiMute(take.id, unmuted))?;
		take.unmuted = unmuted;
		Ok(())
	}
}

fn find_first_free_index<T>(map: &HashMap<usize, T>, max: usize) -> Option<usize> {
	for i in 0..max {
		if map.get(&i).is_none() {
			return Some(i);
		}
	}
	return None;
}

