pub enum Event {
	AudioTakeStateChanged(usize, u32, RecordState, u32),
	MidiTakeStateChanged(usize, u32, RecordState, u32 /* timestamp */),
	Timestamp(u32, u32),
	Kill
}

#[derive(std::cmp::PartialEq, Debug)]
pub enum RecordState {
	Waiting,
	Recording,
	Finished
}
