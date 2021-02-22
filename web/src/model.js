/** retrieves obj.prop or throws if not present */
function get(obj, prop) {
	if (!(prop in obj)) {
		throw "Property '" + prop + "' is missing";
	}
	return obj[prop];
}

export class TakeModel {
	constructor(data) {
		copy_all_properties(this, data, ['id', 'name', 'type', 'state', 'muted', 'muted_scheduled', 'associated_midi_takes', 'playing_since', 'duration']);
		this.selected = false;
	}

	patch(data) {
		copy_existing_properties(this, data, ['name', 'type', 'state', 'muted', 'muted_scheduled', 'associated_midi_takes', 'playing_since', 'duration']);
	}
}

export class ChainModel {
	constructor(data) {
		copy_all_properties(this, data, ['id', 'name', 'midi', 'echo']);
		this.takes = data.takes ? data.takes.map((take) => new TakeModel(take)) : [];
		this.selected = false;
	}
	
	patch(data) {
		copy_existing_properties(this, data, ['name', 'midi', 'echo']);
		patch_array(this.takes, data.takes, TakeModel);
	}
}

export class SynthModel {
	constructor(data) {
		copy_all_properties(this, data, ['id', 'name']);
		this.chains = data.chains ? data.chains.map((chain) => new ChainModel(chain)) : [];
	}
	
	patch(data) {
		copy_existing_properties(this, data, ['name']);
		patch_array(this.chains, data.chains, ChainModel);
	}
}

function copy_all_properties(destination, source, properties) {
	for (const prop of properties) {
		if (!prop in source) {
			throw "Error: Property '" + prop + "' is missing in source object";
		}
		destination[prop] = source[prop];
	}
}

function copy_existing_properties(destination, source, properties) {
	for (const prop of properties) {
		if (prop in source) {
			destination[prop] = source[prop];
		}
	}
}

export function patch_array(array_to_patch, patches, clazz) {
	if (patches === undefined || patches === null) {
		return;
	}
	for (let patch of patches) {
		if (patch.delete === true) {
			let index = array_to_patch.findIndex( x => x.id === patch.id );
			if (index != -1) {
				array_to_patch.splice(index, 1);
			}
		}
		else
		{
			let object_to_patch = array_to_patch.find( x => x.id === patch.id );
			if (object_to_patch === undefined) {
				array_to_patch.push(new clazz(patch));
			}
			else {
				object_to_patch.patch(patch);
			}
		}
	}
}


