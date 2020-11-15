use serde::Serialize;

#[derive(Serialize,Clone)]
pub struct Song {
	pub song_position: f32,
	pub transport_position: f32,
	pub playing: bool,
	pub loop_length: f32
}

#[derive(Serialize,Clone)]
pub struct Synth {
	pub id: u32,
	pub name: String,
	pub chains: Vec<Chain>,

	#[serde(skip)]
	pub engine_mididevice_id: usize
}

#[derive(Serialize,Clone)]
pub struct Chain {
	pub id: u32,
	pub name: String,
	pub takes: Vec<Take>,
	pub midi: bool,

	#[serde(skip)]
	pub engine_audiodevice_id: usize
}

#[derive(Serialize,Clone,PartialEq)]
pub enum RecordingState {
	Waiting,
	Recording,
	Finished
}

impl From<crate::engine::RecordState> for RecordingState {
	fn from(record_state: crate::engine::RecordState) -> Self {
		use crate::engine;
		match record_state {
			engine::RecordState::Waiting => RecordingState::Waiting,
			engine::RecordState::Recording => RecordingState::Recording,
			engine::RecordState::Finished => RecordingState::Finished,
		}
	}
}

#[derive(Clone, PartialEq)]
pub enum EngineTakeRef {
	Audio(u32),
	Midi(u32)
}

impl Serialize for EngineTakeRef {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
		where S: serde::Serializer,
		{
			match *self {
				EngineTakeRef::Audio(_) => serializer.serialize_unit_variant("EngineTakeRef", 0, "Audio"),
				EngineTakeRef::Midi(_) => serializer.serialize_unit_variant("EngineTakeRef", 1, "Midi"),
			}
		}
}

#[derive(Serialize,Clone)]
pub struct Take {
	pub id: u32,
	pub name: String,
	#[serde(rename="type")]
	pub engine_take_id: EngineTakeRef,
	pub state: RecordingState,
	pub muted: bool,
	pub muted_scheduled: bool,
	pub associated_midi_takes: Vec<u32>,
}

impl Take {
	pub fn is_midi(&self) -> bool {
		if let EngineTakeRef::Midi(_) = self.engine_take_id {
			return true;
		}
		else {
			return false;
		}
	}

	pub fn is_audio(&self) -> bool { return !self.is_midi(); }

	pub fn is_audible(&self) -> bool {
		return self.state == RecordingState::Finished && self.muted == false;
	}
}


