use jack;
use std::cmp::{min,max};
use lockfree::channel;

use assert_no_alloc::assert_no_alloc;

#[cfg(debug_assertions)]
#[global_allocator]
static A: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

struct CircularBuffer<T> {
	data: Vec<T>,
	next_idx: usize,
}

/*impl<T> CircularBuffer<T> {
	const BUF_SECONDS: u32= 10;
	pub fn new(sample_rate: u32) -> CircularBuffer<T> {
		CircularBuffer {
			data: Vec::<T>::with_capacity((sample_rate*CircularBuffer::<T>::BUF_SECONDS) as usize),
			next_idx: 0
		}
	}

	pub fn push(&self, newdata: &[f32]) {
		let space_until_end = self.data.len() - self.next_idx;
		let n_until_end = min(space_until_end, newdata.len());
	}
}*/

struct Take {
	samples: Vec<Vec<f32>>,
}

struct AudioChannel {
	in_port: jack::Port<jack::AudioIn>,
	out_port: jack::Port<jack::AudioOut>,
}

impl AudioChannel {
	fn new(client: &jack::Client, name: &str, num: u32) -> Result<AudioChannel, jack::Error> {
		return Ok( AudioChannel {
			in_port: client.register_port(&format!("{}_in{}", name, num), jack::AudioIn::default())?,
			out_port: client.register_port(&format!("{}_out{}", name, num), jack::AudioOut::default())?
		});
	}
}

struct AudioDevice {
	pub channels: Vec<AudioChannel>
}

impl AudioDevice {
	pub fn new(client: &jack::Client, n_channels: u32, name: &str) -> Result<AudioDevice, jack::Error> {
		let dev = AudioDevice {
			channels: (0..n_channels).map(|channel| AudioChannel::new(client, name, channel+1)).collect::<Result<_,_>>()?
		};
		Ok(dev)
	}
}

#[derive(std::cmp::PartialEq)]
enum RecordState {
	Waiting,
	Recording,
	Finished
}

struct ArmedTake {
	take: Take,
	dev_id: usize,
	state: RecordState
}

struct PlayableTake {
	take: Take,
	dev_id: usize,
	active: bool
}

impl ArmedTake {
	fn record(&mut self, client: &jack::Client, scope: &jack::ProcessScope, device: &AudioDevice, range: std::ops::Range<usize>) {
		for (channel_buffer, channel_ports) in self.take.samples.iter_mut().zip(device.channels.iter()) {
			channel_buffer.extend_from_slice( &channel_ports.in_port.as_slice(scope)[range.clone()] );
		}
	}
}

struct AudioThreadState {
	devices: Vec<AudioDevice>,
	armed_takes: Vec<ArmedTake>,
	playable_takes: Vec<PlayableTake>,
	new_take_channel: lockfree::channel::spsc::Receiver<ArmedTake>,
	song_position: u32,
	song_length: u32,
}

struct FrontendThreadState {
	new_take_channel: lockfree::channel::spsc::Sender<ArmedTake>
}

fn create_thread_states(devices: Vec<AudioDevice>, song_length: u32) -> (AudioThreadState, FrontendThreadState) {
	let (take_sender, take_receiver) = lockfree::channel::spsc::create();
	
	let audio_thread_state = AudioThreadState {
		devices,
		armed_takes: Vec::new(), // TODO should have a capacity
		playable_takes: Vec::new(), // TODO should have a capacity
		new_take_channel: take_receiver,
		song_position: 0,
		song_length
	};

	let frontend_thread_state = FrontendThreadState {
		new_take_channel: take_sender,
	};

	return (audio_thread_state, frontend_thread_state);
}

struct RemoveUnorderedIter<'a,T,F: FnMut(&mut T) -> bool> {
	vec: &'a mut Vec<T>,
	i: usize,
	filter: F
}

impl<T,F: FnMut(&mut T) -> bool> Iterator for RemoveUnorderedIter<'_,T,F> {
	type Item = T;

	fn next(&mut self) -> Option<T> {
		while self.i < self.vec.len() {
			if (self.filter)(&mut self.vec[self.i]) {
				let item = self.vec.swap_remove(self.i);
				return Some(item);
			}
			else {
				self.i += 1;
			}
		}
		return None;
	}
}

fn remove_unordered_iter<T,F: FnMut(&mut T) -> bool>(vec: &mut Vec<T>, filter: F) -> RemoveUnorderedIter<T,F> {
	RemoveUnorderedIter {
		vec,
		i: 0,
		filter
	}
}

impl AudioThreadState {
	fn process_callback(&mut self, client: &jack::Client, scope: &jack::ProcessScope) -> jack::Control {
		use lockfree::channel::spsc::*;
		use RecordState::*;

		assert!(scope.n_frames() < self.song_length);

		let song_position_after = self.song_position + scope.n_frames();
		let song_wraps = self.song_length < song_position_after;
		let song_wraps_at = min(self.song_length - self.song_position, scope.n_frames()) as usize;

		// first, handle the take channel
		loop {
			match self.new_take_channel.recv() {
				Ok(armed_take) => { self.armed_takes.push(armed_take); }
				Err(RecvErr::NoMessage) | Err(RecvErr::NoSender) => { break; }
			}
		}

		// then, handle all armed takes and record into them
		for t in self.armed_takes.iter_mut() {
			if t.state == Recording {
				t.record(client,scope, &self.devices[t.dev_id], 0..song_wraps_at);

				if song_wraps {
					println!("Finished recording on device {}", t.dev_id);
					t.state = Finished;
				}
			}
			else if t.state == Waiting {
				if song_wraps {
					println!("Started recording on device {}", t.dev_id);
					t.state = Recording;
					t.record(client,scope, &self.devices[t.dev_id], song_wraps_at..scope.n_frames() as usize);
				}
			}
		}

		// remove all finished takes from armed_takes
		for take in remove_unordered_iter(&mut self.armed_takes, |t| t.state != Finished) {
			self.playable_takes.push(PlayableTake{
				take: take.take,
				dev_id: take.dev_id,
				active: true
			});
		}

		jack::Control::Continue
	}
}


struct Notifications;
impl jack::NotificationHandler for Notifications {
	fn thread_init(&self, _: &jack::Client) {
		println!("JACK: thread init");
	}

	fn shutdown(&mut self, status: jack::ClientStatus, reason: &str) {
		println!(
				"JACK: shutdown with status {:?} because \"{}\"",
				status, reason
				);
	}
}

fn main() {
    println!("Hello, world!");

	let (client, _status) = jack::Client::new("loopfisch", jack::ClientOptions::NO_START_SERVER).unwrap();

	println!("JACK running with sampling rate {} Hz, buffer size = {} samples", client.sample_rate(), client.buffer_size());

	let audiodev = AudioDevice::new(&client, 2, "fnord").unwrap();
	let devices = vec![audiodev];
	
	let (mut audio_thread_state, mut frontend_thread_state) = create_thread_states(devices,42);

	let process_callback = move |client: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
		audio_thread_state.process_callback(client, ps)
	};
	let process = jack::ClosureProcessHandler::new(process_callback);
	let active_client = client.activate_async(Notifications, process).unwrap();

	loop {}
}
