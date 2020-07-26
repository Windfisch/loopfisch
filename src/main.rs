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
mod user_interface;
use user_interface::UserInterface;

use crossterm;


#[cfg(debug_assertions)]
#[global_allocator]
static A: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;









fn main() {
    println!("Hello, world!");

	let mut engine = engine::launch();
	let frontend_thread_state = engine.get_frontend_thread_state();

	let mut ui = UserInterface::new();
	crossterm::terminal::enable_raw_mode().unwrap();
	loop {
		if ui.spin(frontend_thread_state).unwrap() {
			break;
		}
	}
	crossterm::terminal::disable_raw_mode().unwrap();
}
