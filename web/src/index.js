import pie from './pie.vue';
import take from './take.vue';
import chain from './chain.vue';
import synth from './synth.vue';
import bpm from './bpm.vue';
import Vue from 'vue';
import axios from 'axios';

Vue.component('pie', pie);
Vue.component("take", take);
Vue.component("chain", chain);
Vue.component("synth", synth);
Vue.component("bpm", bpm);

var app2 = new Vue({
	el: '#app',
	data: function(){ return{
		playback_time: 0,
		message: "Hello World",
		count: 0,
		loop_settings: {
			bpm: 126,
			beats:8,
		},
		user_id: "<not registered yet>",
		synths: [
			{
				name: "Deepmind 13",
				id: 0,
				chains: [
					{
						name: "Pad",
						midi: true,
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
					"loop_length": 60 * this.loop_settings.beats / this.loop_settings.bpm
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
	app2.synths = json;

	var response2 = await fetch("http://localhost:8000/api/song");
	var song = await response2.json();
	app2.playback_time_offset = (song.song_position || 0) - now();
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
							app2.playback_time_offset = update.action.song.song_position - now();
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
		app2.synths, patch.synths, ["name"],
		[
			[
				"chains",
				["name", "midi", "audiomute", "midimute"],
				[
					["takes", ["name", "type", "state", "associated_midi_takes", "muted", "muted_scheduled"], []]
				]
			]
		]
	);
}

function helper(array_to_patch, patch_array, props, arrayprops) {
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
					helper(object_to_patch[arrayprop[0]], patch[arrayprop[0]], arrayprop[1], arrayprop[2])
				}
			}

			if (push) {
				array_to_patch.push(object_to_patch);
			}
		}
	}

}

async_main();
