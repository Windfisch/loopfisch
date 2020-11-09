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

use backend::*;

pub use data::Event;

use shared::SharedThreadState;

use messages::*;
pub use frontend::FrontendThreadState;
use retry_channel::*;

use std::sync::atomic::*;
use std::sync::Arc;
use crate::id_generator::IdGenerator;

use jack_driver::*;

use metronome::AudioMetronome;
use crate::realtime_send_queue;

pub fn create_thread_states(client: jack::Client, devices: Vec<AudioDevice>, mididevices: Vec<MidiDevice>, metronome: AudioMetronome, song_length: u32) -> (FrontendThreadState, realtime_send_queue::Consumer<Event>) {
	let shared = Arc::new(SharedThreadState {
		song_length: AtomicU32::new(1),
		song_position: AtomicU32::new(0),
		transport_position: AtomicU32::new(0),
	});

	let (command_sender, command_receiver) = ringbuf::RingBuffer::<Message>::new(16).split();

	let frontend_devices = devices.iter().enumerate().map(|d| (d.0, frontend::GuiAudioDevice { info: d.1.info(), takes: Vec::new() }) ).collect();
	let frontend_mididevices = mididevices.iter().enumerate().map(|d| (d.0, frontend::GuiMidiDevice { info: d.1.info(), takes: Vec::new() }) ).collect();

	let (event_producer, event_consumer) = realtime_send_queue::new(64);

	let mut audio_thread_state = AudioThreadState::new(devices, mididevices, metronome, command_receiver, song_length, shared.clone(), event_producer);

	let process_callback = move |client: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
		audio_thread_state.process_callback(client, ps)
	};
	let process = jack::ClosureProcessHandler::new(process_callback);
	let active_client = client.activate_async(Notifications, process).unwrap();


	let frontend_thread_state = FrontendThreadState {
		command_channel: RetryChannelPush(command_sender),
		devices: frontend_devices,
		mididevices: frontend_mididevices,
		shared: Arc::clone(&shared),
		next_id: IdGenerator::new(),
		async_client: Box::new(active_client)
	};


	return (frontend_thread_state, event_consumer);
}

pub fn launch() -> (FrontendThreadState, realtime_send_queue::Consumer<Event>) {
	let (client, _status) = jack::Client::new("loopfisch", jack::ClientOptions::NO_START_SERVER).unwrap();

	println!("JACK running with sampling rate {} Hz, buffer size = {} samples", client.sample_rate(), client.buffer_size());

	let audiodev = AudioDevice::new(&client, 2, "fnord").unwrap();
	let audiodev2 = AudioDevice::new(&client, 2, "dronf").unwrap();
	let mididev = MidiDevice::new(&client, "midi").unwrap();
	let mididev2 = MidiDevice::new(&client, "midi2").unwrap();
	let devices = vec![audiodev, audiodev2];
	let mididevs = vec![mididev, mididev2];

	let metronome = AudioMetronome::new(&client).unwrap();

	let loop_length = client.sample_rate() as u32 * 4;
	let (frontend_thread_state, event_queue) = create_thread_states(client, devices, mididevs, metronome, loop_length);


	frontend_thread_state.async_client.as_client().connect_ports_by_name("loopfisch:fnord_out1", "system:playback_1").unwrap();
	frontend_thread_state.async_client.as_client().connect_ports_by_name("loopfisch:fnord_out2", "system:playback_2").unwrap();
	frontend_thread_state.async_client.as_client().connect_ports_by_name("system:capture_1", "loopfisch:fnord_in1").unwrap();
	frontend_thread_state.async_client.as_client().connect_ports_by_name("system:capture_2", "loopfisch:fnord_in2").unwrap();

	return (frontend_thread_state, event_queue);
}

