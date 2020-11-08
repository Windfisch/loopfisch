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
	let update_list = Arc::new(UpdateList::new());
	let state = GuiState {
		update_list: update_list.clone(),
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

	let mut event_channel = event_channel_;
	tokio::task::spawn( async move {
		loop {
			match event_channel.receive().await
			{
				Event::AudioTakeStateChanged(dev_id, take_id, new_state) =>
				{
					println!("\n\n\n############# audio state {:?}\n\n\n", new_state);
				}
				Event::MidiTakeStateChanged(mididev_id, take_id, new_state) =>
				{
					println!("\n\n\n############# midi state {:?}\n\n\n", new_state);
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
			updates,
			synths_get, synths_get_one,
			chains_get, chains_get_one,
			takes_get, takes_get_one,
			patch_synths, patch_synth, post_synth,
			patch_chains, patch_chain, post_chain,
			patch_takes, patch_take, post_take,
		])
		.register(catchers![not_found])
		.attach(cors::CORS())
		.launch().await.unwrap();
}
