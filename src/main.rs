// loopfisch -- A loop machine written in rust.
// Copyright (C) 2020 Florian Jung <flo@windfis.ch>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License verion 3 as
// published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

mod engine;
mod metronome;
mod jack_driver;
mod midi_message;
mod bit_array;
mod midi_registry;
mod outsourced_allocation_buffer;
mod id_generator;
use tokio;

mod rest_api;

#[macro_use] extern crate rocket;


#[cfg(debug_assertions)]
#[global_allocator]
static A: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;








#[rocket::main]
async fn main() {
    println!("Hello, world!");

	let engine = engine::launch();
	//let frontend_thread_state = engine.get_frontend_thread_state();

	/*let mut rt = tokio::runtime::Builder::new()
		.threaded_scheduler()
		.enable_all()
		.build()
		.unwrap();*/

	rest_api::launch_server(engine).await;
	//rt.block_on(rest_api::launch_server(frontend_thread_state));
	return;
}
