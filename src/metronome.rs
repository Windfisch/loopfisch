use std::cmp::min;
use jack;

pub struct AudioMetronome {
	out_port: jack::Port<jack::AudioOut>,
	sample_rate: usize,
	volume: f32,
	unmuted: bool
}

impl AudioMetronome {
	pub fn new(client: &jack::Client) -> Result<AudioMetronome, jack::Error> {
		let out_port = client.register_port("metronome", jack::AudioOut::default())?;
		Ok(AudioMetronome {
			out_port,
			sample_rate: client.sample_rate(),
			volume: 0.3,
			unmuted: true
		})
	}

	pub fn process(&mut self, position: u32, period: u32, beats: u32, scope: &jack::ProcessScope) {
		if !self.unmuted { return; }
		let latency = self.out_port.get_latency_range(jack::LatencyType::Playback).1;
		let buffer = self.out_port.as_mut_slice(scope);
		for i in 0..scope.n_frames() {
			buffer[i as usize] = self.volume * Self::process_one(position + i + latency, period, beats, self.sample_rate as u32);
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
