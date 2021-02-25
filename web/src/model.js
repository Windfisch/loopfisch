/** retrieves obj.prop or throws if not present */
function get(obj, prop) {
	if (!(prop in obj)) {
		throw "Property '" + prop + "' is missing";
	}
	return obj[prop];
}

export class TakeModel {
	constructor(data, parent_chain) {
		copy_all_properties(this, data, ['id', 'name', 'type', 'state', 'muted', 'muted_scheduled', 'associated_midi_takes', 'playing_since', 'duration']);
		this.selected = false;
		this.parent_chain = parent_chain;
	}

	patch(data) {
		copy_existing_properties(this, data, ['name', 'type', 'state', 'muted', 'muted_scheduled', 'associated_midi_takes', 'playing_since', 'duration']);
	}
}

export class ChainModel {
	constructor(data, parent_synth) {
		copy_all_properties(this, data, ['id', 'name', 'midi', 'echo']);
		this.takes = data.takes ? data.takes.map((take) => new TakeModel(take, this)) : [];
		this.selected = false;
		this.parent_synth = parent_synth;
	}
	
	patch(data) {
		copy_existing_properties(this, data, ['name', 'midi', 'echo']);
		patch_array(this.takes, data.takes, TakeModel, this);
	}

	async new_take(type) {
		var post = await fetch(
			"http://localhost:8000/api/synths/" + this.parent_synth.id
			+ "/chains/" + this.id + "/takes", {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			redirect: 'follow',
			mode: 'cors',
			body: JSON.stringify({
				"name": "New Take",
				"type": type
			})
		});
		if (post.status == 201) {
			path = post.headers.get('Location');
			console.log(path);

			var response = await fetch("http://localhost:8000"+path);
			if (response.status !== 200) {
				console.log("whoopsie :o");
				return;
			}

			json = await response.json();
			console.log(json);
			if (this.takes.find(x => x.id === json.id) === undefined) {
				this.takes.push(new TakeModel(json, this));
			}
		}
		else {
			alert("Failed to create take!");
		}
	}

	async toggle_record_audio() {
		if (this.has_recording_takes()) {
			await this.stop_all_recording_takes();
		}
		else {
			await this.new_take("Audio");
		}
	}

	async toggle_record_midi() {
		if (this.has_recording_takes()) {
			await this.stop_all_recording_takes();
		}
		else {
			await this.new_take("Midi");
		}
	}

	has_recording_takes() {
		return this.takes.filter(t => t.state === "Recording").length > 0;
	}

	async stop_recording_take(take) { // FIXME move to take
		fetch(
			"http://localhost:8000/api/synths/" + this.parent_synth.id +
			"/chains/" + this.id +
			"/takes/" + take.id +
			"/finish_recording",
			{ method: 'POST', mode: 'cors' }
		);
	}

	async stop_all_recording_takes() {
		for (var take of this.takes.filter(t => t.state === "Recording")) {
			this.stop_recording_take(take);
		}
	}

	set_take_audiomute(take, state) {
		take.muted = state;
		
		if (!take.audiomute && !take.midimute) {
			this.update_associated_midi_takes(take, true);
		}
		
		this.send_mute_patch(take);
	}

	set_take_midimute(take, state) {
		if (take.type == "Audio") {
			this.update_associated_midi_takes(take, state);
			if (!take.audiomute && !take.midimute) {
				take.muted = true;
			}
		}
		else {
			take.muted = !take.muted;
		}
		this.send_mute_patch(take);
	}

	get_take_audiomute(take) {
		return take.type == "Audio" ? take.muted : true;
	}

	get_take_midimute(take) {
		if (take.type == "Audio")
		{
			var result = true;
			for (var id of take.associated_midi_takes) {
				var miditake = this.takes.find(t => t.id == id);
				result = result && miditake.muted;
			}
			return result;
		}
		else
		{
			return take.muted;
		}
	}

	async set_echo(state) { // FIXME move to synth
		console.log(state);
		this.echo = state;

		for (var chain of this.parent_synth.chains) {
			if (chain != this) {
				chain.echo = false;
			}
		}
		
		var patch = [];
		for (var chain of this.parent_synth.chains) {
			patch.push({id: chain.id, echo: chain.echo});
		}

		console.log(patch);

		var req = await fetch(
			"http://localhost:8000/api/synths/" + this.parent_synth.id
			+ "/chains", {
			method: 'PATCH',
			headers: { 'Content-Type': 'application/json' },
			redirect: 'follow',
			mode: 'cors',
			body: JSON.stringify(patch)
		});
	}

	async send_mute_patch(take) {
		console.log(this.parent_synth, this.parent_synth.id);

		var patch = [];
		patch.push({id: take.id, muted: take.muted});
		for (var id of take.associated_midi_takes) {
			var miditake = this.takes.find(t => t.id == id);
			patch.push({id: id, muted: miditake.muted});
		}

		console.log("sending patch");
		console.log(patch);

		var req = await fetch(
			"http://localhost:8000/api/synths/" + this.parent_synth.id
			+ "/chains/" + this.id + "/takes", {
			method: 'PATCH',
			headers: { 'Content-Type': 'application/json' },
			redirect: 'follow',
			mode: 'cors',
			body: JSON.stringify(patch)
		});
		console.log(req.status);
	}

	update_associated_midi_takes(take, value) {
		for (var id of take.associated_midi_takes) {
			var miditake = this.takes.find(t => t.id == id);
			miditake.muted = value;
		}
	}
}

export class SynthModel {
	constructor(data) {
		copy_all_properties(this, data, ['id', 'name']);
		this.chains = data.chains ? data.chains.map((chain) => new ChainModel(chain, this)) : [];
	}
	
	patch(data) {
		copy_existing_properties(this, data, ['name']);
		patch_array(this.chains, data.chains, ChainModel, this);
	}

	async add_chain() {
		var post = await fetch("http://localhost:8000/api/synths/" + this.id + "/chains", {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			redirect: 'follow',
			mode: 'cors',
			body: JSON.stringify({
				"name": "New Chain"
			})
		});
		if (post.status == 201) {
			var path = post.headers.get('Location');
			console.log(path);

			var response = await fetch("http://localhost:8000"+path);
			if (response.status !== 200) {
				console.log("whoopsie :o");
				return;
			}

			var json = await response.json();
			console.log(json);
			if (this.chains.find(x => x.id === json.id) === undefined) {
				this.chains.push(new ChainModel(json, this));
			}
		}
		else {
			alert("Failed to create chain!");
		}
	}

	async restart_transport() {
		await fetch("http://localhost:8000/api/synths/" + this.id + "/restart_transport", {
			method: 'POST',
			redirect: 'follow',
			mode: 'cors'
		});
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

export function patch_array(array_to_patch, patches, clazz, parent_object) {
	console.log("patch array ", parent_object);
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
				array_to_patch.push(new clazz(patch, parent_object));
			}
			else {
				object_to_patch.patch(patch);
			}
		}
	}
}

export async function add_synth(synth_array) {
	var post = await fetch("http://localhost:8000/api/synths", {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		redirect: 'follow',
		mode: 'cors',
		body: JSON.stringify({
			"name": "New Synth"
		})
	});
	if (post.status == 201) {
		var path = post.headers.get('Location');
		console.log(path);

		var response = await fetch("http://localhost:8000"+path);
		if (response.status !== 200) {
			console.log("whoopsie :o");
			return;
		}

		var json = await response.json();
		console.log(json);
		if (synth_array.find(x => x.id === json.id) === undefined) {
			synth_array.push(new SynthModel(json));
		}
	}
	else {
		alert("Failed to create synth!");
	}
}

