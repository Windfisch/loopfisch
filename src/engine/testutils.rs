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

pub fn rand_iter_f32(seed: u32) -> impl Iterator<Item=f32> {
	rand_iter(seed).map(|x| x as f32 / 0x8000_0000_0000_0000u64 as f32 - 1.0)
}

pub fn rand_vec_f32(seed: u32, length: usize) -> Vec<f32> {
	rand_iter_f32(seed).take(length).collect()
}
	
pub fn ticks(samples: &[f32], level: f32) -> Vec<usize> {
	let mut high_time = 0;
	let mut result = vec![];
	for (i,s) in samples.iter().enumerate() {
		if s.abs() <= level/2.0 {
			if high_time > 0 {
				high_time -= 1;
			}
		}
		else if s.abs() >= level {
			if high_time == 0 {
				result.push(i);
			}
			high_time = 100;
		}
	}
	return result;
}

// GRCOV_EXCL_START
pub fn slice_diff<T: PartialEq + std::fmt::Debug>(lhs: &[T], rhs: &[T]) {
	if let Some(result) = lhs.iter().zip(rhs.iter()).map(|x| x.0 != x.1).enumerate().find(|t| t.1) {
		let index = result.0;
		let max = std::cmp::max(lhs.len(), rhs.len());
		let lo = if index < 10 { 0 } else { index-10 };
		let hi = if index + 10 >= max { max } else { index+10 };

		println!("First difference at {}, context: {:?} != {:?}", index, &lhs[lo..hi], &rhs[lo..hi]);
	}
}

/// Asserts two (large) slices are equal. Prints a small context around the first
/// difference, if unequal
#[macro_export]
macro_rules! assert_sleq {
	($lhs:expr, 0.0) => {{
		let lhs = &$lhs;
		let rhs = &vec![0.0; lhs.len()];
		if *lhs != *rhs {
			slice_diff(lhs, rhs);
			panic!("Slices are different!");
		}
	}};
	($lhs:expr, 0.0, $reason:expr) => {{
		let lhs = &$lhs;
		let rhs = &vec![0.0; lhs.len()];
		if *lhs != *rhs {
			slice_diff(lhs, rhs);
			panic!($reason);
		}
	}};
	($lhs:expr, $rhs:expr) => {{
		let lhs = &$lhs;
		let rhs = &$rhs;
		if *lhs != *rhs {
			slice_diff(lhs, rhs);
			panic!("Slices are different!");
		}
	}};
	($lhs:expr, $rhs:expr, $reason:expr) => {{
		let lhs = &$lhs;
		let rhs = &$rhs;
		if *lhs != *rhs {
			slice_diff(lhs, rhs);
			panic!($reason);
		}
	}}
}

pub fn assert_iter_eq<T: PartialEq + std::fmt::Debug>(mut iter1: impl Iterator<Item=T>, mut iter2: impl Iterator<Item=T>) {
	let mut i = 0;
	loop {
		let v1 = iter1.next();
		let v2 = iter2.next();
		match (v1.is_some(), v2.is_some()) {
			(false, false) => break,
			(true, false) => panic!("First list is longer than the second"),
			(false, true) => panic!("Second list is longer than the first"),
			(true, true) => {}
		};
		assert_eq!(v1.unwrap(), v2.unwrap());
		i += 1;
	}
	assert!(i > 0, "assert_iter_eq fails when both lists are empty");
}
// GRCOV_EXCL_STOP

