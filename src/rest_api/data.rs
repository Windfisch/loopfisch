use serde::Serialize;

#[derive(Serialize,Clone)]
pub struct Song {
	pub song_position: f64,
	pub transport_position: f64,
	pub playing: bool,
	pub loop_length: f64
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
	pub echo: bool,

	#[serde(skip)]
	pub engine_audiodevice_id: usize
}

#[derive(Clone,PartialEq)]
pub enum RecordingState {
	Waiting,
	Recording(u32 /*since when*/),
	Finished
}

impl Serialize for RecordingState {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
		where S: serde::Serializer,
		{
			match *self {
				RecordingState::Waiting => serializer.serialize_unit_variant("RecordingState", 0, "Waiting"),
				RecordingState::Recording(_) => serializer.serialize_unit_variant("RecordingState", 0, "Recording"),
				RecordingState::Finished => serializer.serialize_unit_variant("RecordingState", 0, "Finished"),
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
	pub playing_since: Option<f64>,
	pub duration: Option<f64>,
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


