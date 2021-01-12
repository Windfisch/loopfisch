// Engine integration tests

use super::*;
use tokio;
use smallvec::smallvec;
use testutils::*;
use dummy_driver::*;
use crate::midi_message::MidiMessage;
use crate::realtime_send_queue;

fn midi_events_in_range(iter: impl Iterator<Item = DummyMidiEvent>, range: std::ops::Range<u32>) -> impl Iterator<Item = DummyMidiEvent> {
	let start = range.start;
	iter
		.filter(move |e| range.contains(&e.time))
		.map(move |e| DummyMidiEvent { data: e.data.clone(), time: e.time - start })
}

fn to_dummy_midi_event(iter: impl Iterator<Item = MidiMessage>) -> impl Iterator<Item = DummyMidiEvent> {
	iter.map(|e| DummyMidiEvent { data: e.data[0..e.datalen as usize].into(), time: e.timestamp })
}

#[tokio::test]
async fn special_devices_are_created() {
	let driver = DummyDriver::new(0,0, 48000);
	let (_frontend, _events) = launch(driver.clone(), 1000);

	let guard = driver.lock();
	assert_eq!(guard.audio_devices.len(), 1);
	assert_eq!(guard.midi_devices.len(), 1);
	assert!(guard.audio_devices.contains_key("metronome"));
	assert!(guard.midi_devices.contains_key("clock"));
}

#[tokio::test]
async fn device_creation() {
	let driver = DummyDriver::new(0,0, 48000);
	let (mut frontend, _) = launch(driver.clone(), 1000);

	let aid = frontend.add_device("My Audio Device", 3).expect("Adding audio device failed");
	let mid = frontend.add_mididevice("My Midi Device").expect("Adding midi device failed");

	assert_eq!(frontend.devices().len(), 1);
	assert_eq!(frontend.devices().get(&aid).expect("could not find device").info().n_channels, 3);
	assert!(frontend.mididevices().contains_key(&mid));

	let guard = driver.lock();
	assert_eq!(guard.audio_devices.len(), 2);
	assert_eq!(guard.midi_devices.len(), 2);
	assert!(guard.audio_devices.contains_key("My Audio Device"));
	assert!(guard.midi_devices.contains_key("My Midi Device"));
	assert_eq!(guard.audio_devices.get("My Audio Device").unwrap().record_buffers(&DummyScope::new()).count(), 3);
}

#[tokio::test]
async fn creating_too_many_devices_fails_gracefully() {
	let driver = DummyDriver::new(0,0, 48000);
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
	let driver = DummyDriver::new(0,0, 13337);
	let (frontend, _) = launch(driver.clone(), 1000);
	assert_eq!(frontend.sample_rate(), 13337);
}

#[tokio::test]
async fn song_position_wraps_and_transport_position_does_not_wrap() {
	let check = |length| {
		let sample_rate = 48000;
		let length_samples = sample_rate*length/1000;

		let driver = DummyDriver::new(0,0, sample_rate);
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
		let driver = DummyDriver::new(latency, 0, 96000);
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
async fn song_length_cannot_be_changed_if_takes_exist() {
	{
		let driver = DummyDriver::new(0, 0, 48000);
		let (mut frontend, _) = launch(driver.clone(), 1000);
		let id = frontend.add_device("dev", 2).unwrap();
		frontend.add_audiotake(id, false).unwrap();
		frontend.set_loop_length(48000, 8).expect_err("frontend should not allow changing song length when audio takes exist");
	}
	{
		let driver = DummyDriver::new(0, 0, 48000);
		let (mut frontend, _) = launch(driver.clone(), 1000);
		let id = frontend.add_mididevice("dev").unwrap();
		frontend.add_miditake(id, false).unwrap();
		frontend.set_loop_length(48000, 8).expect_err("frontend should not allow changing song length when midi takes exist");
	}
}

#[tokio::test]
async fn midiclock_reacts_to_set_loop_length() {
	for latency in vec![0,64] {
		let driver = DummyDriver::new(latency, 0, 96000);
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
	let driver = DummyDriver::new(2048, 0, 48000);
	let (mut frontend, _) = launch(driver.clone(), 1337);
	frontend.set_loop_length(480000, 8).unwrap();
	driver.process_for(480000, 128);
	let d = driver.lock();
	let dev = d.audio_devices.get("metronome").unwrap().lock().unwrap();
	let ticks = ticks(&dev.playback_buffers[0], 0.24);
	assert_eq!(ticks.len(), 8);
	assert_eq!(*ticks.last().unwrap(), 480000-2048)
}

#[tokio::test]
async fn restart_midi_transport() {
	let driver = DummyDriver::new(2048, 0, 44100);
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

fn fill_audio_device(driver: &DummyDriver, name: &str, length: usize) {
	let d = driver.lock();
	let mut dev = d.audio_devices.get(name).unwrap().lock().unwrap();
	dev.capture_buffers[0] = (0..length).map(|x| x as f32).collect();
	dev.capture_buffers[1] = (0..length).map(|x| -(x as f32)).collect();
}

fn fill_midi_device(driver: &DummyDriver, name: &str, length: usize) {
	let d = driver.lock();
	let mut dev = d.midi_devices.get(name).unwrap().lock().unwrap();
	for (i,time) in (0..=(length as u32 - 11025)).step_by(11025).enumerate() {
		let note = ((42 + i) % 128) as u8;
		dev.incoming_events.push(DummyMidiEvent {
			data: smallvec![0x90, note, 96],
			time
		});
		dev.incoming_events.push(DummyMidiEvent {
			data: smallvec![0x80, note, 96],
			time: time + 5512
		});
	}
}

#[tokio::test]
async fn audio_echo_can_be_enabled_and_disabled() {
	let driver = DummyDriver::new(0, 0, 44100);
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
		let driver = DummyDriver::new(0, 0, 44100);
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
async fn assert_receive(events: &mut realtime_send_queue::Consumer<Event>, required_event: &Event) {
	async fn wait_for(events: &mut realtime_send_queue::Consumer<Event>, required_event: &Event) {
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

macro_rules! recording_test {
	($add_take:ident, $finish_take:ident, $TakeStateChanged:ident, setup_device: $setup_device:expr, check: $check:expr) => {{
		// on_point_offset controls whether loop points align with chunk boundaries (=0) or not (>0 and < chunksize).
		// finish_late controls whether the take is finished before its actual end, or finished retroactively afterwards.
		for (on_point_offset, finish_late) in vec![(0, false) , (5, false), (0, true)] {
			println!("on_point_offset = {}; finish_late = {};", on_point_offset, finish_late);
			let driver = DummyDriver::new(0, 0, 44100);
			let (mut frontend, mut events) = launch(driver.clone(), 1000);
			frontend.set_loop_length(44100,4).unwrap();
			let dev_id = $setup_device(&mut frontend, &driver);

			// add a take during the first period
			driver.process_for(30000, 128);
			let take_id = frontend.$add_take(dev_id, true).unwrap();
			driver.process_for(14100 + on_point_offset, 128);
			assert_receive(&mut events, &Event::$TakeStateChanged(dev_id, take_id, RecordState::Recording, 44100)).await;
			
			if !finish_late {
				// let it record for the second and third period; finish recording during the third
				driver.process_for(70000 - on_point_offset, 128);
				frontend.$finish_take(dev_id, take_id, 88200).unwrap();
				driver.process_for(18200 + on_point_offset, 128);
				assert_receive(&mut events, &Event::$TakeStateChanged(dev_id, take_id, RecordState::Finished, 44100+88200)).await;
			}
			else {
				// let it record for the second and third period and a bit of the fourth period, then retroactively finish
				driver.process_for(88200 + 300, 128);
				frontend.$finish_take(dev_id, take_id, 88200).unwrap();
				driver.process_for(33, 128);
				assert_receive(&mut events, &Event::$TakeStateChanged(dev_id, take_id, RecordState::Finished, 44100+88200)).await;
			}
			// let it play for (at least) four periods, i.e. two repetitions
			driver.process_for(2*88200 - on_point_offset, 128);

			let late_offset = if finish_late { 300 } else { 0 };
			let capture_begin = 44100;
			let playback_begin = capture_begin + 88200;

			$check(&driver, late_offset, capture_begin, playback_begin);
		}
	}}
}

#[tokio::test]
async fn audio_takes_can_be_recorded() {
	recording_test!(add_audiotake, finish_audiotake, AudioTakeStateChanged,
		setup_device: |frontend: &mut FrontendThreadState<DummyDriver>, driver| {
			let id = frontend.add_device("dev", 2).unwrap();
			fill_audio_device(driver, "dev", 44100*8);
			id
		},
		check: |driver: &DummyDriver, late_offset, capture_begin, playback_begin| {
			let d = driver.lock();
			let dev = d.audio_devices.get("dev").unwrap().lock().unwrap();
			for channel in 0..=1 {
				assert_sleq!(dev.playback_buffers[channel][0..playback_begin+late_offset], 0.0, "expected silence at the beginning");
				assert_sleq!(dev.playback_buffers[channel][playback_begin+late_offset..playback_begin+88200], dev.capture_buffers[channel][capture_begin+late_offset..capture_begin+88200],
					"first repetition was not played correctly");
				assert_sleq!(dev.playback_buffers[channel][(playback_begin+88200)..(playback_begin+2*88200)], dev.capture_buffers[channel][capture_begin..capture_begin+88200],
					"second repetition was not played correctly");
			}
		}
	);
}

#[tokio::test]
async fn midi_takes_can_be_recorded() {
	recording_test!(add_miditake, finish_miditake, MidiTakeStateChanged,
		setup_device: |frontend: &mut FrontendThreadState<DummyDriver>, driver: &DummyDriver| {
			let id = frontend.add_mididevice("dev").unwrap();
			fill_midi_device(driver, "dev", 44100*8);
			id
		},
		check: |driver: &DummyDriver, late_offset, capture_begin, playback_begin| {
			let d = driver.lock();
			let dev = d.midi_devices.get("dev").unwrap().lock().unwrap();
			let reference : Vec<_> = midi_events_in_range(dev.incoming_events.iter().cloned(), capture_begin..capture_begin+88200).collect();
			println!("reference: {:?}\ncommitted: {:?}", reference, dev.committed);
			assert!(reference.len() > 0);
			assert_iter_eq(
				midi_events_in_range(dev.incoming_events.iter().cloned(), capture_begin+late_offset..capture_begin+88200),
				midi_events_in_range(to_dummy_midi_event(dev.committed.iter().cloned()), playback_begin+late_offset..playback_begin+88200)
			);
			assert_iter_eq(
				midi_events_in_range(dev.incoming_events.iter().cloned(), capture_begin..capture_begin+88200),
				midi_events_in_range(to_dummy_midi_event(dev.committed.iter().cloned()), playback_begin+88200..playback_begin+2*88200)
			);
		}
	);
}

#[tokio::test]
async fn midi_takes_capture_held_notes() {
	let driver = DummyDriver::new(0, 0, 44100);
	let (mut frontend, _) = launch(driver.clone(), 1000);
	frontend.set_loop_length(10000,4).unwrap();
	let dev_id = frontend.add_mididevice("dev").unwrap();
	
	{
		let d = driver.lock();
		let mut dev = d.midi_devices.get("dev").unwrap().lock().unwrap();
		dev.incoming_events.push(DummyMidiEvent {
			data: smallvec![0x90, 42, 92],
			time: 1337
		});
		dev.incoming_events.push(DummyMidiEvent {
			data: smallvec![0x80, 42, 55],
			time: 14200
		});
	}

	driver.process_for(5000, 128); // not capturing, but a note has been played
	let take_id = frontend.add_miditake(dev_id, true).unwrap();
	frontend.finish_miditake(dev_id, take_id, 10000).unwrap();
	driver.process_for(25000, 128); // capture will start, complete and the first iteration will be played back

	let d = driver.lock();
	let dev = d.midi_devices.get("dev").unwrap().lock().unwrap();
	assert_eq!(dev.committed, vec![
		MidiMessage {
			timestamp: 20000,
			data: [0x90, 42, 92],
			datalen: 3
		},
		MidiMessage {
			timestamp: 24200,
			data: [0x80, 42, 55],
			datalen: 3
		},
	]);
}

#[tokio::test]
async fn latency_compensation() {
	let playback_latency = 64;
	let capture_latency = 128;
	let driver = DummyDriver::new(playback_latency as u32, capture_latency as u32, 44100);
	let (mut frontend, _) = launch(driver.clone(), 1000);
	frontend.set_loop_length(44100,4).unwrap();
	let audiodev_id = frontend.add_device("audiodev", 2).unwrap();
	let mididev_id = frontend.add_mididevice("mididev").unwrap();
	fill_audio_device(&driver, "audiodev", 44100*8);
	{ // fill midi device
		let d = driver.lock();
		let mut dev = d.midi_devices.get("mididev").unwrap().lock().unwrap();
		dev.incoming_events.push(DummyMidiEvent {
			data: smallvec![0x90, 42, 92],
			time: 1000
		});
		dev.incoming_events.push(DummyMidiEvent {
			data: smallvec![0x80, 42, 55],
			time: 2000
		});
	}

	let audiotake_id = frontend.add_audiotake(audiodev_id, true).unwrap();
	let miditake_id = frontend.add_miditake(mididev_id, true).unwrap();
	frontend.finish_audiotake(audiodev_id, audiotake_id, 44100).unwrap();
	frontend.finish_miditake(mididev_id, miditake_id, 44100).unwrap();
	driver.process_for(3*44100, 128);
		
	let d = driver.lock();
	{ // check audio
		let dev = d.audio_devices.get("audiodev").unwrap().lock().unwrap();
		let begin = 44100-playback_latency;
		assert_sleq!(dev.playback_buffers[0][0..begin], 0.0,
			"expected silence at the beginning");
		assert_sleq!(dev.playback_buffers[0][begin..begin+44100], dev.capture_buffers[0][capture_latency..44100 + capture_latency],
			"first repetition was not played correctly");
		assert_sleq!(dev.playback_buffers[0][begin+44100..begin+2*44100], dev.capture_buffers[0][capture_latency..44100 + capture_latency],
			"second repetition was not played correctly");
	}
	{ // check midi
		let dev = d.midi_devices.get("mididev").unwrap().lock().unwrap();
		assert_eq!(dev.committed[0..2], vec![
			MidiMessage {
				timestamp: 44100 + 1000 - (playback_latency + capture_latency) as u32,
				data: [0x90, 42, 92],
				datalen: 3
			},
			MidiMessage {
				timestamp: 44100 + 2000 - (playback_latency + capture_latency) as u32,
				data: [0x80, 42, 55],
				datalen: 3
			},
		]);
	}
}

macro_rules! mute_test {
	($add_take:ident, $finish_take:ident, $set_unmuted:ident, setup_device: $setup_device:expr) => {{
		let driver = DummyDriver::new(0, 0, 44100);
		let (mut frontend, _) = launch(driver.clone(), 1000);
		frontend.set_loop_length(44100,4).unwrap();
		let dev_id = $setup_device(&mut frontend, &driver);

		driver.process_for(22050, 128); // not capturing
		let take_id = frontend.$add_take(dev_id, false).unwrap();
		frontend.$finish_take(dev_id, take_id, 44100).unwrap();
		driver.process_for(22050, 128); // not capturing
		driver.process_for(44100, 128); // capturing

		driver.process_for(22050, 128); // playback, muted
		frontend.$set_unmuted(dev_id, take_id, true).unwrap();
		driver.process_for(44100, 128); // playback, unmuted
		frontend.$set_unmuted(dev_id, take_id, false).unwrap();
		driver.process_for(22050, 128); // playback, muted

		driver
	}}
}

#[tokio::test]
async fn audio_takes_can_be_muted_and_unmuted() {
	let driver = mute_test!(add_audiotake, finish_audiotake, set_audiotake_unmuted,
		setup_device: |frontend: &mut FrontendThreadState<DummyDriver>, driver| {
			let dev_id = frontend.add_device("dev", 2).unwrap();
			fill_audio_device(driver, "dev", 44100*8);
			dev_id
		}
	);
		
	let d = driver.lock();
	let dev = d.audio_devices.get("dev").unwrap().lock().unwrap();
	let t = 22050;
	assert_sleq!(dev.playback_buffers[0][4*t..5*t], 0.0, "expected silence when muted");
	assert_sleq!(dev.playback_buffers[0][5*t..6*t], dev.capture_buffers[0][3*t..4*t], "unmuted part of first repetition was not played correctly");
	assert_sleq!(dev.playback_buffers[0][6*t..7*t], dev.capture_buffers[0][2*t..3*t], "unmuted part of second repetition was not played correctly");
	assert_sleq!(dev.playback_buffers[0][7*t..8*t], 0.0, "expected silence when muted");
}

#[tokio::test]
async fn midi_takes_can_be_muted_and_unmuted() {
	let driver = mute_test!(add_miditake, finish_miditake, set_miditake_unmuted,
		setup_device: |frontend: &mut FrontendThreadState<DummyDriver>, driver| {
			let dev_id = frontend.add_mididevice("dev").unwrap();
			fill_midi_device(driver, "dev", 44100*8);
			dev_id
		}
	);
	
	let d = driver.lock();
	let dev = d.midi_devices.get("dev").unwrap().lock().unwrap();
	let t = 22050;
	assert_eq!(midi_events_in_range(to_dummy_midi_event(dev.committed.iter().cloned()), 4*t..5*t).count(), 0, "expected silence when muted");
	assert_iter_eq(
		midi_events_in_range(dev.incoming_events.iter().cloned(), 3*t..4*t),
		midi_events_in_range(to_dummy_midi_event(dev.committed.iter().cloned()), 5*t..6*t)
	);
	assert_iter_eq(
		midi_events_in_range(dev.incoming_events.iter().cloned(), 2*t..3*t),
		midi_events_in_range(to_dummy_midi_event(dev.committed.iter().cloned()), 6*t..7*t)
	);
	assert_eq!(midi_events_in_range(to_dummy_midi_event(dev.committed.iter().cloned()), 7*t..8*t).count(), 0, "expected silence when muted");
}
