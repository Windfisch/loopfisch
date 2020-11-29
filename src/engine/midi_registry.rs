use crate::midi_message::*;
use super::jack_driver::MidiDevice;

#[derive(Clone)]
pub struct MidiNoteRegistry {
	playing_notes: [[u8; 128]; 16]
}


impl MidiNoteRegistry {
	pub fn new() -> MidiNoteRegistry {
		MidiNoteRegistry { playing_notes: [[0u8;128]; 16] }
	}

	pub fn register_event(&mut self, data: [u8; 3]) {
		use MidiEvent::*;
		match MidiEvent::parse(&data) {
			NoteOn(channel, note, velocity) => {
				self.playing_notes[channel as usize][note as usize] = velocity;
			}
			NoteOff(channel, note, _) => {
				self.playing_notes[channel as usize][note as usize] = 0;
			}
			_ => {}
		}
	}

	pub fn active_notes<'a>(&'a self) -> impl Iterator<Item=[u8; 3]> + 'a {
		gen_iter::gen_iter!(move {
			for channel in 0..16 {
				for note in 0..128 {
					let velocity = self.playing_notes[channel as usize][note as usize];
					if velocity != 0 {
						yield [0x90 | channel, note, velocity];
					}
				}
			}
		})
	}

	pub fn send_noteons(&mut self, device: &mut MidiDevice) {
		// FIXME: queue_event could fail; better allow for a "second chance"
		for channel in 0..16 {
			for note in 0..128 {
				let velocity = self.playing_notes[channel as usize][note as usize];
				if velocity != 0 {
					device.queue_event( MidiMessage {
						timestamp: 0,
						data: [0x90 | channel, note, velocity]
					}).unwrap();
				}
			}
		}
	}
	pub fn send_noteoffs(&mut self, device: &mut MidiDevice) {
		// FIXME: queue_event could fail; better allow for a "second chance"
		for channel in 0..16 {
			for note in 0..128 {
				let velocity = self.playing_notes[channel as usize][note as usize];
				if velocity != 0 {
					device.queue_event( MidiMessage {
						timestamp: 0,
						data: [0x80 | channel, note, 64]
					}).unwrap();
				}
			}
		}
	}
}
