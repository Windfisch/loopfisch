use super::takes::{AudioTakeNode,MidiTakeNode};

#[derive(Debug)]
pub enum Message<AudioDevice, MidiDevice> {
	SetSongLength(u32, u32),
	UpdateAudioDevice(usize, Option<AudioDevice>),
	UpdateMidiDevice(usize, Option<MidiDevice>),
	NewAudioTake(Box<AudioTakeNode>),
	NewMidiTake(Box<MidiTakeNode>),
	RestartMidiTransport(usize),
	SetAudioEcho(usize, bool),
	SetAudioMute(u32,bool),
	SetMidiMute(u32,bool),
	FinishAudioTake(u32, u32),
	FinishMidiTake(u32, u32),
	DeleteTake(u32)
}

pub enum DestructionRequest<AudioDevice, MidiDevice> {
	AudioDevice(AudioDevice),
	MidiDevice(MidiDevice),
	End
}

