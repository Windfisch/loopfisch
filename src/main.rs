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

#![feature(proc_macro_hygiene)]
#![feature(generators)]
#![feature(in_band_lifetimes)]
#![feature(generic_associated_types)]

mod engine;
mod midi_message;
mod outsourced_allocation_buffer;
mod id_generator;
mod rest_api;
mod realtime_send_queue;

#[macro_use] extern crate rocket;


#[cfg(debug_assertions)]
#[global_allocator]
static A: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;








#[rocket::main]
async fn main() {
    println!("Hello, world!");

	let (engine, event_queue) = engine::launch(6000);
	rest_api::launch_server(engine, event_queue).await;
	return;
}
