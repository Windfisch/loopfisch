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

	pub fn process(&mut self, position: u32, period_per_beat: u32, scope: &T::Scope) {
		let latency = self.device.playback_latency();
		let period_per_clock = period_per_beat / 24;
		let mut time_since_last_beat = (position + latency) % period_per_clock;
		if time_since_last_beat == 0 {
			time_since_last_beat = period_per_clock;
		}

		for timestamp in ((period_per_clock - time_since_last_beat)..scope.n_frames()).step_by(period_per_clock as usize) {
			self.device.queue_event(MidiMessage {
				timestamp,
				data: [0xF8, 0, 0],
				datalen: 1
			});
		}

		self.device.commit_out_buffer(scope);
	}
}
