#[derive(Debug)]
pub struct MidiMessage {
	pub timestamp: jack::Frames,
	pub data: [u8; 3]
}

pub enum MidiEvent {
	/// NoteOn(channel, note, velocity)
	NoteOn(u8, u8, u8),
	/// NoteOff(channel, note, velocity)
	NoteOff(u8, u8, u8),
	Unknown
}

impl MidiEvent {
	pub fn parse(data: &[u8; 3]) -> MidiEvent {
		use MidiEvent::*;
		let kind = data[0] & 0xF0;
		let chan = data[0] & 0x0F;
		match kind {
			0x90 => {
				if data[2] > 0 {
					NoteOn(chan, data[1], data[2])
				}
				else { // zero velocity note ons are treated as note offs.
					NoteOff(chan, data[1], 0)
				}
			}
			0x80 => NoteOff(chan, data[1], data[2]),
			_ => Unknown
		}
	}
}

