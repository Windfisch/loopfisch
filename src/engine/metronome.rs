use std::cmp::min;
use super::driver_traits::*;

pub struct AudioMetronome<T: AudioDeviceTrait> {
	device: T,
	volume: f32,
	unmuted: bool
}

impl<T: AudioDeviceTrait> AudioMetronome<T> {
	pub fn new(device: T) -> AudioMetronome<T> {
		AudioMetronome {
			device,
			volume: 0.3,
			unmuted: true
		}
	}

	pub fn process(&mut self, position: u32, period: u32, beats: u32, sample_rate: u32, scope: &T::Scope) {
		if !self.unmuted { return; }
		let latency = self.device.playback_latency();
		for buffer in self.device.playback_buffers(scope) {
			for i in 0..scope.n_frames() {
				buffer[i as usize] = self.volume * Self::process_one(position + i + latency, period, beats, sample_rate);
			}
		}
	}

	fn process_one(position: u32, period: u32, beats: u32, sample_rate: u32) -> f32 {
		let position_in_beat = position % period;
		let beat = position / period;
		let emphasis = (beat % beats) == 0;

		let click_length = sample_rate / 10;

		let volume = 1.0 - min(position_in_beat, click_length) as f32 / click_length as f32;
		let freq = if emphasis { 880 } else { 440 };

		let sawtooth: f32 = (position_in_beat as f32 / sample_rate as f32 * freq as f32).fract();
		let square = if sawtooth < 0.5 {0.0} else {1.0};

		return square * volume;
	}
}
