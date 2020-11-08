pub enum Event {
	AudioTakeStateChanged(usize, u32, RecordState),
	MidiTakeStateChanged(usize, u32, RecordState),
	Kill
}

#[derive(std::cmp::PartialEq, Debug)]
pub enum RecordState {
	Waiting,
	Recording,
	Finished
}
