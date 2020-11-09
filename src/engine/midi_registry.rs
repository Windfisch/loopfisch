use crate::bit_array::BitArray2048;
use crate::midi_message::*;
use super::jack_driver::MidiDevice;

pub struct MidiNoteRegistry {
	playing_notes: BitArray2048
}

impl MidiNoteRegistry {
	pub fn new() -> MidiNoteRegistry {
		MidiNoteRegistry { playing_notes: BitArray2048::new() }
	}

	pub fn register_event(&mut self, data: [u8; 3]) {
		use MidiEvent::*;
		match MidiEvent::parse(&data) {
			NoteOn(channel, note, _) => {
				self.playing_notes.set(note as u32 + 128*channel as u32, true);
			}
			NoteOff(channel, note, _) => {
				self.playing_notes.set(note as u32 + 128*channel as u32, false);
			}
			_ => {}
		}
	}

	pub fn stop_playing(&mut self, device: &mut MidiDevice) {
		// FIXME: queue_event could fail; better allow for a "second chance"
		for channel in 0..16 {
			for note in 0..128 {
				if self.playing_notes.get(note as u32 + 128*channel as u32) {
					device.queue_event( MidiMessage {
						timestamp: 0,
						data: [0x80 | channel, note, 64]
					}).unwrap();
				}
			}
		}
		self.playing_notes = BitArray2048::new(); // clear the array
	}
}
