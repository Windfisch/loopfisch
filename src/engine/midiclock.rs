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
