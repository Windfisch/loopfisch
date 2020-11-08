use crate::jack_driver::*;
use super::takes::{AudioTakeNode,MidiTakeNode};


#[derive(Debug)]
pub enum Message {
	UpdateAudioDevice(usize, Option<AudioDevice>),
	UpdateMidiDevice(usize, Option<MidiDevice>),
	NewAudioTake(Box<AudioTakeNode>),
	NewMidiTake(Box<MidiTakeNode>),
	SetAudioMute(u32,bool),
	SetMidiMute(u32,bool),
	DeleteTake(u32)
}

pub enum DestructionRequest {
	AudioDevice(AudioDevice),
	MidiDevice(MidiDevice),
	End
}

