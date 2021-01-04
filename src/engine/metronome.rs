use std::cmp::min;
use super::driver_traits::*;

pub struct AudioMetronome<T: AudioDeviceTrait> {
	device: T,
	volume: f32,
	unmuted: bool
}

fn ceil_div(a: u32, b: u32) -> u32 { (a+b-1)/b }

impl<T: AudioDeviceTrait> AudioMetronome<T> {
	pub fn new(device: T) -> AudioMetronome<T> {
		AudioMetronome {
			device,
			volume: 0.3,
			unmuted: true
		}
	}

	pub fn process(&mut self, position: u32, song_length: u32, beats: u32, sample_rate: u32, scope: &T::Scope) {
		if !self.unmuted { return; }
		let period = ceil_div(song_length, beats);
		let latency = self.device.playback_latency();
		for buffers in self.device.playback_and_capture_buffers(scope) {
			for i in 0..scope.n_frames() {
				buffers.0[i as usize] = self.volume * Self::process_one((position + i + latency) % song_length, period, beats, sample_rate);
			}
		}
	}

	fn process_one(position: u32, period: u32, beats: u32, sample_rate: u32) -> f32 {
		let position_in_beat = position % period;
		let beat = position / period;

		let click_length = sample_rate / 10;

		let volume = 1.0 - min(position_in_beat, click_length) as f32 / click_length as f32;
		let freq = if beat == 0 { 880 } else { 440 };

		let sawtooth: f32 = (position_in_beat as f32 / sample_rate as f32 * freq as f32).fract();
		let square = if sawtooth < 0.5 {-1.0} else {1.0};

		return square * volume;
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use super::super::dummy_driver::*;
	use super::super::testutils;

	const sample_rate : u32 = 44100;

	#[test]
	pub fn zero_dc_offset() {
		let song_length = sample_rate * 4;
		let device = DummyAudioDevice::new(1, 0, 0);
		let mut metronome = AudioMetronome::new(device);
		let mut scope = DummyScope::new();
		scope.run_for(song_length, 1024, |scope| metronome.process(scope.time, song_length, 8, sample_rate, scope));
		let buffer = &metronome.device.playback_buffers[0][0..1000];
		let mean = buffer.iter().sum::<f32>() / (buffer.len() as f32);
		let max = buffer.iter().map(|x|x.abs()).fold(0.0, |a,b| f32::max(a,b));
		assert!( (mean / max).abs() < 0.01 );
	}

	#[test]
	pub fn correct_amount_of_ticks() {
		for bpm in [85, 116, 120, 121, 213].iter() {
			for n_beats in 4..=8 {
				let song_length = sample_rate * n_beats *60/bpm;

				let device = DummyAudioDevice::new(1, 0, 0);
				let mut metronome = AudioMetronome::new(device);
				let mut scope = DummyScope::new();
				scope.run_for(4*song_length, 1024, |scope| metronome.process(scope.time, song_length, n_beats, sample_rate, scope));
				let n_ticks = testutils::ticks(&metronome.device.playback_buffers[0], 0.2).len();
				assert!(n_ticks as u32 == 4*n_beats);
			}
		}
	}

	#[test]
	pub fn all_channels_have_same_data() {
		let channels = 3;
		let bpm = 161;
		let song_length = sample_rate * 4 *60/bpm;

		let device = DummyAudioDevice::new(channels, 128, 0);
		let mut metronome = AudioMetronome::new(device);
		let mut scope = DummyScope::new();
		scope.run_for(4*song_length, 1024, |scope| metronome.process(scope.time, song_length, 8, sample_rate, scope));

		for i in 1..channels {
			assert!( metronome.device.playback_buffers[0] == metronome.device.playback_buffers[i] );
		}
	}

	#[test]
	pub fn latency_compensation_works_correctly() {
		let n_beats = 8;
		let bpm = 117;
		let song_length = sample_rate * n_beats *60/bpm;

		for latency in [0, 1024, 9001].iter() {
			let device = DummyAudioDevice::new(1, *latency, 0);
			let mut metronome = AudioMetronome::new(device);
			let mut scope = DummyScope::new();
			scope.run_for(4*song_length, 1024, |scope| metronome.process(scope.time, song_length, 8, sample_rate, scope));


			let beats = testutils::ticks(&metronome.device.playback_buffers[0], 0.25);
			let found = beats.into_iter().find(|x| *x as u32 == song_length - latency).is_some();
			assert!(found);
		}
	}

	#[test]
	pub fn jitter_is_low_enough() {
		let n_beats = 8;
		for bpm in [113, 116, 127].iter() {
			let song_length = sample_rate * n_beats *60/bpm;

			let device = DummyAudioDevice::new(1, 0, 0);
			let mut metronome = AudioMetronome::new(device);
			let mut scope = DummyScope::new();
			scope.run_for(4*song_length, 1024, |scope| metronome.process(scope.time, song_length, 8, sample_rate, scope));

			let (lo, hi) = testutils::spacing( testutils::ticks(&metronome.device.playback_buffers[0], 0.25).into_iter() );
			assert!(hi-lo <= 10); // 0.25ms are acceptable
		}
	}

	#[test]
	pub fn results_do_not_depend_on_chunksize() {
		let bpm=121;
		let song_length = sample_rate * 8 *60/bpm;
		for latency in [0, 1024].iter() {
			let reference = {
				let device = DummyAudioDevice::new(1, *latency, 0);
				let mut metronome = AudioMetronome::new(device);
				let mut scope = DummyScope::new();
				scope.next(4*song_length);
				metronome.process(scope.time, song_length, 8, sample_rate, &scope);
				metronome.device.playback_buffers[0].clone()
			};
			
			for chunksize in [1, 127, 128, 1023, 1024].iter() {
				let device = DummyAudioDevice::new(1, *latency, 0);
				let mut metronome = AudioMetronome::new(device);
				let mut scope = DummyScope::new();
				scope.run_for(4*song_length, *chunksize, |scope| metronome.process(scope.time, song_length, 8, sample_rate, scope));
				assert!(reference == metronome.device.playback_buffers[0]);
			}
		}
	}

	
}
