use num_traits::*;

pub fn spacing<T: Num + Ord + Copy + std::ops::Sub + Bounded + std::fmt::Display>(mut foo: impl Iterator<Item=T>) -> (T, T) {
	let mut prev = foo.next().unwrap();

	let mut lo = T::max_value();
	let mut hi = T::min_value();

	for val in foo {
		let diff = val - prev;
		lo = clamp_max(lo, diff);
		hi = clamp_min(hi, diff);

		prev = val;
	}
	return (lo, hi);
}
	
pub struct XorShift {
	state: u64
}

impl Iterator for XorShift {
	type Item = u64;
	fn next(&mut self) -> Option<u64> {
		self.state ^= self.state << 13;
		self.state ^= self.state >> 17;
		self.state ^= self.state << 5;
		Some(self.state)
	}
}

pub fn rand_iter(seed: u32) -> XorShift {
	let mut iter = XorShift { state: seed as u64 | (((seed ^ 0xDEADBEEF) as u64) << 32) };
	for _ in 0..16 {
		iter.next();
	}
	return iter;
}

pub fn rand_vec(seed: u32, length: usize) -> Vec<u64> {
	rand_iter(seed).take(length).collect()
}

pub fn rand_iter_f32(seed: u32) -> impl Iterator<Item=f32> {
	rand_iter(seed).map(|x| x as f32 / 0x8000_0000_0000_0000u64 as f32 - 1.0)
}

pub fn rand_vec_f32(seed: u32, length: usize) -> Vec<f32> {
	rand_iter_f32(seed).take(length).collect()
}
