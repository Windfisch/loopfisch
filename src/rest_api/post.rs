use rocket_contrib::json::Json;
use super::gui_state::*;
use rocket::State;
use rocket::http::Status;
use serde::Deserialize;
use super::updates::*;
use super::util::gen_unique_name;


#[derive(Deserialize,Clone,PartialEq)]
pub enum TakeType {
	Audio,
	Midi
}

#[derive(Deserialize,Clone)]
pub struct TakePost {
	name: Option<String>,
	r#type: TakeType,
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

#[post("/synths/<synthid>/restart_transport")]
pub async fn post_restart_transport(state: State<'_, std::sync::Arc<GuiState>>, synthid: u32) -> Result<rocket::response::status::Accepted<()>, Status> {
	let mut guard_ = state.mutex.lock().await;
	let guard = &mut *guard_;
	if let Some(synth) = guard.synths.iter_mut().find(|s| s.id == synthid) {
		guard.engine.restart_midi_transport(synth.engine_mididevice_id)
			.map_err(|_| Status::InternalServerError)?;
		return Ok(rocket::response::status::Accepted(None));
	}
	Err(Status::NotFound)
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
				midi: true, // FIXME this should not be hard-coded,
				echo: false,
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
pub async fn post_take(state: State<'_, std::sync::Arc<GuiState>>, synthid: u32, chainid: u32, data: Json<TakePost>) -> Result<rocket::response::status::Created<()>, Status> {
	let mut guard_ = state.mutex.lock().await;
	let guard = &mut *guard_;
	if let Some(synth) = guard.synths.iter_mut().find(|s| s.id == synthid) {
		if let Some(chain) = synth.chains.iter_mut().find(|c| c.id == chainid) {
			// FIXME this is racy! there should be an atomic function for adding multiple takes at once!
			// FIXME and the unwrap... there is so much wrong with this.
			let name = gen_unique_name(data.name.as_deref().unwrap_or("Take"), chain.takes.iter().map(|c|&c.name[..]));

			// set up the MIDI take
			let midi_id = guard.take_id.gen();
			let engine_miditake_id = guard.engine.add_miditake(synth.engine_mididevice_id, true).unwrap();

			chain.takes.push( Take {
				id: midi_id,
				engine_take_id: EngineTakeRef::Midi(engine_miditake_id),
				name: name.clone(),
				muted: false,
				muted_scheduled: false,
				state: RecordingState::Waiting,
				playing_since: None,
				duration: None,
				associated_midi_takes: Vec::new(),
			});
			state.update_list.push(make_update_take(chain.takes.last().unwrap(), synthid, chainid)).await;

			let result_take_id;
			// set up the audio take, if requested.
			if data.r#type == TakeType::Audio {
				let audio_id = guard.take_id.gen();
				let engine_audiotake_id = guard.engine.add_audiotake(chain.engine_audiodevice_id, false).unwrap();

				let mut associated_midi_takes: Vec<u32> =
					chain.takes.iter()
						.filter( |t| t.is_midi() && t.is_audible() )
						.map(|t| t.id)
						.collect();
				associated_midi_takes.push(midi_id);

				chain.takes.push( Take {
					id: audio_id,
					engine_take_id: EngineTakeRef::Audio(engine_audiotake_id),
					name,
					muted: true,
					muted_scheduled: false,
					playing_since: None,
					duration: None,
					state: RecordingState::Waiting,
					associated_midi_takes
				});
				state.update_list.push(make_update_take(chain.takes.last().unwrap(), synthid, chainid)).await;
				result_take_id = audio_id;
			}
			else {
				result_take_id = midi_id;
			}

			return Ok(rocket::response::status::Created::new(format!("/api/synths/{}/chains/{}/takes/{}", synthid, chainid, result_take_id)));
		}
	}
	Err(Status::NotFound)
}

fn div_ceil(a: u32, b: u32) -> u32 { (a+b-1)/b }

#[post("/synths/<synthid>/chains/<chainid>/takes/<takeid>/finish_recording")]
pub async fn post_take_finish_recording(state: State<'_, std::sync::Arc<GuiState>>, synthid: u32, chainid: u32, takeid: u32) -> Result<rocket::response::status::Accepted::<()>, Status> {
	let mut guard_ = state.mutex.lock().await;
	let guard = &mut *guard_;
	if let Some(synth) = guard.synths.iter_mut().find(|s| s.id == synthid) {
		if let Some(chain) = synth.chains.iter_mut().find(|c| c.id == chainid) {
			if let Some(take) = chain.takes.iter_mut().find(|s| s.id == takeid) {
				if let RecordingState::Recording(started) = take.state {
					// TODO: check whether the take has already been finished
					let now = guard.engine.transport_position();
					let current_duration = now - started;
					let loop_length = guard.engine.loop_length();
					let target_duration = div_ceil(current_duration - std::cmp::min(loop_length/4, current_duration-1), loop_length) * loop_length;
					println!("rounding take duration {} to {} (base loop length is {})", current_duration, target_duration, loop_length);

					match take.engine_take_id {
						EngineTakeRef::Audio(id) => guard.engine.finish_audiotake(chain.engine_audiodevice_id, id, target_duration).map_err(|_| Status::InternalServerError)?,
						EngineTakeRef::Midi(id) => guard.engine.finish_miditake(synth.engine_mididevice_id, id, target_duration).map_err(|_| Status::InternalServerError)?
					};

					// TODO set the take as finished
					return Ok(rocket::response::status::Accepted(None));
				}
			}
		}
	}
	Err(Status::NotFound)
}
