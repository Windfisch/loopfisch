mod cors;
mod util;
mod updates;
mod gui_state;
mod data;
mod get;
mod patch;
mod post;

use get::*;
use patch::*;
use post::*;
use updates::*;
use gui_state::*;


use crate::engine::{Event, FrontendThreadState};
use async_std::sync::Mutex;
use std::sync::Arc;
use crate::id_generator::IdGenerator;
use crate::realtime_send_queue;



#[catch(404)]
fn not_found(req: &rocket::Request) -> String {
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

pub async fn launch_server(engine: FrontendThreadState, event_channel_: realtime_send_queue::Consumer<Event>) {
	let sample_rate = engine.sample_rate();
	let update_list = Arc::new(UpdateList::new());
	let state = Arc::new( GuiState {
		update_list: update_list.clone(),
		mutex: Mutex::new( GuiMutexedState {
			engine,
			synths: vec![],
			take_id: IdGenerator::new(),
			chain_id: IdGenerator::new(),
			synth_id: IdGenerator::new()
		})
	} );

	let mut event_channel = event_channel_;
	let state2 = state.clone();
	tokio::task::spawn( async move {
		loop {
			match event_channel.receive().await
			{
				Event::AudioTakeStateChanged(dev_id, take_id, new_state, timestamp) =>
				{
					println!("\n\n\n############# audio state {:?}\n\n\n", new_state);
					let mut guard = state2.mutex.lock().await;
					if let Some((synthid, chainid, mut take)) = guard.find_audiotake_by_engine_id(dev_id, take_id) {
						use crate::engine::RecordState;
						take.state =
							match new_state {
								RecordState::Waiting => RecordingState::Waiting,
								RecordState::Recording => RecordingState::Recording(timestamp),
								RecordState::Finished => RecordingState::Finished
							};
						state2.update_list.push( make_update_take(&take, synthid, chainid) ).await;
					}
					else {
						panic!();
					}
				}
				Event::MidiTakeStateChanged(mididev_id, take_id, new_state, timestamp) =>
				{
					println!("\n\n\n############# midi state {:?}\n\n\n", new_state);
					let mut guard = state2.mutex.lock().await;
					if let Some((synthid, chainid, mut take)) = guard.find_miditake_by_engine_id(mididev_id, take_id) {
						use crate::engine::RecordState;
						take.state =
							match new_state {
								RecordState::Waiting => RecordingState::Waiting,
								RecordState::Recording => RecordingState::Recording(timestamp),
								RecordState::Finished => RecordingState::Finished
							};
						state2.update_list.push( make_update_take(&take, synthid, chainid) ).await;
					}
					else {
						panic!();
					}
				}
				Event::Timestamp(song_position, transport_position) =>
				{
					state2.update_list.push(UpdateRoot {
						synths: None,
						song: Some(UpdateSong {
							song_position: Some(song_position as f32 / sample_rate as f32),
							transport_position: Some(transport_position as f32 / sample_rate as f32),
							loop_length: None,
						})
					}).await;
				}
				Event::Kill =>
				{
					println!("\n\n\n############# error reading\n\n\n"); break;
				}
			}
		}
	});
	
	rocket::ignite()
		.manage(state)
		.mount("/api", routes![
			cors::options,
			song_get, song_patch,
			updates,
			synths_get, synths_get_one,
			chains_get, chains_get_one,
			takes_get, takes_get_one,
			patch_synths, patch_synth, post_synth,
			patch_chains, patch_chain, post_chain,
			patch_takes, patch_take, post_take, post_take_finish_recording, post_restart_transport
		])
		.register(catchers![not_found])
		.attach(cors::CORS())
		.launch().await.unwrap();
}
