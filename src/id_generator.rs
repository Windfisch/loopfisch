pub struct IdGenerator {
	counter: u32
}
impl IdGenerator {
	pub fn gen(&mut self) -> u32 {
		let result = self.counter;
		self.counter += 1;
		result
	}

	pub fn new() -> IdGenerator { IdGenerator{ counter: 0 } }
}

impl Default for IdGenerator {
    fn default() -> IdGenerator { IdGenerator::new() }
}
