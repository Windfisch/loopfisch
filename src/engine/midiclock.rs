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
		let n_frames = scope.n_frames();
		let latency = self.device.playback_latency();
		let position = (position_uncompensated + latency) % song_length;
		
		let song_wraps_at = std::cmp::min(song_length - position, n_frames);

		let n_clocks = n_beats * 24;
		let period_per_clock = (song_length+n_clocks-1) / n_clocks; // round towards +inf

		let mut time_since_last_clock = position % period_per_clock;
		if time_since_last_clock == 0 {
			time_since_last_clock = period_per_clock;
		}

		println!("0..{}..{}", song_wraps_at, n_frames);
		println!("{}..{}..{}", position, position+song_wraps_at, position+n_frames);
		println!("{} clocks per {} samples -> period per clock: {}", song_length, n_clocks, period_per_clock);

		for timestamp in
			((period_per_clock - time_since_last_clock)..song_wraps_at).step_by(period_per_clock as usize)
			.chain( (song_wraps_at..n_frames).step_by(period_per_clock as usize) )
		{
			self.device.queue_event(MidiMessage {
				timestamp,
				data: [0xF8, 0, 0],
				datalen: 1
			});
		}

		self.device.commit_out_buffer(scope);
	}
}
