use super::data::*;
use rocket_contrib::json::Json;
use super::gui_state::*;
use rocket::State;
use rocket::http::Status;
use serde::Deserialize;
use super::updates::*;

#[derive(Deserialize,Clone)]
pub struct SynthPatch {
	id: u32,
	name: Option<String>,
	chains: Option<Vec<ChainPatch>>
}

#[derive(Deserialize,Clone)]
pub struct ChainPatch {
	id: u32,
	name: Option<String>,
	takes: Option<Vec<TakePatch>>
}

#[derive(Deserialize,Clone)]
pub struct TakePatch {
	id: u32,
	name: Option<String>,
	muted: Option<bool>,
	muted_scheduled: Option<bool>,
	associated_midi_takes: Option<Vec<u32>>,
}


#[patch("/synths", data="<patch>")]
pub async fn patch_synths(state: State<'_, GuiState>, patch: Json<Vec<SynthPatch>>) -> Result<(), Status> {
	let mut guard = state.mutex.lock().await;
	patch_synths_(&mut guard.synths, &*patch, true)?;
	patch_synths_(&mut guard.synths, &*patch, false).unwrap();
	for p in patch.iter() {
		state.update_list.push(make_update_synth(guard.synths.iter().find(|s| s.id == p.id).unwrap())).await;
	}
	Ok(())
}

#[patch("/synths/<id>", data="<patch>")]
pub async fn patch_synth(state: State<'_, GuiState>, id: u32, patch: Json<SynthPatch>) -> Result<(), Status> {
	if id != patch.id {
		return Err(Status::UnprocessableEntity); //422
	}
	let mut guard = state.mutex.lock().await;
	patch_synth_(&mut guard.synths, &*patch, true)?;
	patch_synth_(&mut guard.synths, &*patch, false).unwrap();
	state.update_list.push(make_update_synth(guard.synths.iter().find(|s| s.id == patch.id).unwrap())).await;
	Ok(())
}

#[patch("/synths/<synthid>/chains", data="<patch>")]
pub async fn patch_chains(state: State<'_, GuiState>, synthid: u32, patch: Json<Vec<ChainPatch>>) -> Result<(), Status> {
	let mut guard = state.mutex.lock().await;
	if let Some(synth) = guard.synths.iter_mut().find(|s| s.id == synthid) {
		patch_chains_(&mut synth.chains, &*patch, true)?;
		patch_chains_(&mut synth.chains, &*patch, false).unwrap();
		for p in patch.iter() {
			state.update_list.push(make_update_chain(synth.chains.iter().find(|s| s.id == p.id).unwrap(), synthid)).await;
		}
		return Ok(());
	}
	Err(Status::NotFound)
}

#[patch("/synths/<synthid>/chains/<chainid>", data="<patch>")]
pub async fn patch_chain(state: State<'_, GuiState>, synthid: u32, chainid: u32, patch: Json<ChainPatch>) -> Result<(), Status> {
	let mut guard = state.mutex.lock().await;
	if let Some(synth) = guard.synths.iter_mut().find(|s| s.id == synthid) {
		if chainid != patch.id {
			return Err(Status::UnprocessableEntity);
		}
		patch_chain_(&mut synth.chains, &*patch, true)?;
		patch_chain_(&mut synth.chains, &*patch, false).unwrap();
		state.update_list.push(make_update_chain(synth.chains.iter().find(|s| s.id == patch.id).unwrap(), synthid)).await;
		return Ok(());
	}
	Err(Status::NotFound)
}

#[patch("/synths/<synthid>/chains/<chainid>/takes", data="<patch>")]
pub async fn patch_takes(state: State<'_, GuiState>, synthid: u32, chainid: u32, patch: Json<Vec<TakePatch>>) -> Result<(), Status> {
	let mut guard = state.mutex.lock().await;
	if let Some(synth) = guard.synths.iter_mut().find(|s| s.id == synthid) {
		if let Some(chain) = synth.chains.iter_mut().find(|c| c.id == chainid) {
			patch_takes_(&mut chain.takes, &*patch, true)?;
			patch_takes_(&mut chain.takes, &*patch, false).unwrap();
			for _p in patch.iter() {
				// state.update_list.push(make_update_take(chain.takes.iter().find(|s| s.id == p.id).unwrap(), synthid, chainid)).await; TODO
			}
			return Ok(());
		}
	}
	Err(Status::NotFound)
}

#[patch("/synths/<synthid>/chains/<chainid>/takes/<takeid>", data="<patch>")]
pub async fn patch_take(state: State<'_, GuiState>, synthid: u32, chainid: u32, takeid: u32, patch: Json<TakePatch>) -> Result<(), Status> {
	let mut guard = state.mutex.lock().await;
	if let Some(synth) = guard.synths.iter_mut().find(|s| s.id == synthid) {
		if let Some(chain) = synth.chains.iter_mut().find(|s| s.id == chainid) {
			if takeid != patch.id {
				return Err(Status::UnprocessableEntity);
			}
			patch_take_(&mut chain.takes, &*patch, true)?;
			patch_take_(&mut chain.takes, &*patch, false).unwrap();
			// state.update_list.push(make_update_take(chain.takes.iter().find(|s| s.id == patch.id).unwrap(), synthid, chainid)).await; TODO
			return Ok(());
		}
	}
	Err(Status::NotFound)
}

fn patch_synths_(synths: &mut Vec<Synth>, patch: &Vec<SynthPatch>, check: bool) -> Result<(), Status> {
	for synth in patch.iter() {
		patch_synth_(synths, synth, check)?;
	}
	Ok(())
}

fn patch_synth_(synths: &mut Vec<Synth>, patch: &SynthPatch, check: bool) -> Result<(), Status> {
	if let Some(synth_to_patch) = synths.iter_mut().find(|s| s.id == patch.id) {
		if let Some(chains) = &patch.chains {
			patch_chains_(&mut synth_to_patch.chains, chains, check)?;
		}
		if !check {
			if let Some(name) = &patch.name {
				synth_to_patch.name = name.clone();
			}
		}

		Ok(())
	}
	else {
		Err(Status::UnprocessableEntity) // 422
	}
}

fn patch_chains_(chains: &mut Vec<Chain>, patch: &Vec<ChainPatch>, check: bool) -> Result<(), Status> {
	for chain in patch.iter() {
		patch_chain_(chains, chain, check)?;
	}
	Ok(())
}

fn patch_chain_(chains: &mut Vec<Chain>, patch: &ChainPatch, check: bool) -> Result<(), Status> {
	if let Some(chain_to_patch) = chains.iter_mut().find(|s| s.id == patch.id) {
		if let Some(takes) = &patch.takes {
			patch_takes_(&mut chain_to_patch.takes, takes, check)?;
		}
		if !check {
			if let Some(name) = &patch.name {
				chain_to_patch.name = name.clone();
			}
		}

		Ok(())
	}
	else {
		Err(Status::UnprocessableEntity)
	}
}

fn patch_takes_(takes: &mut Vec<Take>, patch: &Vec<TakePatch>, check: bool) -> Result<(), Status> {
	for take in patch.iter() {
		patch_take_(takes, take, check)?;
	}
	Ok(())
}

fn patch_take_(takes: &mut Vec<Take>, patch: &TakePatch, check: bool) -> Result<(), Status> {
	if let Some(take_to_patch) = takes.iter_mut().find(|s| s.id == patch.id) {
		if !check {
			if let Some(name) = &patch.name {
				take_to_patch.name = name.clone();
			}
			if let Some(muted) = patch.muted {
				// TODO: mute take immediately, communicate with the engine.
				take_to_patch.muted = muted;
			}
			if let Some(muted_scheduled) = patch.muted_scheduled {
				// TODO: schedule mute, communicate with the engine.
				take_to_patch.muted_scheduled = muted_scheduled;
			}
		}

		Ok(())
	}
	else {
		Err(Status::UnprocessableEntity)
	}
}
