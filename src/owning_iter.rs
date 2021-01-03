use stable_deref_trait::StableDeref;

/// Can be used to return an iterator of a container inside
/// of a Mutex, RefCell or similar.
///
/// Safety: The closure passed to `new` must ensure that the
/// returned iterator does not outlive the referent of the pointer.
///
/// Usage example:
/// ```
/// fn demo(mutex: &std::sync::Mutex<Vec<u32>>) -> impl Iterator<Item=&u32> {
///     return OwningIterator::new(
///         mutex.lock().unwrap(),
///         |vec| unsafe { (*vec).iter() }
///     );
/// }
/// ```
///
/// Note that the mutex remains locked until the  OwningIterator that was
/// returned by demo() is dropped.
pub struct OwningIterator<O: StableDeref, I: Iterator> {
	iter: I,
	_owner: O
}

impl<O: StableDeref, I: Iterator> Iterator for OwningIterator<O, I> {
	type Item = I::Item;
	fn next(&mut self) -> Option<Self::Item> {
		self.iter.next()
	}
}

impl<O: StableDeref + std::ops::DerefMut, I: Iterator> OwningIterator<O, I> {
	pub fn new(mut owner: O, func: impl FnOnce(*mut O::Target) -> I) -> OwningIterator<O, I> {
		let iter = {
			func(owner.deref_mut() as *mut O::Target)
		};

		OwningIterator {
			iter,
			_owner: owner
		}
	}
}
