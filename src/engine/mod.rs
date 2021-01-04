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


	#[tokio::test]
	async fn special_devices_are_created() {
		let driver = dummy_driver::DummyDriver::new(0,0, 48000);
		let (_frontend, _events) = launch(driver.clone(), 1000);

		let guard = driver.lock();
		assert!(guard.audio_devices.len() == 1);
		assert!(guard.midi_devices.len() == 1);
		assert!(guard.audio_devices.contains_key("metronome"));
		assert!(guard.midi_devices.contains_key("clock"));
	}

	#[tokio::test]
	async fn device_creation() {
		let driver = dummy_driver::DummyDriver::new(0,0, 48000);
		let (mut frontend, _) = launch(driver.clone(), 1000);

		let aid = frontend.add_device("My Audio Device", 3).expect("Adding audio device failed");
		let mid = frontend.add_mididevice("My Midi Device").expect("Adding midi device failed");

		assert!(frontend.devices().len() == 1);
		assert!(frontend.devices().get(&aid).expect("could not find device").info().n_channels == 3);
		assert!(frontend.mididevices.contains_key(&mid));

		let guard = driver.lock();
		assert!(guard.audio_devices.len() == 2);
		assert!(guard.midi_devices.len() == 2);
		assert!(guard.audio_devices.contains_key("My Audio Device"));
		assert!(guard.midi_devices.contains_key("My Midi Device"));
		assert!(guard.audio_devices.get("My Audio Device").unwrap().record_buffers(&dummy_driver::DummyScope::new()).count() == 3);
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
		assert!(frontend.sample_rate() == 13337);
	}
	
	#[tokio::test]
	async fn song_position_wraps_and_transport_position_does_not_wrap() {
		let check = |length| {
			let sample_rate = 48000;
			let length_samples = sample_rate*length/1000;

			let driver = dummy_driver::DummyDriver::new(0,0, sample_rate);
			let (frontend, _) = launch(driver.clone(), length);

			assert!(frontend.loop_length() == length_samples);
			assert!(frontend.song_position() == 0);
			assert!(frontend.transport_position() == 0);

			for i in (128..3460).step_by(128) {
				driver.process(128);
				assert!(frontend.song_position() == i % length_samples);
				assert!(frontend.transport_position() == i);
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
			assert_eq!(dev.playback_buffers[0][t..t+11025], vec![0.0; 11025]);
			assert_eq!(dev.playback_buffers[0][t+11025..t+22050], dev.capture_buffers[0][(t+11025)..(t+22050)]);
			assert_eq!(dev.playback_buffers[1][t..t+11025], vec![0.0; 11025]);
			assert_eq!(dev.playback_buffers[1][t+11025..t+22050], dev.capture_buffers[1][(t+11025)..(t+22050)]);
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
	async fn audio_takes_can_be_recorded_and_finished_early() {// TODO test latency handling, test early and late finish
		for on_point_offset in vec![0,5] { // check this for loop points being "on point" with the chunksizes and being not on point.
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
			
			// let it record for the second and third period; finish recording during the third
			driver.process_for(70000 - on_point_offset, 128);
			frontend.finish_audiotake(dev_id, take_id, 88200).unwrap();
			driver.process_for(18200 + on_point_offset, 128);
			assert_receive(&mut events, &Event::AudioTakeStateChanged(dev_id, take_id, RecordState::Finished, 44100+88200)).await;

			// let it play for four periods, i.e. two repetitions
			driver.process_for(2*88200 - on_point_offset, 128);

			let d = driver.lock();
			let dev = d.audio_devices.get("audiodev").unwrap().lock().unwrap();
			let capture_begin = 44100;
			let playback_begin = capture_begin + 88200;
			assert!(dev.playback_buffers[0][playback_begin..playback_begin+88200] == dev.capture_buffers[0][capture_begin..capture_begin+88200],
				"first repetition was not played correctly");
			assert!(dev.playback_buffers[0][(playback_begin+88200)..(playback_begin+2*88200)] == dev.capture_buffers[0][capture_begin..capture_begin+88200],
				"second repetition was not played correctly");
		}
	}
}
