mod frontend;
mod retry_channel;
mod messages;
mod takes;
mod data;
mod shared;
mod backend;
mod jack_driver;
mod metronome;
mod midi_registry;
mod midiclock;
mod driver_traits;
#[cfg(test)]
mod dummy_driver;
#[cfg(test)]
mod testutils;

use backend::*;

use std::collections::HashMap;

pub use data::{Event, RecordState};

use shared::SharedThreadState;

use messages::*;
pub use frontend::{FrontendTrait, FrontendThreadState};
use retry_channel::*;

use std::sync::atomic::*;
use std::sync::Arc;
use crate::id_generator::IdGenerator;
use driver_traits::*;

use metronome::AudioMetronome;
use midiclock::MidiClock;
use crate::realtime_send_queue;

pub use jack_driver::JackDriver;

fn create_thread_states<Driver: DriverTrait>(mut driver: Driver, devices: Vec<Driver::AudioDev>, mididevices: Vec<Driver::MidiDev>, song_length: u32) -> (FrontendThreadState<Driver>, realtime_send_queue::Consumer<Event>) {
	let shared = Arc::new(SharedThreadState {
		song_length: AtomicU32::new(song_length),
		song_position: AtomicU32::new(0),
		transport_position: AtomicU32::new(0),
	});

	let (command_sender, command_receiver) = ringbuf::RingBuffer::<Message<Driver::AudioDev, Driver::MidiDev>>::new(16).split();

	let frontend_devices = devices.iter().enumerate().map(|d| (d.0, frontend::GuiAudioDevice { info: d.1.info(), takes: HashMap::new() }) ).collect();
	let frontend_mididevices = mididevices.iter().enumerate().map(|d| (d.0, frontend::GuiMidiDevice { info: d.1.info(), takes: HashMap::new() }) ).collect();

	let (event_producer, event_consumer) = realtime_send_queue::new(64);

	let metronome = AudioMetronome::new( driver.new_audio_device(1, "metronome").unwrap() );
	let midiclock = MidiClock::new( driver.new_midi_device("clock").unwrap() );

	let audio_thread_state = AudioThreadState::new(driver.sample_rate(), devices, mididevices, metronome, midiclock, command_receiver, song_length, shared.clone(), event_producer);

	driver.activate(audio_thread_state);

	let frontend_thread_state = FrontendThreadState {
		command_channel: RetryChannelPush(command_sender),
		devices: frontend_devices,
		mididevices: frontend_mididevices,
		shared: Arc::clone(&shared),
		next_id: IdGenerator::new(),
		driver
	};

	return (frontend_thread_state, event_consumer);
}

pub fn launch<Driver: DriverTrait>(driver: Driver, loop_length_msec: u32) -> (FrontendThreadState<Driver>, realtime_send_queue::Consumer<Event>) {

	let loop_length = driver.sample_rate() as u32 * loop_length_msec / 1000;
	let (frontend_thread_state, event_queue) = create_thread_states(driver, vec![], vec![], loop_length);

	return (frontend_thread_state, event_queue);
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio;
	use std::sync::{Arc,Mutex};

// GRCOV_EXCL_START
	fn slice_diff<T: PartialEq + std::fmt::Debug>(lhs: &[T], rhs: &[T]) {
		if let Some(result) = lhs.iter().zip(rhs.iter()).map(|x| x.0 != x.1).enumerate().find(|t| t.1) {
			let index = result.0;
			let max = std::cmp::max(lhs.len(), rhs.len());
			let lo = if index < 10 { 0 } else { index-10 };
			let hi = if index + 10 >= max { max } else { index+10 };

			println!("First difference at {}, context: {:?} != {:?}", index, &lhs[lo..hi], &rhs[lo..hi]);
		}
	}

	/// Asserts two (large) slices are equal. Prints a small context around the first
	/// difference, if unequal
	macro_rules! assert_sleq {
		($lhs:expr, 0.0) => {{
			let lhs = &$lhs;
			let rhs = &vec![0.0; lhs.len()];
			if *lhs != *rhs {
				slice_diff(lhs, rhs);
				panic!("Slices are different!");
			}
		}};
		($lhs:expr, 0.0, $reason:expr) => {{
			let lhs = &$lhs;
			let rhs = &vec![0.0; lhs.len()];
			if *lhs != *rhs {
				slice_diff(lhs, rhs);
				panic!($reason);
			}
		}};
		($lhs:expr, $rhs:expr) => {{
			let lhs = &$lhs;
			let rhs = &$rhs;
			if *lhs != *rhs {
				slice_diff(lhs, rhs);
				panic!("Slices are different!");
			}
		}};
		($lhs:expr, $rhs:expr, $reason:expr) => {{
			let lhs = &$lhs;
			let rhs = &$rhs;
			if *lhs != *rhs {
				slice_diff(lhs, rhs);
				panic!($reason);
			}
		}}
	}
// GRCOV_EXCL_STOP


	#[tokio::test]
	async fn special_devices_are_created() {
		let driver = dummy_driver::DummyDriver::new(0,0, 48000);
		let (_frontend, _events) = launch(driver.clone(), 1000);

		let guard = driver.lock();
		assert_eq!(guard.audio_devices.len(), 1);
		assert_eq!(guard.midi_devices.len(), 1);
		assert!(guard.audio_devices.contains_key("metronome"));
		assert!(guard.midi_devices.contains_key("clock"));
	}

	#[tokio::test]
	async fn device_creation() {
		let driver = dummy_driver::DummyDriver::new(0,0, 48000);
		let (mut frontend, _) = launch(driver.clone(), 1000);

		let aid = frontend.add_device("My Audio Device", 3).expect("Adding audio device failed");
		let mid = frontend.add_mididevice("My Midi Device").expect("Adding midi device failed");

		assert_eq!(frontend.devices().len(), 1);
		assert_eq!(frontend.devices().get(&aid).expect("could not find device").info().n_channels, 3);
		assert!(frontend.mididevices.contains_key(&mid));

		let guard = driver.lock();
		assert_eq!(guard.audio_devices.len(), 2);
		assert_eq!(guard.midi_devices.len(), 2);
		assert!(guard.audio_devices.contains_key("My Audio Device"));
		assert!(guard.midi_devices.contains_key("My Midi Device"));
		assert_eq!(guard.audio_devices.get("My Audio Device").unwrap().record_buffers(&dummy_driver::DummyScope::new()).count(), 3);
	}

	#[tokio::test]
	async fn creating_too_many_devices_fails_gracefully() {
		let driver = dummy_driver::DummyDriver::new(0,0, 48000);
		let (mut frontend, _) = launch(driver.clone(), 1000);

		for i in 0..32 {
			frontend.add_device(&format!("audio{}",i), 2).expect("Adding audio device failed");
			driver.process(32);
		}
		frontend.add_device("audioX", 2).expect_err("Adding audio device should have failed");
		
		for i in 0..32 {
			frontend.add_mididevice(&format!("midi{}",i)).expect("Adding midi device failed");
			driver.process(32);
		}
		frontend.add_mididevice("midiX").expect_err("Adding midi device should have failed");
	}
	
	#[tokio::test]
	async fn sample_rate_is_reported() {
		let driver = dummy_driver::DummyDriver::new(0,0, 13337);
		let (frontend, _) = launch(driver.clone(), 1000);
		assert_eq!(frontend.sample_rate(), 13337);
	}
	
	#[tokio::test]
	async fn song_position_wraps_and_transport_position_does_not_wrap() {
		let check = |length| {
			let sample_rate = 48000;
			let length_samples = sample_rate*length/1000;

			let driver = dummy_driver::DummyDriver::new(0,0, sample_rate);
			let (frontend, _) = launch(driver.clone(), length);

			assert_eq!(frontend.loop_length(), length_samples);
			assert_eq!(frontend.song_position(), 0);
			assert_eq!(frontend.transport_position(), 0);

			for i in (128..3460).step_by(128) {
				driver.process(128);
				assert_eq!(frontend.song_position(), i % length_samples);
				assert_eq!(frontend.transport_position(), i);
			}
		};

		check(1000); // loop length is divisible by the process chunk size
		check(1001); // loop length is not divisible by the chunk size
	}

	#[tokio::test]
	async fn midiclock_is_always_active() {
		for latency in vec![0,256] {
			let driver = dummy_driver::DummyDriver::new(latency, 0, 96000);
			let (_frontend, _) = launch(driver.clone(), 1000);
			driver.process_for(48000, 128);
			let d = driver.lock();
			let dev = d.midi_devices.get("clock").unwrap().lock().unwrap();
			assert_eq!(dev.committed.len(), 2 * 24);
			if latency == 0 {
				assert_eq!(dev.committed[0].timestamp, 0);
			}
			else {
				assert_eq!(dev.committed.last().unwrap().timestamp, 48000 - latency);
			}
		}
	}

	#[tokio::test]
	async fn midiclock_reacts_to_set_loop_length() {
		for latency in vec![0,64] {
			let driver = dummy_driver::DummyDriver::new(latency, 0, 96000);
			let (mut frontend, _) = launch(driver.clone(), 1000);
			frontend.set_loop_length(48000, 8).unwrap();
			driver.process_for(48000, 128);
			let d = driver.lock();
			let dev = d.midi_devices.get("clock").unwrap().lock().unwrap();
			assert_eq!(dev.committed.len(), 8 * 24);
			if latency == 0 {
				assert_eq!(dev.committed[0].timestamp, 0);
			}
			else {
				assert_eq!(dev.committed.last().unwrap().timestamp, 48000 - latency);
			}
		}
	}

	#[tokio::test]
	async fn metronome_is_always_active_and_reacts_to_set_loop_length() {
		use super::testutils;
		let driver = dummy_driver::DummyDriver::new(2048, 0, 48000);
		let (mut frontend, _) = launch(driver.clone(), 1337);
		frontend.set_loop_length(480000, 8).unwrap();
		driver.process_for(480000, 128);
		let d = driver.lock();
		let dev = d.audio_devices.get("metronome").unwrap().lock().unwrap();
		let ticks = testutils::ticks(&dev.playback_buffers[0], 0.24);
		assert_eq!(ticks.len(), 8);
		assert_eq!(*ticks.last().unwrap(), 480000-2048)
	}

	#[tokio::test]
	async fn restart_midi_transport() {
		use crate::midi_message::MidiMessage;
		let driver = dummy_driver::DummyDriver::new(2048, 0, 44100);
		let (mut frontend, _) = launch(driver.clone(), 1000);

		let id = frontend.add_mididevice("mididev").unwrap();
		driver.process_for(13337, 256);
		frontend.restart_midi_transport(id).unwrap();
		driver.process_for(88200, 256);

		let d = driver.lock();
		let dev = d.midi_devices.get("mididev").unwrap().lock().unwrap();
		assert_eq!(dev.committed, vec![
			MidiMessage { timestamp: 13337, data: [0xFC, 0, 0], datalen: 1 },
			MidiMessage { timestamp: 44100, data: [0xFA, 0, 0], datalen: 1 },
		]);
	}

	fn fill_audio_device(driver: &dummy_driver::DummyDriver, name: &str, length: usize) {
		let d = driver.lock();
		let mut dev = d.audio_devices.get(name).unwrap().lock().unwrap();
		dev.capture_buffers[0] = (0..length).map(|x| x as f32).collect();
		dev.capture_buffers[1] = (0..length).map(|x| -(x as f32)).collect();
	}

	#[tokio::test]
	async fn audio_echo_can_be_enabled_and_disabled() {
		let driver = dummy_driver::DummyDriver::new(0, 0, 44100);
		let (mut frontend, _) = launch(driver.clone(), 1000);
		let id = frontend.add_device("audiodev", 2).unwrap();
		fill_audio_device(&driver, "audiodev", 89000);

		for _ in 0..4 {
			driver.process_for(11025, 128);
			frontend.set_audiodevice_echo(id, true).unwrap();
			driver.process_for(11025, 128);
			frontend.set_audiodevice_echo(id, false).unwrap();
		}

		let d = driver.lock();
		let dev = d.audio_devices.get("audiodev").unwrap().lock().unwrap();
		for t in (0..88200).step_by(22050) {
			assert_sleq!(dev.playback_buffers[0][t..t+11025], 0.0);
			assert_sleq!(dev.playback_buffers[0][t+11025..t+22050], dev.capture_buffers[0][(t+11025)..(t+22050)]);
			assert_sleq!(dev.playback_buffers[1][t..t+11025], 0.0);
			assert_sleq!(dev.playback_buffers[1][t+11025..t+22050], dev.capture_buffers[1][(t+11025)..(t+22050)]);
		}
	}

	#[tokio::test]
	async fn timestamp_events_are_sent() {
		for chunksize in vec![256, 100] {
			let driver = dummy_driver::DummyDriver::new(0, 0, 44100);
			let (_frontend, mut events) = launch(driver.clone(), 1000);

			driver.process_for(44100*4+2*chunksize, chunksize);
			for t in (0..44100*4+1).step_by(44100).skip(1) {
				let time_of_wrap_in_chunk = 1 + (t-1) % chunksize;
				let time_at_chunk_end = chunksize - time_of_wrap_in_chunk;
				assert_eq!(events.receive().await, Event::Timestamp(time_at_chunk_end, t + time_at_chunk_end));
			}
		}
	}

	/// Checks if the next element in the event queue is `required_event`, ignoring all Timestamp events
	/// on the way. Fails if a different or no element was found after 1 second.
	async fn assert_receive(events: &mut crate::realtime_send_queue::Consumer<Event>, required_event: &Event) {
		async fn wait_for(events: &mut crate::realtime_send_queue::Consumer<Event>, required_event: &Event) {
			loop {
				let ev = events.receive().await;
				if ev == *required_event {
					return;
				}
				match ev {
					Event::Timestamp(_, _) => continue,
					other => assert!(false, "Expected event {:?} but found {:?}", required_event, other)
				}
			}
		}

		let result = async_std::future::timeout(std::time::Duration::from_millis(1000), wait_for(events, required_event)).await;
		assert!(result.is_ok(), "Expected event {:?} was not received after 1 sec.", required_event);
	}

	#[tokio::test]
	async fn audio_takes_can_be_recorded() {
		// on_point_offset controls whether loop points align with chunk boundaries (=0) or not (>0 and < chunksize).
		// finish_late controls whether the take is finished before its actual end, or finished retroactively afterwards.
		for (on_point_offset, finish_late) in vec![(0, false) , (5, false), (0, true)] {
			println!("on_point_offset = {}; finish_late = {};", on_point_offset, finish_late);
			let driver = dummy_driver::DummyDriver::new(0, 0, 44100);
			let (mut frontend, mut events) = launch(driver.clone(), 1000);
			frontend.set_loop_length(44100,4).unwrap();
			let dev_id = frontend.add_device("audiodev", 2).unwrap();
			fill_audio_device(&driver, "audiodev", 44100*8);

			// add a take during the first period
			driver.process_for(30000, 128);
			let take_id = frontend.add_audiotake(dev_id, true).unwrap();
			driver.process_for(14100 + on_point_offset, 128);
			assert_receive(&mut events, &Event::AudioTakeStateChanged(dev_id, take_id, RecordState::Recording, 44100)).await;
			
			if !finish_late {
				// let it record for the second and third period; finish recording during the third
				driver.process_for(70000 - on_point_offset, 128);
				frontend.finish_audiotake(dev_id, take_id, 88200).unwrap();
				driver.process_for(18200 + on_point_offset, 128);
				assert_receive(&mut events, &Event::AudioTakeStateChanged(dev_id, take_id, RecordState::Finished, 44100+88200)).await;
			}
			else {
				// let it record for the second and third period and a bit of the fourth period, then retroactively finish
				driver.process_for(88200 + 300, 128);
				frontend.finish_audiotake(dev_id, take_id, 88200).unwrap();
				driver.process_for(33, 128);
				assert_receive(&mut events, &Event::AudioTakeStateChanged(dev_id, take_id, RecordState::Finished, 44100+88200)).await;
			}
			// let it play for (at least) four periods, i.e. two repetitions
			driver.process_for(2*88200 - on_point_offset, 128);

			let d = driver.lock();
			let dev = d.audio_devices.get("audiodev").unwrap().lock().unwrap();
			let late_offset = if finish_late { 300 } else { 0 };
			let capture_begin = 44100;
			let playback_begin = capture_begin + 88200;
			for channel in 0..=1 {
				assert_sleq!(dev.playback_buffers[channel][0..playback_begin+late_offset], 0.0, "expected silence at the beginning");
				assert_sleq!(dev.playback_buffers[channel][playback_begin+late_offset..playback_begin+88200], dev.capture_buffers[channel][capture_begin+late_offset..capture_begin+88200],
					"first repetition was not played correctly");
				assert_sleq!(dev.playback_buffers[channel][(playback_begin+88200)..(playback_begin+2*88200)], dev.capture_buffers[channel][capture_begin..capture_begin+88200],
					"second repetition was not played correctly");
			}
		}
	}
	
	#[tokio::test]
	async fn latency_compensation() {
		let playback_latency = 64;
		let capture_latency = 128;
		let driver = dummy_driver::DummyDriver::new(playback_latency as u32, capture_latency as u32, 44100);
		let (mut frontend, _) = launch(driver.clone(), 1000);
		frontend.set_loop_length(44100,4).unwrap();
		let dev_id = frontend.add_device("audiodev", 2).unwrap();
		fill_audio_device(&driver, "audiodev", 44100*8);

		let take_id = frontend.add_audiotake(dev_id, true).unwrap();
		frontend.finish_audiotake(dev_id, take_id, 44100).unwrap();
		driver.process_for(3*44100, 128);
			
		let d = driver.lock();
		let dev = d.audio_devices.get("audiodev").unwrap().lock().unwrap();
		let begin = 44100-playback_latency;
		assert_sleq!(dev.playback_buffers[0][0..begin], 0.0,
			"expected silence at the beginning");
		assert_sleq!(dev.playback_buffers[0][begin..begin+44100], dev.capture_buffers[0][capture_latency..44100 + capture_latency],
			"first repetition was not played correctly");
		assert_sleq!(dev.playback_buffers[0][begin+44100..begin+2*44100], dev.capture_buffers[0][capture_latency..44100 + capture_latency],
			"second repetition was not played correctly");

		// TODO FIXME: test for midi takes
	}

	#[tokio::test]
	async fn audio_takes_can_be_muted_and_unmuted() {
		let driver = dummy_driver::DummyDriver::new(0, 0, 44100);
		let (mut frontend, _) = launch(driver.clone(), 1000);
		frontend.set_loop_length(44100,4).unwrap();
		let dev_id = frontend.add_device("audiodev", 2).unwrap();
		fill_audio_device(&driver, "audiodev", 44100*8);

		driver.process_for(22050, 128); // not capturing
		let take_id = frontend.add_audiotake(dev_id, false).unwrap();
		frontend.finish_audiotake(dev_id, take_id, 44100).unwrap();
		driver.process_for(22050, 128); // not capturing
		driver.process_for(44100, 128); // capturing

		driver.process_for(22050, 128); // playback, muted
		frontend.set_audiotake_unmuted(dev_id, take_id, true).unwrap();
		driver.process_for(44100, 128); // playback, unmuted
		frontend.set_audiotake_unmuted(dev_id, take_id, false).unwrap();
		driver.process_for(22050, 128); // playback, muted
			
		let d = driver.lock();
		let dev = d.audio_devices.get("audiodev").unwrap().lock().unwrap();
		let t = 22050;
		assert_sleq!(dev.playback_buffers[0][4*t..5*t], 0.0, "expected silence when muted");
		assert_sleq!(dev.playback_buffers[0][5*t..6*t], dev.capture_buffers[0][3*t..4*t], "unmuted part of first repetition was not played correctly");
		assert_sleq!(dev.playback_buffers[0][6*t..7*t], dev.capture_buffers[0][2*t..3*t], "unmuted part of second repetition was not played correctly");
		assert_sleq!(dev.playback_buffers[0][7*t..8*t], 0.0, "expected silence when muted");
	}
}
