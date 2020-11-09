use rocket_contrib::json::Json;
use super::gui_state::*;
use rocket::State;
use rocket::http::Status;
use serde::Deserialize;
use super::updates::*;
use super::util::gen_unique_name;

#[derive(Deserialize,Clone)]
pub struct TakePost {
	name: Option<String>,
}

#[derive(Deserialize,Clone)]
pub struct ChainPost {
	name: String,
}

#[derive(Deserialize,Clone)]
pub struct SynthPost {
	name: String,
}

#[post("/synths", data="<data>")]
pub async fn post_synth(state: State<'_, std::sync::Arc<GuiState>>, data: Json<SynthPost>) -> Result<rocket::response::status::Created<()>, Status> {
	let mut guard_ = state.mutex.lock().await;
	let guard = &mut *guard_;
	let id = guard.synth_id.gen();

	let name = gen_unique_name(&data.name, guard.synths.iter().map(|c|&c.name[..]));

	if let Ok(engine_mididevice_id) = guard.engine.add_mididevice(&name) {
		let new_synth = Synth {
			id,
			chains: Vec::new(),
			name,
			engine_mididevice_id
		};
		state.update_list.push(make_update_synth(&new_synth)).await;
		guard.synths.push(new_synth);

		return Ok(rocket::response::status::Created::new(format!("/api/synths/{}", id)));
	}
	else {
		return Err(Status::InternalServerError);
	}
}

#[post("/synths/<synthid>/chains", data="<data>")]
pub async fn post_chain(state: State<'_, std::sync::Arc<GuiState>>, synthid: u32, data: Json<ChainPost>) -> Result<rocket::response::status::Created<()>, Status> {
	let mut guard_ = state.mutex.lock().await;
	let guard = &mut *guard_;
	if let Some(synth) = guard.synths.iter_mut().find(|s| s.id == synthid) {
		let id = guard.chain_id.gen();
		let name = gen_unique_name(&(synth.name.clone() + "_" + &data.name), synth.chains.iter().map(|c|&c.name[..]));

		if let Ok(engine_audiodevice_id) = guard.engine.add_device(&name, 2) {
			let new_chain = Chain {
				id,
				takes: Vec::new(),
				name,
				engine_audiodevice_id
			};
			state.update_list.push(make_update_chain(&new_chain, synthid)).await;
			synth.chains.push(new_chain);

			return Ok(rocket::response::status::Created::new(format!("/api/synths/{}/chains/{}", synthid, id)));
		}
		else {
			return Err(Status::InternalServerError);
		}
	}
	Err(Status::NotFound)
}

#[post("/synths/<synthid>/chains/<chainid>/takes", data="<data>")]
pub async fn post_take(state: State<'_, std::sync::Arc<GuiState>>, synthid: u32, chainid: u32, data: Json<TakePost>) -> Result<(), Status> {
	let mut guard_ = state.mutex.lock().await;
	let guard = &mut *guard_;
	if let Some(synth) = guard.synths.iter_mut().find(|s| s.id == synthid) {
		if let Some(chain) = synth.chains.iter_mut().find(|c| c.id == chainid) {
			let audio_id = guard.take_id.gen();
			let midi_id = guard.take_id.gen();
			let name = gen_unique_name(data.name.as_deref().unwrap_or("Take"), chain.takes.iter().map(|c|&c.name[..]));

			let mut associated_midi_takes: Vec<u32> =
				chain.takes.iter()
					.filter( |t| t.is_midi() && !t.is_audible() )
					.map(|t| t.id)
					.collect();
			associated_midi_takes.push(midi_id);

			// FIXME this is racy! there should be an atomic function for adding multiple takes at once!
			// FIXME and the unwrap... there is so much wrong with this.
			let engine_miditake_id = guard.engine.add_miditake(synth.engine_mididevice_id, true).unwrap();
			let engine_audiotake_id = guard.engine.add_audiotake(chain.engine_audiodevice_id, false).unwrap();

			chain.takes.push( Take {
				id: audio_id,
				engine_take_id: EngineTakeRef::Audio(engine_audiotake_id),
				name: name.clone(),
				muted: true,
				muted_scheduled: false,
				state: RecordingState::Waiting,
				associated_midi_takes
			});
			state.update_list.push(make_update_take(chain.takes.last().unwrap(), synthid, chainid)).await;
			chain.takes.push( Take {
				id: midi_id,
				engine_take_id: EngineTakeRef::Midi(engine_miditake_id),
				name,
				muted: false,
				muted_scheduled: false,
				state: RecordingState::Waiting,
				associated_midi_takes: Vec::new()
			});
			state.update_list.push(make_update_take(chain.takes.last().unwrap(), synthid, chainid)).await;
			return Ok(());
		}
	}
	Err(Status::NotFound)
}
