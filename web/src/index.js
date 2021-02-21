import pie from './pie.vue';
import take from './take.vue';
import chain from './chain.vue';
import synth from './synth.vue';
import bpm from './bpm.vue';
import Vue from 'vue';

Vue.component('pie', pie);
Vue.component("take", take);
Vue.component("chain", chain);
Vue.component("synth", synth);
Vue.component("bpm", bpm);

// FIXME this function should not exist. this is an ugly hack around a design mistake.
// start_recording etc should not be methods of the vue components, but top level functions.
// or methods of the data objects.
function component_with_model(root, model) {
	if (root.model === model || root.reference === model) {
		return root;
	}
	if (root.$children !== undefined) {
		for (var child of root.$children) {
			var temp = component_with_model(child, model);
			if (temp !== undefined) {
				return temp;
			}
		}
	}
	return undefined;
}


var app2 = new Vue({
	el: '#app',
	created() {
		window.addEventListener('keyup', (e) => {
			this.pressed_keys.delete(e.code);

			console.log(this.pressed_keys.size);
			console.log(this.keys_in_chord.size);
			console.log(e.shiftKey);
			console.log(this.deselect_on_1_chord);
			if (this.pressed_keys.size == 0 && e.shiftKey == false && this.keys_in_chord.size == 1) {
				for (var thing of this.deselect_on_1_chord) {
					thing.selected = false;
				}
				this.deselect_on_1_chord.clear();
			}
		});
		window.addEventListener('keydown', (e) => {
			var qwerty = ["KeyQ", "KeyW", "KeyE", "KeyR", "KeyT", "KeyY", "KeyU", "KeyI", "KeyO", "KeyP"]
			var asdf = ["KeyA", "KeyS", "KeyD", "KeyF", "KeyG", "KeyH", "KeyJ", "KeyK", "KeyL", "KeyZ", "KeyX", "KeyC", "KeyV", "KeyB", "KeyN", "KeyM"];
			if (e.srcElement.nodeName != "INPUT" && e.repeat == false) {
				console.log(e);
				var chain_index = qwerty.findIndex((x) => x == e.code);
				var take_index = asdf.findIndex((x) => x == e.code);

				if (chain_index != -1 || take_index != -1) {
					if (this.pressed_keys.size == 0 && !e.shiftKey) {
						this.keys_in_chord.clear();
						this.deselect_on_1_chord.clear();
					}
					this.keys_in_chord.add(e.code);
					console.log("keys in chord: ", this.keys_in_chord);

					this.pressed_keys.add(e.code);
				}

				var all_chains = this.synths.map((synth) => synth.chains).flat();
				if (chain_index >= 0) {
					var chain = chain_index <= all_chains.length ? all_chains[chain_index] : null;

					if (chain && this.keys_in_chord.size == 1 && chain.selected == true) {
						var n_selected = all_chains.map((c) => c.selected ? 1 : 0).reduce((a,b)=>a+b, 0);
						console.log(n_selected);
						if (n_selected == 1) {
							this.deselect_on_1_chord.add(chain);
						}
					}

					if (!e.shiftKey && this.pressed_keys.size <= 1) {
						for (let chain of all_chains) {
							chain.selected = false;
						}
					}
					
					if (chain)
					{
						chain.selected = !chain.selected;
						console.log(all_chains[chain_index].selected);
					}
				}

				if (take_index >= 0) {
					var n_selected_positions = new Set(
							all_chains
								.filter((c) => c.selected)
								.map((c) => c.takes
										.map((t, i) => [t.selected, i])
										.filter((tuple) => tuple[0])
								)
								.flat()
						).size;
					console.log("n selected pos", n_selected_positions);
					for (var chain of all_chains.filter((c) => c.selected)) {
						console.log(chain);

						var take = (take_index < chain.takes.length) ? chain.takes[take_index] : null;
					
						if (take && this.keys_in_chord.size == 1 && take.selected == true) {
							this.deselect_on_1_chord.add(take);
						}

						if (!e.shiftKey && this.pressed_keys.size <= 1) {
							for (var t of chain.takes) {
								t.selected = false;
							}
						}

						if (take) {
							take.selected = !take.selected;
						}
					}
				}

				if (e.key == "1") {
					for (let chain of all_chains.filter((c) => c.selected)) {
						component_with_model(this, chain).toggle_echo();
					}
				}
				if (e.key == "2") {
					for (let chain of all_chains.filter((c) => c.selected)) {
						component_with_model(this, chain).showhidemidi();
					}
				}
				if (e.key == "3") {
					for (let chain of all_chains.filter((c) => c.selected)) {
						component_with_model(this, chain).record_audio();
					}
				}
				if (e.key == "4") {
					for (let chain of all_chains.filter((c) => c.selected)) {
						component_with_model(this, chain).record_midi();
					}
				}
			}
		});
	},
	data: function(){ return{
		playback_time: 0,
		message: "Hello World",
		count: 0,
		loop_settings: {
			bpm: 126,
			beats:8,
		},
		user_id: "<not registered yet>",
		pressed_keys: new Set(),
		keys_in_chord: new Set(),
		deselect_on_1_chord: new Set(),
		synths: [
			{
				name: "Deepmind 13",
				id: 0,
				chains: [
					{
						name: "Pad",
						midi: true,
						echo: false,
						takes: [
							{
								id: 0,
								name: "Flausch",
								type: "Audio",
								audiomute: true,
								midimute: true,
								associated_midi_takes: [1]
							},
							{
								id: 3,
								name: "gefiltertes Flausch",
								type: "Audio",
								audiomute: false,
								midimute: true,
								associated_midi_takes: [1,2]
							},
							{
								id: 1,
								name: "Flausch (MIDI)",
								type: "Midi",
								audiomute: false,
								midimute: true
							},
							{
								id: 2,
								name: "Filter controller",
								type: "Midi",
								audiomute: false,
								midimute: true
							},
						]
					},
					{
						name: "Lead",
						midi: true,
						echo: false,
						takes: [
							{
								id: 0,
								name: "Lead",
								type: "Audio",
								audiomute: true,
								midimute: true
							}
						]
					},
					{
						name: "Bass",
						midi: true,
						echo: false,
						takes: [
							{
								id: 0,
								name: "Intro",
								type: "Audio",
								audiomute: true,
								midimute: true,
								associated_midi_takes: [2,3]
							},
							{
								id: 1,
								name: "Main line",
								type: "Audio",
								audiomute: false,
								midimute: true,
								associated_midi_takes: [3,4,5]
							},
							{
								id: 2,
								name: "Intro (MIDI)",
								type: "Midi",
								audiomute: false,
								midimute: true
							},
							{
								id: 3,
								name: "Filter",
								type: "Midi",
								audiomute: false,
								midimute: true
							},
							{
								id: 4,
								name: "Main line (MIDI)",
								type: "Midi",
								audiomute: false,
								midimute: true
							},
							{
								id: 5,
								name: "Envelope decay",
								type: "Midi",
								audiomute: false,
								midimute: true
							},
						]
					}
				]
			},
			{
				name: "Weird MIDI-Only thingy",
				chains: [
					{
						name: "Something",
						midi: true,
						echo: false,
						takes: [
							{
								id: 0,
								name: "Some take",
								type: "Midi",
								audiomute: false,
								midimute: true,
								associated_midi_takes: []
							},
						]
					}
				]
			},
			{
				name: "Guitar",
				chains: [
					{
						name: "Distorted",
						midi: false,
						echo: false,
						takes: [
							{
								id: 0,
								name: "Rhythm Djents",
								type: "Audio",
								audiomute: false,
								midimute: true,
								associated_midi_takes: []
							},
							{
								id: 1,
								name: "Rhythm Djents 2",
								type: "Audio",
								audiomute: false,
								midimute: true,
								associated_midi_takes: []
							},
						]
					},
					{
						name: "Clean",
						midi: false,
						echo: false,
						takes: [
							{
								id: 0,
								name: "Solo",
								type: "Audio",
								audiomute: false,
								midimute: true,
								associated_midi_takes: []
							},
						]
					}
				]
			}
		]
	}},
	methods: {
		async bpm_beats_changed() {
			console.log("bpms / beats have changed");
			console.log(this.loop_settings);

			var patch = await fetch("http://localhost:8000/api/song", {
				method: 'PATCH',
				headers: { 'Content-Type': 'application/json' },
				redirect: 'follow',
				mode: 'cors',
				body: JSON.stringify({
					"loop_length": 60 * this.loop_settings.beats / this.loop_settings.bpm,
					"beats": this.loop_settings.beats
				})
			});
		},
		async add_synth_clicked() {
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
				path = post.headers.get('Location');
				console.log(path);

				var response = await fetch("http://localhost:8000"+path);
				if (response.status !== 200) {
					console.log("whoopsie :o");
					return;
				}

				json = await response.json();
				console.log(json);
				if (this.synths.find(x => x.id === json.id) === undefined) {
					this.synths.push(json);
				}
			}
			else {
				alert("Failed to create synth!");
			}
		}
	},
	computed: {
		n_takes: function() {
			var total = 0;
			for (var synth of this.synths) {
				for (var chain of synth.chains) {
					total += chain.takes.length;
				}
			}
			return total;
		}
	}
})

async function async_main() {
	await init();
	timeloop();
	await mainloop(); // never returns
}

function now() {
	return new Date().getTime() / 1000.0;
}

async function init() {
	var response = await fetch("http://localhost:8000/api/synths");
	var json = await response.json();

	for (let synth of json) {
		for (let chain of synth.chains) {
			chain.selected = false;
			for (let take of chain.takes) {
				take.selected = false;
			}
		}
	}

	app2.synths = json;

	var response2 = await fetch("http://localhost:8000/api/song");
	var song = await response2.json();
	app2.playback_time_offset = (song.transport_position || 0) - now();
	app2.loop_length = song.loop_length;
}

async function timeloop()
{
	const fps = 20;
	while(true) {
		app2.playback_time = now() + app2.playback_time_offset;
		await new Promise(r => setTimeout(r, 1000 / fps));
	}
}

async function mainloop()
{
	var next_update_id = 0;
	while (true) {
		var begin_time = now();
		var response = await fetch("http://localhost:8000/api/updates?since=" + next_update_id + "&seconds=10");
		var update_list = await response.json();
		var duration = now() - begin_time;

		for (var update of update_list) {
			if (Number.isInteger(update.id))
			{
				next_update_id = Math.max(next_update_id, update.id + 1);
				if (update.action.synths !== undefined) {
					try {
						apply_patch(update.action);
					}
					catch (e) {
						console.log("Error while applying update")
						console.log(e);
					}
				}
				if (update.action.song !== undefined) {
					if (update.action.song.song_position !== undefined) { // only update the time when the answer was really polled.
						if (duration >= 0.1) {
							app2.playback_time_offset = update.action.song.transport_position - now();
						}
						else {
							console.log("ignoring timestamp which likely is stale");
						}
					}
					if (update.action.song.loop_length !== undefined) {
						app2.loop_length = update.action.song.loop_length;
					}
				}
			}
		}
	}
}

function apply_patch(patch) {
	helper(
		app2.synths, patch.synths, ["name"], {},
		[
			[
				"chains",
				["name", "midi", "audiomute", "midimute", "echo"], {'selected': false},
				[
					["takes", ["name", "type", "state", "associated_midi_takes", "muted", "muted_scheduled", "playing_since", "duration"], {'selected': false}, []]
				]
			]
		]
	);
}

function helper(array_to_patch, patch_array, props, additional_props, arrayprops) {
	for (let patch of patch_array) {
		if (patch.delete === true) {
			let index = array_to_patch.findIndex( x => x.id === patch.id );
			if (index != -1) {
				array_to_patch.splice(index, 1);
			}
		}
		else
		{
			let object_to_patch = array_to_patch.find( x => x.id === patch.id );
			let push = false;
			if (object_to_patch === undefined) {
				push = true;
				object_to_patch = { id: patch.id, created_by_patch: true };
				for (let arrayprop of arrayprops) {
					object_to_patch[arrayprop[0]] = [];
				}
			}

			for (let prop of props) {
				if (patch[prop] !== undefined) {
					object_to_patch[prop] = patch[prop];
				}
			}

			for (let arrayprop of arrayprops) {
				if (patch[arrayprop[0]] !== undefined) {
					helper(object_to_patch[arrayprop[0]], patch[arrayprop[0]], arrayprop[1], arrayprop[2], arrayprop[3])
				}
			}

			if (push) {
				for (const [key, value] of Object.entries(additional_props)) {
					object_to_patch[key] = value;
				}

				array_to_patch.push(object_to_patch);
			}
		}
	}

}

async_main();
