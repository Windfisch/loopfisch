pub struct BitArray2048 {
	storage: [u8; 256]
}

impl BitArray2048 {
	pub fn new() -> BitArray2048 {
		BitArray2048{ storage: [0; 256] }
	}

	fn storidx_and_mask(idx: u32) -> (usize, u8) {
		let storidx = idx / 8;
		let bitidx = idx % 8;
		let mask = 1 << bitidx;
		return (storidx as usize, mask);
	}

	pub fn set(&mut self, idx: u32, val: bool) {
		let (storidx, mask) = Self::storidx_and_mask(idx);
		if val {
			self.storage[storidx] |= mask;
		}
		else {
			self.storage[storidx] &= !mask;
		}
	}

	pub fn get(&self, idx: u32) -> bool {
		let (storidx, mask) = Self::storidx_and_mask(idx);
		return (self.storage[storidx] & mask) != 0;
	}
}

