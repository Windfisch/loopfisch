use crate::engine::{FrontendThreadState, GuiTake};

use std::sync::atomic::AtomicBool;

use std::collections::HashMap;

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

struct DevicePair {
	audio_dev: Option<usize>,
	midi_dev: Option<usize>
}

/// Very basic text mode user interface.
pub struct UserInterface {
	dev_id: usize,
	take_id: usize,
	device_map: Vec<DevicePair>,
	take_supplement: HashMap<u32, TakeSupplement>
}

struct TakeSupplement {
	marked: bool
}

impl Default for TakeSupplement {
	fn default() -> Self {
		return TakeSupplement {
			marked: false
		}
	}
}

impl Drop for UserInterface {
	fn drop(&mut self) {
		print!("\r\n--- disabling terminal raw mode ---\r\n");
		crossterm::terminal::disable_raw_mode().unwrap();
		INSTANCE_EXISTS.store(false, std::sync::atomic::Ordering::Relaxed);
	}
}

fn hilight(s: &str, hilight: bool, semi: bool) -> crossterm::style::StyledContent<String> {
	use crossterm::style::Styler;
	use crossterm::style::Colorize;
	if hilight {
		return s.to_owned().negative();
	}
	else if semi {
		return s.to_owned().on_grey();
	}
	else {
		return s.to_owned().reset();
	}
}

impl UserInterface {
	pub fn new() -> UserInterface {
		if INSTANCE_EXISTS.compare_and_swap(false, true, std::sync::atomic::Ordering::Relaxed) {
			panic!("Cannot create more than one UserInterface!");
		}

		crossterm::terminal::enable_raw_mode().unwrap();
		UserInterface {
			dev_id: 0,
			take_id: 0,
			device_map: vec![ // FIXME configure
				DevicePair{ audio_dev: Some(0), midi_dev: Some(0) },
				DevicePair{ audio_dev: Some(1), midi_dev: None },
				DevicePair{ audio_dev: None   , midi_dev: Some(1) },
			],
			take_supplement: HashMap::new()
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
		print!("\r\n");

		for (i,dev) in frontend_thread_state.devices().iter().enumerate() {
			print!("{:10} | ", hilight(&dev.info().name, i == self.dev_id, false));
		}
		print!("\r\n\n");
		for j in 0..26 {
			for (i,dev) in frontend_thread_state.devices().iter().enumerate() {
				let label =
					if dev.takes().len() > j {
						let take = &dev.takes()[j];
						let suppl = self.take_supplement.get(&take.id);
						let marked = if let Some(s) = suppl { s.marked } else { false };
						let letter = LETTERS[j];
						let label = format!("({:1}) #{}", letter, take.id);
						let selected = i == self.dev_id && j == self.take_id; // FIXME
						hilight(&label, selected, marked)
					}
					else {
						hilight("", false, false)
					};
				print!("{:10} | ", label);
			}
			print!("\r\n");
		}


	}

	fn current_take<'a>(&self, frontend_thread_state: &'a FrontendThreadState) -> Option<&'a GuiTake> {
		if self.dev_id < frontend_thread_state.devices().len() {
			let d = &frontend_thread_state.devices()[self.dev_id];
			if self.take_id < d.takes().len() {
				return Some(&d.takes()[self.take_id]);
			}
		}
		return None;
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
									if num <= letter2id('l') {
										self.take_id = num as usize
									}
									else {
										match c {
											'z' => {frontend_thread_state.add_take(self.dev_id).unwrap();}
											'x' => {frontend_thread_state.add_miditake(self.dev_id).unwrap();}
											'v' => {
												if let Some(t) = self.current_take(frontend_thread_state) {
													let ent = self.take_supplement.entry(t.id).or_default();
													ent.marked = !ent.marked;
												}
											}
											'c' => {
												for ent in self.take_supplement.iter_mut() {
													ent.1.marked = false
												}
											}
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

