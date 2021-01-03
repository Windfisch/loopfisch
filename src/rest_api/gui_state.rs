pub use super::data::*;
use std::sync::Arc;
use super::updates::*;
use crate::id_generator::IdGenerator;
use async_std::sync::Mutex;
use crate::engine::*;

pub struct GuiMutexedState {
	pub engine: Box<dyn FrontendTrait>,
	pub synths: Vec<Synth>,
	pub take_id: IdGenerator,
	pub chain_id: IdGenerator,
	pub synth_id: IdGenerator,
}

impl GuiMutexedState {
	pub fn find_audiotake_by_engine_id(&mut self, dev_id: usize, take_id: u32) -> Option<(u32, u32, &mut Take)> {
		for synth in self.synths.iter_mut() {
			for chain in synth.chains.iter_mut() {
				if chain.engine_audiodevice_id == dev_id {
					for take in chain.takes.iter_mut() {
						if take.engine_take_id == EngineTakeRef::Audio(take_id) {
							return Some((synth.id, chain.id, take));
						}
					}
				}
			}
		}
		return None;
	}
	pub fn find_miditake_by_engine_id(&mut self, mididev_id: usize, take_id: u32) -> Option<(u32, u32, &mut Take)> {
		for synth in self.synths.iter_mut() {
			if synth.engine_mididevice_id == mididev_id {
				for chain in synth.chains.iter_mut() {
					for take in chain.takes.iter_mut() {
						if take.engine_take_id == EngineTakeRef::Midi(take_id) {
							return Some((synth.id, chain.id, take));
						}
					}
				}
			}
		}
		return None;
	}
}

pub struct GuiState {
	pub update_list: Arc<UpdateList>,
	pub mutex: Mutex<GuiMutexedState>,
}
