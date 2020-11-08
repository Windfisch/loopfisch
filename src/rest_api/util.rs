
pub fn gen_unique_name<'a,T: Iterator<Item=&'a str> + Clone>(desired_name: &str, iter: T) -> String {
	if iter.clone().find(|s| *s == desired_name).is_some() {
		let mut i = 2;
		loop {
			let name = format!("{} {}", desired_name, i);
			if iter.clone().find(|s| *s == name).is_none() {
				return name;
			}
			i+=1;
		}
	}
	else {
		return desired_name.into();
	}
}

