use std::cmp::min;
use jack;

pub struct MidiClock {
	out_port: jack::Port<jack::MidiOut>
}

impl MidiClock {
	pub fn new(client: &jack::Client) -> Result<MidiClock, jack::Error> {
		let out_port = client.register_port("midi_clock", jack::MidiOut::default())?;
		Ok(MidiClock {
			out_port
		})
	}

	pub fn process(&mut self, position: u32, period_per_beat: u32, scope: &jack::ProcessScope) {
		let latency = self.out_port.get_latency_range(jack::LatencyType::Playback).1;
		let period_per_clock = period_per_beat / 24;
		let mut time_since_last_beat = (position + latency) % period_per_clock;
		if time_since_last_beat == 0 {
			time_since_last_beat = period_per_clock;
		}
		let mut writer = self.out_port.writer(scope);
		for timestamp in ((period_per_clock - time_since_last_beat)..scope.n_frames()).step_by(period_per_clock as usize) {
			writer.write(&jack::RawMidi {
				time: timestamp,
				bytes: &[0xF8]
			}).unwrap();
		}
	}
}
