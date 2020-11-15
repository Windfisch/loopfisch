pub enum Event {
	AudioTakeStateChanged(usize, u32, RecordState),
	MidiTakeStateChanged(usize, u32, RecordState),
	Timestamp(u32, u32),
	Kill
}

#[derive(std::cmp::PartialEq, Debug)]
pub enum RecordState {
	Waiting,
	Recording,
	Finished
}
