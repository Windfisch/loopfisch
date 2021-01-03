use crate::midi_message::*;
use super::driver_traits::MidiDeviceTrait;

#[derive(Clone)]
pub struct MidiNoteRegistry {
	playing_notes: [[u8; 128]; 16]
}

impl std::fmt::Debug for MidiNoteRegistry {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		let notes: Vec<_> = self.active_notes().collect();
		notes.fmt(f)
	}
}


impl MidiNoteRegistry {
	pub fn new() -> MidiNoteRegistry {
		MidiNoteRegistry { playing_notes: [[0u8;128]; 16] }
	}

	pub fn clear(&mut self) { // FIXME this is quite expensive
		*self = MidiNoteRegistry::new();
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

	pub fn send_noteons(&mut self, device: &mut impl MidiDeviceTrait) {
		// FIXME: queue_event could fail; better allow for a "second chance"
		for channel in 0..16 {
			for note in 0..128 {
				let velocity = self.playing_notes[channel as usize][note as usize];
				if velocity != 0 {
					device.queue_event( MidiMessage {
						timestamp: 0,
						data: [0x90 | channel, note, velocity],
						datalen: 3
					}).unwrap();
				}
			}
		}
	}
	pub fn send_noteoffs(&mut self, device: &mut impl MidiDeviceTrait) {
		self.send_noteoffs_at(device, 0);
	}
	pub fn send_noteoffs_at(&mut self, device: &mut impl MidiDeviceTrait, timestamp: u32) {
		// FIXME: queue_event could fail; better allow for a "second chance"
		for channel in 0..16 {
			for note in 0..128 {
				let velocity = self.playing_notes[channel as usize][note as usize];
				if velocity != 0 {
					device.queue_event( MidiMessage {
						timestamp,
						data: [0x80 | channel, note, 64],
						datalen: 3
					}).unwrap();
				}
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use super::super::dummy_driver::*;
	use super::super::testutils::rand_iter;

	#[test]
	pub fn new_registry_has_no_notes() {
		let mut reg = MidiNoteRegistry::new();
		let mut dev = DummyMidiDevice::new(0, 0);
		assert!(reg.active_notes().count() == 0);

		reg.send_noteons(&mut dev);
		assert!(dev.committed.len() == 0);

		dev = DummyMidiDevice::new(0, 0);
		reg.send_noteoffs(&mut dev);
		assert!(dev.committed.len() == 0);
	}

	macro_rules! hashmap(
		{ $($key:expr => $value:expr),+ } => {{
			let mut m = std::collections::HashMap::new();
			$(
				m.insert($key, $value);
			 )+
			m
		}};
	);

	/** send_noteons and send_noteoffs always sends those notes in active_notes,
	  * so we check them in one function */
	fn check(reg: &mut MidiNoteRegistry, expected_notes: std::collections::HashMap<(u8, u8), u8>) {
		let active: Vec<_> = reg.active_notes().collect();

		let mut scope = DummyScope::new();
		scope.next(1024);

		let mut note_ons = DummyMidiDevice::new(0, 0);
		reg.send_noteons(&mut note_ons);
		note_ons.commit_out_buffer(&scope);

		let mut note_offs = DummyMidiDevice::new(0, 0);
		reg.send_noteoffs(&mut note_offs);
		note_offs.commit_out_buffer(&scope);

		let mut note_offs_timestamped = DummyMidiDevice::new(0, 0);
		reg.send_noteoffs_at(&mut note_offs_timestamped, 42);
		note_offs_timestamped.commit_out_buffer(&scope);

		println!("{:?}", expected_notes);
		println!("{:?}", active);

		assert!(expected_notes.len() == active.len());
		assert!(expected_notes.len() == note_ons.committed.len());
		assert!(expected_notes.len() == note_offs.committed.len());
		assert!(expected_notes.len() == note_offs_timestamped.committed.len());

		for note in active {
			assert!(note[0] & 0xF0 == 0x90); // is a note on
			assert!(*expected_notes.get(&(note[0] & 0x0F, note[1])).unwrap() == note[2]);
		}
		for note in note_ons.committed {
			assert!(note.data[0] & 0xF0 == 0x90); // is a note on
			assert!(*expected_notes.get(&(note.data[0] & 0x0F, note.data[1])).unwrap() == note.data[2]);
			assert!(note.datalen == 3);
			assert!(note.timestamp == 0);
		}
		for note in note_offs.committed {
			assert!(note.data[0] & 0xF0 == 0x80); // is a note off
			assert!(expected_notes.get(&(note.data[0] & 0x0F, note.data[1])).is_some());
			assert!(note.datalen == 3);
			assert!(note.timestamp == 0);
		}
		for note in note_offs_timestamped.committed {
			assert!(note.data[0] & 0xF0 == 0x80); // is a note off
			assert!(expected_notes.get(&(note.data[0] & 0x0F, note.data[1])).is_some());
			assert!(note.datalen == 3);
			assert!(note.timestamp == 42);
		}
	}

	#[test]
	pub fn cleared_registry_has_no_notes() {
		let mut reg = MidiNoteRegistry::new();

		reg.register_event([0x90, 0x34, 0x7f]);
		reg.register_event([0x93, 0x42, 0x23]);
		reg.register_event([0x80, 0x34, 0x7f]);
		reg.register_event([0x9a, 0x6b, 0x13]);

		reg.clear();

		check(&mut reg, std::collections::HashMap::new());
	}

	#[test]
	pub fn note_on_is_registered() {
		let mut reg = MidiNoteRegistry::new();

		reg.register_event([0x93, 0x34, 0x7f]);

		check(&mut reg, hashmap!{ (3, 0x34) => 0x7f });
	}

	#[test]
	pub fn note_off_is_registered() {
		let mut reg = MidiNoteRegistry::new();

		reg.register_event([0x93, 0x34, 0x7f]);
		reg.register_event([0x83, 0x34, 0x44]);

		check(&mut reg, std::collections::HashMap::new());
	}

	#[test]
	pub fn note_on_with_zero_velocity_is_handled_like_note_off() {
		let mut reg = MidiNoteRegistry::new();

		reg.register_event([0x93, 0x34, 0x7f]);
		reg.register_event([0x93, 0x34, 0]);

		check(&mut reg, std::collections::HashMap::new());
	}

	fn test_notes(seed: u32) -> impl Iterator<Item=((u8,u8),bool)> {
		rand_iter(0xdeadbeef ^ seed).map(|x|(x%16) as u8)
			.zip(rand_iter(0xbaadf00d ^ seed).map(|x|(x%128) as u8))
			.zip(rand_iter(0x13374722 ^ seed).map(|x|x%2 == 0))
	}

	#[test]
	pub fn handles_up_to_64_notes() {
		let mut reg = MidiNoteRegistry::new();
	
		// FIXME: for now, this depends a bit on the seed. to do: ensure there are no duplicate notes
		for ((channel, note), _) in test_notes(4).take(64) {
			reg.register_event([0x90 | channel, note, 64]);
		}
		for ((channel, note), _) in test_notes(4).take(64).filter(|x| x.1) {
			reg.register_event([0x80 | channel, note, 64]);
		}
		check(&mut reg, test_notes(4).take(64).filter(|x| !x.1).map(|x| (x.0, 64)).collect());
	}
}
