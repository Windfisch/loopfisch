use super::jack_driver::*;
use super::takes::{AudioTakeNode,MidiTakeNode};


#[derive(Debug)]
pub enum Message {
	SetSongLength(u32, u32),
	UpdateAudioDevice(usize, Option<AudioDevice>),
	UpdateMidiDevice(usize, Option<MidiDevice>),
	NewAudioTake(Box<AudioTakeNode>),
	NewMidiTake(Box<MidiTakeNode>),
	SetAudioMute(u32,bool),
	SetMidiMute(u32,bool),
	FinishAudioTake(u32, u32),
	FinishMidiTake(u32, u32),
	DeleteTake(u32)
}

pub enum DestructionRequest {
	AudioDevice(AudioDevice),
	MidiDevice(MidiDevice),
	End
}

