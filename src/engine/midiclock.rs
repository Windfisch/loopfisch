use super::driver_traits::*;
use crate::midi_message::MidiMessage;

pub struct MidiClock<T: MidiDeviceTrait> {
	device: T
}

impl<T: MidiDeviceTrait> MidiClock<T> {
	pub fn new(device: T) -> MidiClock<T> {
		MidiClock {
			device
		}
	}

	pub fn process(&mut self, position_uncompensated: u32, song_length: u32, n_beats: u32, scope: &T::Scope) {
		let factor = n_beats * 24;

		let n_frames = scope.n_frames();
		let latency = self.device.playback_latency();
		let position_f = factor * ((position_uncompensated + latency) % song_length);
		let song_length_f = factor * song_length;
		let n_frames_f = factor * n_frames;
		
		let song_wraps_at_f = std::cmp::min(song_length_f - position_f, n_frames_f);

		let n_clocks = n_beats * 24;
		let period_per_clock_f = (song_length_f+n_clocks-1) / n_clocks; // round towards +inf

		let mut time_since_last_clock_f = position_f % period_per_clock_f;
		if time_since_last_clock_f == 0 {
			time_since_last_clock_f = period_per_clock_f;
		}

		for timestamp_f in
			((period_per_clock_f - time_since_last_clock_f)..song_wraps_at_f).step_by(period_per_clock_f as usize)
			.chain( (song_wraps_at_f..n_frames_f).step_by(period_per_clock_f as usize) )
		{
			self.device.queue_event(MidiMessage {
				timestamp: timestamp_f / factor,
				data: [0xF8, 0, 0],
				datalen: 1
			});
		}

		self.device.commit_out_buffer(scope);
	}
}

#[cfg(test)]
mod tests {
	use super::super::dummy_driver::*;
	use super::*;


	use std::cmp::{min,max};

	fn spacing(mut foo: impl Iterator<Item=u32>) -> (u32, u32) {
		let mut prev = foo.next().unwrap();

		let mut lo = u32::MAX;
		let mut hi = 0;

		for val in foo {
			let diff = val - prev;
			lo = min(lo, diff);
			hi = max(hi, diff);

			prev = val;
		}
		
		return (lo, hi);
	}

	#[test]
	pub fn midiclock_given_songlength_produces_correct_clockticks() {
		let sample_rate = 44100;
		for bpm in [31, 47, 100, 112,113,114,115,116,117,118,119,120,121, 161, 180, 213].iter() {
			for n_beats in 4..9 {
				let song_length = sample_rate * n_beats *60/bpm;
				
				let mut device = DummyMidiDevice::new(0);
				let mut clock = MidiClock::new( &mut device );
				let mut scope = DummyScope::new();
				scope.next(song_length);
				clock.process(scope.time % song_length, song_length, n_beats, &scope);
				assert!(device.committed.len() as u32 == 24*n_beats);
			}
		}
	}

	#[test]
	pub fn midiclock_given_multiple_of_songlength_produces_correct_clockticks() {
		let sample_rate = 44100;
		for bpm in [31, 47, 100, 112,113,114,115,116,117,118,119,120,121, 161, 180, 213].iter() {
			for n_beats in 4..9 {
				let song_length = sample_rate * n_beats *60/bpm;

				for latency in [0, 1, 32, 51, 256, 4096].iter() {
					let mut device = DummyMidiDevice::new(*latency);
					let mut clock = MidiClock::new( &mut device );
					let mut scope = DummyScope::new();
					scope.next(10*song_length);
					clock.process(scope.time % song_length, song_length, n_beats, &scope);
					assert!(device.committed.len() as u32 == 10*24*n_beats);
				}
			}
		}
	}

	#[test]
	pub fn midiclock_given_songlength_plus_1_produces_one_more_clocktick() {
		let sample_rate = 44100;
		for bpm in [31, 47, 100, 112,113,114,115,116,117,118,119,120,121, 161, 180, 213].iter() {
			for n_beats in 4..9 {
				let song_length = sample_rate * n_beats *60/bpm;
				
				let mut device = DummyMidiDevice::new(0);
				let mut clock = MidiClock::new( &mut device );
				let mut scope = DummyScope::new();
				scope.next(song_length + 1);
				clock.process(scope.time % song_length, song_length, n_beats, &scope);
				assert!(device.committed.len() as u32 == 24*n_beats + 1);
			}
		}
	}

	#[test]
	pub fn midiclock_jitter_is_less_than_1() {
		let sample_rate = 44100;
		let n_beats = 8;
		for bpm in [113, 116, 127].iter() {
			let song_length = sample_rate * n_beats *60/bpm;
			for chunksize in [1, 32, 51, 127, 128, 1024, 4096, 4*song_length].iter() {
				let mut device = DummyMidiDevice::new(128);
				let mut clock = MidiClock::new( &mut device );
				let mut scope = DummyScope::new();
				for _ in 0..(4*song_length/chunksize) {
					scope.next(*chunksize);
					clock.process(scope.time % song_length, song_length, n_beats, &scope);
				}
				let (lo, hi) = spacing(device.committed.iter().map(|x| x.timestamp));
				assert!(hi-lo <= 1);
			}
		}
	}


	#[test]
	pub fn midiclock_results_do_not_depend_on_chunksize() {
		let sample_rate = 44100;
		let n_beats = 8;
		for bpm in [113, 116, 127].iter() {
			let song_length = sample_rate * n_beats *60/bpm;

			let reference = {
				let mut device = DummyMidiDevice::new(128);
				let mut clock = MidiClock::new( &mut device );
				let mut scope = DummyScope::new();
				scope.next(4*song_length);
				clock.process(scope.time % song_length, song_length, n_beats, &scope);
				device.committed
			};

			for chunksize in [1, 32, 51, 127, 128, 1024, 4096].iter() {
				let mut device = DummyMidiDevice::new(128);
				let mut clock = MidiClock::new( &mut device );
				let mut scope = DummyScope::new();
				for _ in 0..(4*song_length/chunksize) {
					scope.next(*chunksize);
					clock.process(scope.time % song_length, song_length, n_beats, &scope);
				}
				assert!(
					device.committed.iter().zip( reference.iter() )
						.all(|pair| pair.0 == pair.1)
				);
			}
		}
	}

	#[test]
	pub fn midiclock_latency_handled_correctly() {
		let sample_rate = 44100;
		for bpm in [31, 47, 100, 112,113,114,115,116,117,118,119,120,121, 161, 180, 213].iter() {
			for n_beats in 4..9 {
				let song_length = sample_rate * n_beats *60/bpm;

				for latency in [0, 1, 32, 51, 256, 4096].iter() {
					let mut device = DummyMidiDevice::new(*latency);
					let mut clock = MidiClock::new( &mut device );
					let mut scope = DummyScope::new();
					scope.next(2*song_length);
					clock.process(scope.time % song_length, song_length, n_beats, &scope);
					assert!(device.committed.iter().filter(|x| x.timestamp == song_length-latency).count() == 1);
				}
			}
		}
	}
}
