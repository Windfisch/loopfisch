use rocket_contrib::json::Json;
use super::gui_state::*;
use rocket::State;

#[get("/synths")]
pub async fn synths_get(state: State<'_, std::sync::Arc<GuiState>>) -> Json< Vec<Synth> > {
	let lock = state.mutex.lock().await;
	Json(lock.synths.clone())
}

#[get("/synths/<num>")]
pub async fn synths_get_one(state: State<'_, std::sync::Arc<GuiState>>, num: u32) -> Option<Json<Synth> > {
	let lock = state.mutex.lock().await;
	lock.synths.iter().find(|s| s.id == num).cloned().map(|synth| Json(synth))
}

#[get("/synths/<synthnum>/chains")]
pub async fn chains_get(state: State<'_, std::sync::Arc<GuiState>>, synthnum: u32) -> Option<Json<Vec<Chain>> > {
	let lock = state.mutex.lock().await;
	lock.synths.iter().find(|s| s.id == synthnum).map(|s| Json(s.chains.clone()))
}

#[get("/synths/<synthnum>/chains/<chainnum>")]
pub async fn chains_get_one(state: State<'_, std::sync::Arc<GuiState>>, synthnum: u32, chainnum:u32) -> Option<Json<Chain> > {
	let lock = state.mutex.lock().await;
	lock.synths.iter().find(|s| s.id == synthnum).map(
		|s| s.chains.iter().find(|c| c.id == chainnum).map(
			|c| Json(c.clone())
		)
	).flatten()
}

#[get("/synths/<synthnum>/chains/<chainnum>/takes")]
pub async fn takes_get(state: State<'_, std::sync::Arc<GuiState>>, synthnum: u32, chainnum:u32) -> Option<Json<Vec<Take>> > {
	let lock = state.mutex.lock().await;
	lock.synths.iter().find(|s| s.id == synthnum).map(
		|s| s.chains.iter().find(|c| c.id == chainnum).map(
			|c| Json(c.takes.clone())
		)
	).flatten()
}

#[get("/synths/<synthnum>/chains/<chainnum>/takes/<takenum>")]
pub async fn takes_get_one(state: State<'_, std::sync::Arc<GuiState>>, synthnum: u32, chainnum:u32, takenum: u32) -> Option<Json<Take> > {
	let lock = state.mutex.lock().await;
	lock.synths.iter().find(|s| s.id == synthnum).map(
		|s| s.chains.iter().find(|c| c.id == chainnum).map(
			|c| c.takes.iter().find(|t| t.id == takenum).map(
				|t| Json(t.clone())
			)
		)
	).flatten().flatten()
}

