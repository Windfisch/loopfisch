use serde::Serialize;
use std::time::Duration;
use rocket::State;
use rocket_contrib::json::Json;
use super::gui_state::GuiState;
use super::data::{Synth,Chain,Take,RecordingState,EngineTakeRef};

#[derive(Serialize, Clone)]
pub struct Update {
	pub id: u64,
	pub action: UpdateRoot
}

#[derive(Serialize, Clone)]
pub struct UpdateSong {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub song_position: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub transport_position: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub loop_length: Option<f32>
}

#[derive(Serialize, Clone)]
pub struct UpdateRoot {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub synths: Option<Vec<UpdateSynth>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub song: Option<UpdateSong>
}

#[derive(Serialize, Clone, Default)]
pub struct UpdateSynth {
	pub id: u32,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub chains: Option<Vec<UpdateChain>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub deleted: Option<bool>
}

#[derive(Serialize, Clone, Default)]
pub struct UpdateChain {
	pub id: u32,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub midi: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub echo: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub takes: Option<Vec<UpdateTake>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub deleted: Option<bool>
}

#[derive(Serialize, Clone, Default)]
pub struct UpdateTake {
	pub id: u32,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,
	#[serde(rename="type", skip_serializing_if = "Option::is_none")]
	pub engine_take_id: Option<EngineTakeRef>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub state: Option<RecordingState>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub muted: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub muted_scheduled: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub associated_midi_takes: Option<Vec<u32>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub playing_since: Option<Option<f64>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub duration: Option<Option<f64>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub deleted: Option<bool>
}

pub fn make_update_synth(synth: &Synth) -> UpdateRoot {
	UpdateRoot {
		synths: Some(vec![UpdateSynth {
			id: synth.id,
			name: Some(synth.name.clone()),
			..Default::default()
		}]),
		song: None
	}
}

pub fn make_update_chain(chain: &Chain, synthid: u32) -> UpdateRoot {
	UpdateRoot {
		synths: Some(vec![UpdateSynth {
			id: synthid,
			chains: Some(vec![UpdateChain {
				id: chain.id,
				name: Some(chain.name.clone()),
				midi: Some(chain.midi),
				echo: Some(chain.echo),
				..Default::default()
			}]),
			..Default::default()
		}]),
		song: None
	}
}

pub fn make_update_take(take: &Take, synthid: u32, chainid: u32) -> UpdateRoot {
	UpdateRoot {
		synths: Some(vec![UpdateSynth {
			id: synthid,
			chains: Some(vec![UpdateChain {
				id: chainid,
				takes: Some(vec![UpdateTake {
					id: take.id,
					name: Some(take.name.clone()),
					engine_take_id: Some(take.engine_take_id.clone()),
					state: Some(take.state.clone()),
					muted: Some(take.muted),
					muted_scheduled: Some(take.muted_scheduled),
					associated_midi_takes: Some(take.associated_midi_takes.clone()),
					playing_since: Some(take.playing_since),
					duration: Some(take.duration),
					..Default::default()
				}]),
				..Default::default()
			}]),
			..Default::default()
		}]),
		song: None
	}
}

pub struct UpdateList {
	condvar: async_std::sync::Condvar,
	updates: async_std::sync::Mutex< (u64, std::collections::VecDeque<Update>) >
}

impl UpdateList {
	pub fn new() -> UpdateList {
		return UpdateList {
			condvar: async_std::sync::Condvar::new(),
			updates: async_std::sync::Mutex::new( (0, std::collections::VecDeque::new()) )
		};
	}

	pub async fn push(&self, action: UpdateRoot) {
		let mut guard = self.updates.lock().await;
		let id = guard.0;
		guard.1.push_back( Update{ id, action} );
		guard.0 += 1;
		self.condvar.notify_all();
	}

	pub async fn poll(&self, timeout: Duration, since: u64) -> Vec<Update>
	{
		let (lock_guard, _result) = self.condvar.wait_timeout_until(
			self.updates.lock().await,
			timeout,
			|updates| updates.0 > since
		).await;

		lock_guard.1.iter().filter(|x| x.id >= since).cloned().collect()
	}
}

#[get("/updates?<since>&<seconds>")]
pub async fn updates(state: State<'_, std::sync::Arc<GuiState>>, since: u64, seconds: u64) -> Json<Vec<Update>> {
	Json(state.update_list.poll(Duration::from_secs(seconds), since).await)
}
