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
