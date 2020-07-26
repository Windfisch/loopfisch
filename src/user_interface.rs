use crate::engine::FrontendThreadState;

use std::sync::atomic::AtomicBool;

static INSTANCE_EXISTS: AtomicBool = AtomicBool::new(false);

static LETTERS: [char;26] = ['q','w','e','r','t','y','u','i','o','p','a','s','d','f','g','h','j','k','l','z','x','c','v','b','n','m'];
fn letter2id(c: char) -> u32 {
	for (i,l) in LETTERS.iter().enumerate() {
		if *l==c { return i as u32; }
	}
	panic!("letter2id must be called with a letter");
}
//fn id2letter(i: u32) -> char {
//	return LETTERS[i as usize];
//}

/// Very basic text mode user interface.
pub struct UserInterface {
	dev_id: usize
}

impl Drop for UserInterface {
	fn drop(&mut self) {
		print!("\r\n--- disabling terminal raw mode ---\r\n");
		crossterm::terminal::disable_raw_mode().unwrap();
		INSTANCE_EXISTS.store(false, std::sync::atomic::Ordering::Relaxed);
	}
}

impl UserInterface {
	pub fn new() -> UserInterface {
		if INSTANCE_EXISTS.compare_and_swap(false, true, std::sync::atomic::Ordering::Relaxed) {
			panic!("Cannot create more than one UserInterface!");
		}

		crossterm::terminal::enable_raw_mode().unwrap();
		UserInterface {
			dev_id: 0
		}
	}

	fn redraw(&self, frontend_thread_state: &FrontendThreadState) {
		use std::io::{Write, stdout};
		use crossterm::cursor::*;
		use crossterm::terminal::*;
		use crossterm::*;
		execute!( stdout(),
			Clear(ClearType::All),
			MoveTo(0,0)
		).unwrap();

		let song_length = frontend_thread_state.shared.song_length.load(std::sync::atomic::Ordering::Relaxed);
		let song_position = frontend_thread_state.shared.song_position.load(std::sync::atomic::Ordering::Relaxed);
		let transport_position = frontend_thread_state.shared.transport_position.load(std::sync::atomic::Ordering::Relaxed);
		print!("Transport position: {}     \r\n", transport_position);
		print!("Song position: {:5.1}% {:2x} {:1} {}      \r\n", (song_position as f32 / song_length as f32) * 100.0, 256*song_position / song_length, 8 * song_position / song_length, song_position);
		print!("Selected device: {}    \r\n", self.dev_id);
	}

	fn handle_input(&mut self, frontend_thread_state: &mut FrontendThreadState) -> crossterm::Result<bool> {
		use std::time::Duration;
		use crossterm::event::{KeyModifiers,KeyCode};
		while crossterm::event::poll(Duration::from_millis(16))? {
			let ev = crossterm::event::read()?;
			match ev {
				crossterm::event::Event::Key(kev) => {
					//println!("key: {:?}", kev);

					if kev.code == KeyCode::Char('c') && kev.modifiers == KeyModifiers::CONTROL {
						return Ok(true);
					}

					match kev.code {
						KeyCode::Char(c) => {
							match c {
								'0'..='9' => {
									self.dev_id = c as usize - '0' as usize;
								}
								'a'..='z' => {
									let num = letter2id(c);
									if num <= letter2id('p') {
										frontend_thread_state.toggle_take_muted(num as usize).unwrap();
									}
									else if num <= letter2id('l') {
										frontend_thread_state.toggle_miditake_muted((num - letter2id('a')) as usize).unwrap();
									}
									else {
										match c {
											'z' => {frontend_thread_state.add_take(self.dev_id).unwrap();}
											'x' => {frontend_thread_state.add_miditake(self.dev_id).unwrap();}
											_ => {}
										}
									}
								}
								_ => {}
							}
						}
						_ => {}
					}
				}
				_ => {}
			}
		}
		Ok(false)
	}
	
	pub fn spin(&mut self, frontend_thread_state: &mut FrontendThreadState) -> crossterm::Result<bool> {
		self.redraw(frontend_thread_state);
		self.handle_input(frontend_thread_state)
	}
}

