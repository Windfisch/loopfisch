pub use super::data::*;
use std::sync::Arc;
use super::updates::*;
use crate::id_generator::IdGenerator;
use async_std::sync::Mutex;
use crate::engine::*;

pub struct GuiMutexedState {
	pub engine: FrontendThreadState,
	pub synths: Vec<Synth>,
	pub take_id: IdGenerator,
	pub chain_id: IdGenerator,
	pub synth_id: IdGenerator,
}

pub struct GuiState {
	pub update_list: Arc<UpdateList>,
	pub mutex: Mutex<GuiMutexedState>,
}
