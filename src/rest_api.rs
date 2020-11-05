#![feature(proc_macro_hygiene)]

use crate::engine::*;

use rocket_contrib::json;

use rocket::State;
//use std::sync::Mutex;
use async_std::sync::Mutex;
use std::time::{Duration,Instant};
use std::sync::Arc;
use async_std;
use rocket::http::Method;
use std::path::PathBuf;
use crate::id_generator::IdGenerator;


use rocket::{Request, Response};
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::{Status, Header, ContentType};
use std::io::Cursor;

use serde::{Serialize,Deserialize};

pub struct CORS();


#[rocket::async_trait]
impl Fairing for CORS {
	fn info(&self) -> Info {
		Info {
			name: "Add CORS headers to requests",
			kind: Kind::Response
		}
	}

	async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
		response.set_header(Header::new("Access-Control-Allow-Origin", "http://localhost:8080"));
		response.set_header(Header::new("Access-Control-Allow-Methods", "POST, GET, OPTIONS"));
		response.set_header(Header::new("Access-Control-Allow-Headers", "Content-Type"));
		response.set_header(Header::new("Access-Control-Expose-Headers", "Location"));
		response.set_header(Header::new("Access-Control-Allow-Credentials", "true"));

		if request.method() == Method::Options {
			response.set_header(ContentType::Plain);
			response.set_sized_body(0, Cursor::new(""));
		}
	}
}


use std::collections::HashMap;

#[derive(Serialize,Clone)]
enum Action {
	UpdateSynth(u32),
	DeleteSynth(u32),
	UpdateChain(u32,u32),
	DeleteChain(u32,u32),
	UpdateTake(u32,u32,u32),
	DeleteTake(u32,u32,u32)
}

#[derive(Serialize, Clone)]
struct Update {
	id: u64,
	action: UpdateRoot
}

struct UpdateList {
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
		let (lock_guard, result) = self.condvar.wait_timeout_until(
			self.updates.lock().await,
			timeout,
			|updates| updates.0 > since
		).await;

		lock_guard.1.iter().filter(|x| x.id >= since).cloned().collect()
	}
}

#[options("/<path..>")]
fn options(path: PathBuf) {

}

fn make_update_synth(synth: &Synth) -> UpdateRoot {
	UpdateRoot {
		synths: vec![UpdateSynth {
			id: synth.id,
			name: Some(synth.name.clone()),
			chains: None,
			deleted: None
		}]
	}
}

fn make_update_chain(chain: &Chain, synthid: u32) -> UpdateRoot {
	UpdateRoot {
		synths: vec![UpdateSynth {
			id: synthid,
			name: None,
			deleted: None,
			chains: Some(vec![UpdateChain {
				id: chain.id,
				name: Some(chain.name.clone()),
				takes: None,
				deleted: None
			}])
		}]
	}
}

#[derive(Serialize, Clone)]
struct UpdateRoot {
	synths: Vec<UpdateSynth>
}

#[derive(Serialize, Clone)]
struct UpdateSynth {
	id: u32,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	chains: Option<Vec<UpdateChain>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	deleted: Option<bool>
}

#[derive(Serialize, Clone)]
struct UpdateChain {
	id: u32,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	takes: Option<Vec<UpdateTake>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	deleted: Option<bool>
}

#[derive(Serialize, Clone)]
struct UpdateTake {
	id: u32,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	muted: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	muted_scheduled: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	associated_midi_takes: Option<Vec<u32>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	deleted: Option<bool>
}

#[get("/updates?<since>&<seconds>")]
async fn updates(state: State<'_, GuiState>, since: u64, seconds: u64) -> json::Json<Vec<Update>> {
	json::Json(state.update_list.poll(Duration::from_secs(seconds), since).await)
}

use json::Json;

#[get("/synths")]
async fn synths_get(state: State<'_, GuiState>) -> Json< Vec<Synth> > {
	let lock = state.mutex.lock().await;
	Json(lock.synths.clone())
}

#[get("/synths/<num>")]
async fn synths_get_one(state: State<'_, GuiState>, num: u32) -> Option<Json<Synth> > {
	let lock = state.mutex.lock().await;
	lock.synths.iter().find(|s| s.id == num).cloned().map(|synth| Json(synth))
}

#[get("/synths/<synthnum>/chains")]
async fn chains_get(state: State<'_, GuiState>, synthnum: u32) -> Option<Json<Vec<Chain>> > {
	let lock = state.mutex.lock().await;
	lock.synths.iter().find(|s| s.id == synthnum).map(|s| Json(s.chains.clone()))
}

#[get("/synths/<synthnum>/chains/<chainnum>")]
async fn chains_get_one(state: State<'_, GuiState>, synthnum: u32, chainnum:u32) -> Option<Json<Chain> > {
	let lock = state.mutex.lock().await;
	lock.synths.iter().find(|s| s.id == synthnum).map(
		|s| s.chains.iter().find(|c| c.id == chainnum).map(
			|c| Json(c.clone())
		)
	).flatten()
}

#[get("/synths/<synthnum>/chains/<chainnum>/takes")]
async fn takes_get(state: State<'_, GuiState>, synthnum: u32, chainnum:u32) -> Option<Json<Vec<Take>> > {
	let lock = state.mutex.lock().await;
	lock.synths.iter().find(|s| s.id == synthnum).map(
		|s| s.chains.iter().find(|c| c.id == chainnum).map(
			|c| Json(c.takes.clone())
		)
	).flatten()
}

#[get("/synths/<synthnum>/chains/<chainnum>/takes/<takenum>")]
async fn takes_get_one(state: State<'_, GuiState>, synthnum: u32, chainnum:u32, takenum: u32) -> Option<Json<Take> > {
	let lock = state.mutex.lock().await;
	lock.synths.iter().find(|s| s.id == synthnum).map(
		|s| s.chains.iter().find(|c| c.id == chainnum).map(
			|c| c.takes.iter().find(|t| t.id == takenum).map(
				|t| Json(t.clone())
			)
		)
	).flatten().flatten()
}


#[derive(Deserialize,Clone)]
struct SynthPatch {
	id: u32,
	name: Option<String>,
	chains: Option<Vec<ChainPatch>>
}

#[derive(Deserialize,Clone)]
struct ChainPatch {
	id: u32,
	name: Option<String>,
	takes: Option<Vec<TakePatch>>
}

#[derive(Deserialize,Clone)]
struct ChainPost {
	name: String,
}

#[derive(Deserialize,Clone)]
struct TakePatch {
	id: u32,
	name: Option<String>,
	muted: Option<bool>,
	muted_scheduled: Option<bool>,
	associated_midi_takes: Option<Vec<u32>>,
}

#[derive(Deserialize,Clone)]
struct TakePost {
	name: Option<String>,
}



#[patch("/synths", data="<patch>")]
async fn patch_synths(state: State<'_, GuiState>, patch: Json<Vec<SynthPatch>>) -> Result<(), Status> {
	let mut guard = state.mutex.lock().await;
	patch_synths_(&mut guard.synths, &*patch, true)?;
	patch_synths_(&mut guard.synths, &*patch, false).unwrap();
	for p in patch.iter() {
		state.update_list.push(make_update_synth(guard.synths.iter().find(|s| s.id == p.id).unwrap())).await;
	}
	Ok(())
}

#[patch("/synths/<id>", data="<patch>")]
async fn patch_synth(state: State<'_, GuiState>, id: u32, patch: Json<SynthPatch>) -> Result<(), Status> {
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
async fn patch_chains(state: State<'_, GuiState>, synthid: u32, patch: Json<Vec<ChainPatch>>) -> Result<(), Status> {
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
async fn patch_chain(state: State<'_, GuiState>, synthid: u32, chainid: u32, patch: Json<ChainPatch>) -> Result<(), Status> {
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
async fn patch_takes(state: State<'_, GuiState>, synthid: u32, chainid: u32, patch: Json<Vec<TakePatch>>) -> Result<(), Status> {
	let mut guard = state.mutex.lock().await;
	if let Some(synth) = guard.synths.iter_mut().find(|s| s.id == synthid) {
		if let Some(chain) = synth.chains.iter_mut().find(|c| c.id == chainid) {
			patch_takes_(&mut chain.takes, &*patch, true)?;
			patch_takes_(&mut chain.takes, &*patch, false).unwrap();
			for p in patch.iter() {
				// state.update_list.push(make_update_take(chain.takes.iter().find(|s| s.id == p.id).unwrap(), synthid, chainid)).await; TODO
			}
			return Ok(());
		}
	}
	Err(Status::NotFound)
}

#[patch("/synths/<synthid>/chains/<chainid>/takes/<takeid>", data="<patch>")]
async fn patch_take(state: State<'_, GuiState>, synthid: u32, chainid: u32, takeid: u32, patch: Json<TakePatch>) -> Result<(), Status> {
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

fn gen_unique_name<'a,T: Iterator<Item=&'a str> + Clone>(desired_name: &str, iter: T) -> String {
	if iter.clone().find(|s| *s == desired_name).is_some() {
		let mut i = 2;
		loop {
			let name = format!("{} {}", desired_name, i);
			if iter.clone().find(|s| *s == name).is_none() {
				return name;
			}
			i+=1;
		}
	}
	else {
		return desired_name.into();
	}
}

#[post("/synths", data="<data>")]
async fn post_synth(state: State<'_, GuiState>, data: Json<ChainPost>) -> Result<rocket::response::status::Created<()>, Status> {
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
async fn post_chain(state: State<'_, GuiState>, synthid: u32, data: Json<ChainPost>) -> Result<rocket::response::status::Created<()>, Status> {
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
async fn post_take(state: State<'_, GuiState>, synthid: u32, chainid: u32, data: Json<TakePost>) -> Result<(), Status> {
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
			let engine_audiotake_id = guard.engine.add_take(chain.engine_audiodevice_id, false).unwrap();

			chain.takes.push( Take {
				id: audio_id,
				engine_take_id: EngineTakeRef::Audio(engine_audiotake_id),
				name: name.clone(),
				muted: true,
				muted_scheduled: false,
				state: RecordingState::Waiting,
				associated_midi_takes
			});
			chain.takes.push( Take {
				id: midi_id,
				engine_take_id: EngineTakeRef::Midi(engine_miditake_id),
				name,
				muted: false,
				muted_scheduled: false,
				state: RecordingState::Waiting,
				associated_midi_takes: Vec::new()
			});
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


#[catch(404)]
fn not_found(req: &Request) -> String {
	if req.uri().segments().next() == Some("api") {
		r#"{"error": "not found"}"#.into()
	}
	else {
		r#"
            <!DOCTYPE html>
            <html lang="en">
            <head>
                <meta charset="utf-8">
                <title>404 Not Found</title>
            </head>
            <body align="center">
                <div role="main" align="center">
                    <h1>404: Not Found</h1>
                    <p>The requested resource could not be found.</p>
                    <hr />
                </div>
                <div role="contentinfo" align="center">
                    <small>Rocket</small>
                </div>
            </body>
            </html>
"#.into()
	}
}

#[derive(Serialize,Clone)]
struct Synth {
	id: u32,
	name: String,
	chains: Vec<Chain>,

	#[serde(skip)]
	engine_mididevice_id: usize
}

#[derive(Serialize,Clone)]
struct Chain {
	id: u32,
	name: String,
	takes: Vec<Take>,

	#[serde(skip)]
	engine_audiodevice_id: usize
}

#[derive(Serialize,Clone,PartialEq)]
enum RecordingState {
	Waiting,
	Recording,
	Finished
}

#[derive(Clone)]
enum EngineTakeRef {
	Audio(u32),
	Midi(u32)
}

impl Serialize for EngineTakeRef {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
		where S: serde::Serializer,
		{
			match *self {
				EngineTakeRef::Audio(_) => serializer.serialize_unit_variant("EngineTakeRef", 0, "Audio"),
				EngineTakeRef::Midi(_) => serializer.serialize_unit_variant("EngineTakeRef", 1, "Midi"),
			}
		}
}

#[derive(Serialize,Clone)]
struct Take {
	id: u32,
	name: String,
	#[serde(rename="type")]
	engine_take_id: EngineTakeRef,
	state: RecordingState,
	muted: bool,
	muted_scheduled: bool,
	associated_midi_takes: Vec<u32>,
}

impl Take {
	pub fn is_midi(&self) -> bool {
		if let EngineTakeRef::Midi(_) = self.engine_take_id {
			return true;
		}
		else {
			return false;
		}
	}

	pub fn is_audio(&self) -> bool { return !self.is_midi(); }

	pub fn is_audible(&self) -> bool {
		return self.state == RecordingState::Finished && self.muted == false;
	}
}

struct GuiMutexedState {
	engine: FrontendThreadState,
	synths: Vec<Synth>,
	take_id: IdGenerator,
	chain_id: IdGenerator,
	synth_id: IdGenerator,
}

struct GuiState {
	update_list: UpdateList,
	mutex: Mutex<GuiMutexedState>,
}

pub async fn launch_server(engine: FrontendThreadState) {
	let state = GuiState {
		update_list: UpdateList::new(),
		mutex: Mutex::new( GuiMutexedState {
			engine,
			synths: vec![
				Synth {
					id: 0,
					name: "DeepMind 13".into(),
					engine_mididevice_id: 1337, // FIXME
					chains: vec![
						Chain {
							id: 0,
							name: "Pad".into(),
							takes: vec![],
							engine_audiodevice_id: 1337, // FIXME
						}
					]
				}
			],
			take_id: IdGenerator::new(),
			chain_id: IdGenerator::new(),
			synth_id: IdGenerator::new()
		})
	};
	
	rocket::ignite()
		.manage(state)
		.mount("/api", routes![
			options,
			updates,
			synths_get, synths_get_one,
			chains_get, chains_get_one,
			takes_get, takes_get_one,
			patch_synths, patch_synth, post_synth,
			patch_chains, patch_chain, post_chain,
			patch_takes, patch_take, post_take,
		])
		.register(catchers![not_found])
		.attach(CORS())
		.launch().await.unwrap();
}
