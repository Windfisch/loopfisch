use super::shared::SharedThreadState;
use super::data::RecordState;
use super::takes::{MidiTake,MidiTakeNode,AudioTake,AudioTakeNode};
use super::retry_channel::*;
use super::messages::Message;
use std::sync::Arc;
use std::cell::RefCell;
use std::collections::HashMap;
use crate::id_generator::IdGenerator;
use crate::jack_driver::*;
use crate::midi_registry::MidiNoteRegistry;
use crate::outsourced_allocation_buffer::Buffer;

pub struct GuiAudioTake {
	pub id: u32,
	pub audiodev_id: usize,
	pub unmuted: bool
}

pub struct GuiMidiTake {
	pub id: u32,
	pub mididev_id: usize,
	pub unmuted: bool
}

pub struct GuiAudioDevice {
	pub info: AudioDeviceInfo,
	pub takes: Vec<GuiAudioTake>,
}

impl GuiAudioDevice {
	pub fn info(&self) -> &AudioDeviceInfo { &self.info }
	pub fn takes(&self) -> &Vec<GuiAudioTake> { &self.takes }
}

pub struct GuiMidiDevice {
	pub info: MidiDeviceInfo,
	pub takes: Vec<GuiMidiTake>,
}

impl GuiMidiDevice {
	pub fn info(&self) -> &MidiDeviceInfo { &self.info }
	pub fn takes(&self) -> &Vec<GuiMidiTake> { &self.takes }
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
	pub fn devices(&self) -> &HashMap<usize, GuiAudioDevice> { &self.devices}
	pub fn mididevices(&self) -> &HashMap<usize, GuiMidiDevice> { &self.mididevices}

	pub fn add_device(&mut self, name: &str, channels: u32) -> Result<usize,()> {
		if let Some(id) = find_first_free_index(&self.devices, 32) {
			let dev = AudioDevice::new(self.async_client.as_client(), channels, name).map_err(|_|())?;
			let guidev = GuiAudioDevice { info: dev.info(), takes: Vec::new() };
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
			let guidev = GuiMidiDevice { info: dev.info(), takes: Vec::new() };
			self.command_channel.send_message(Message::UpdateMidiDevice(id, Some(dev)))?;
			self.mididevices.insert(id, guidev);
			Ok(id)
		}
		else {
			Err(())
		}
	}

	pub fn add_audiotake(&mut self, audiodev_id: usize, unmuted: bool) -> Result<u32,()> {
		let id = self.next_id.gen();

		let n_channels = self.devices[&audiodev_id].info.n_channels;
		let take = AudioTake {
			samples: (0..n_channels).map(|_| Buffer::new(1024*8,512*8)).collect(),
			record_state: RecordState::Waiting,
			id,
			audiodev_id,
			unmuted,
			playing: false,
			started_recording_at: 0
		};
		let take_node = Box::new(AudioTakeNode::new(take));

		self.command_channel.send_message(Message::NewAudioTake(take_node))?;
		self.devices.get_mut(&audiodev_id).unwrap().takes.push(GuiAudioTake{id, audiodev_id, unmuted});
		Ok(id)
	}

	pub fn add_miditake(&mut self, mididev_id: usize, unmuted: bool) -> Result<u32,()> {
		let id = self.next_id.gen();

		let take = MidiTake {
			events: Buffer::new(1024, 512),
			record_state: RecordState::Waiting,
			id,
			mididev_id,
			unmuted,
			unmuted_old: unmuted,
			playing: false,
			started_recording_at: 0,
			current_position: 0,
			duration: 0,
			note_registry: RefCell::new(MidiNoteRegistry::new())
		};
		let take_node = Box::new(MidiTakeNode::new(take));

		self.command_channel.send_message(Message::NewMidiTake(take_node))?;
		self.mididevices.get_mut(&mididev_id).unwrap().takes.push(GuiMidiTake{id, mididev_id, unmuted});
		Ok(id)
	}

	pub fn toggle_audiotake_muted(&mut self, audiodev_id: usize, take_id: usize) -> Result<(),()> {
		let take = &mut self.devices.get_mut(&audiodev_id).unwrap().takes[take_id];
		let old_unmuted = take.unmuted;
		self.command_channel.send_message(Message::SetAudioMute(take.id, old_unmuted))?;
		take.unmuted = !old_unmuted;
		Ok(())
	}
	pub fn toggle_miditake_muted(&mut self, audiodev_id: usize, take_id: usize) -> Result<(),()> {
		let take = &mut self.mididevices.get_mut(&audiodev_id).unwrap().takes[take_id];
		let old_unmuted = take.unmuted;
		self.command_channel.send_message(Message::SetMidiMute(take.id, old_unmuted))?;
		take.unmuted = !old_unmuted;
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

