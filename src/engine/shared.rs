use std::sync::atomic::*;

pub struct SharedThreadState {
	pub song_length: AtomicU32,
	pub song_position: AtomicU32,
	pub transport_position: AtomicU32,
}

