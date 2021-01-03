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


	#[tokio::test]
	async fn special_devices_are_created() {
		let driver = dummy_driver::DummyDriver::new(0,0, 48000);
		let (mut frontend, _events) = launch(driver.clone(), 1000);

		let guard = driver.lock();
		assert!(guard.audio_devices.len() == 1);
		assert!(guard.midi_devices.len() == 1);
		assert!(guard.audio_devices.contains_key("metronome"));
		assert!(guard.midi_devices.contains_key("clock"));
	}

	#[tokio::test]
	async fn device_creation() {
		let driver = dummy_driver::DummyDriver::new(0,0, 48000);
		let (mut frontend, _events) = launch(driver.clone(), 1000);

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
		let (mut frontend, mut events) = launch(driver.clone(), 1000);

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
}
