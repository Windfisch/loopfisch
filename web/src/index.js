import pie from './pie.vue';
import take from './take.vue';
import chain from './chain.vue';
import synth from './synth.vue';
import Vue from 'vue';
import axios from 'axios';

Vue.component('pie', pie);
Vue.component("take", take);
Vue.component("chain", chain);
Vue.component("synth", synth);

var app2 = new Vue({
	el: '#app',
	data: {
		message: "Hello World",
		count: 0,
		user_id: "<not registered yet>",
		synths: [
			{
				name: "Deepmind 13",
				id: 0,
				chains: [
					{
						name: "Pad",
						takes: [
							{
								id: 0,
								name: "Flausch",
								audio: true,
								midi: true,
								audiomute: true,
								midimute: true,
								associated_midi_takes: [1]
							},
							{
								id: 3,
								name: "gefiltertes Flausch",
								audio: true,
								midi: true,
								audiomute: false,
								midimute: true,
								associated_midi_takes: [1,2]
							},
							{
								id: 1,
								name: "Flausch (MIDI)",
								audio: false,
								midi: true,
								audiomute: false,
								midimute: true
							},
							{
								id: 2,
								name: "Filter controller",
								audio: false,
								midi: true,
								audiomute: false,
								midimute: true
							},
						]
					},
					{
						name: "Lead",
						takes: [
							{
								id: 0,
								name: "Lead",
								audio: true,
								midi: true,
								audiomute: true,
								midimute: true
							}
						]
					},
					{
						name: "Bass",
						takes: [
							{
								id: 0,
								name: "Intro",
								audio: true,
								midi: true,
								audiomute: true,
								midimute: true,
								associated_midi_takes: [2,3]
							},
							{
								id: 1,
								name: "Main line",
								audio: true,
								midi: true,
								audiomute: false,
								midimute: true,
								associated_midi_takes: [3,4,5]
							},
							{
								id: 2,
								name: "Intro (MIDI)",
								audio: false,
								midi: true,
								audiomute: false,
								midimute: true
							},
							{
								id: 3,
								name: "Filter",
								audio: false,
								midi: true,
								audiomute: false,
								midimute: true
							},
							{
								id: 4,
								name: "Main line (MIDI)",
								audio: false,
								midi: true,
								audiomute: false,
								midimute: true
							},
							{
								id: 5,
								name: "Envelope decay",
								audio: false,
								midi: true,
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
						takes: [
							{
								id: 0,
								name: "Some take",
								audio: false,
								midi: true,
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
						takes: [
							{
								id: 0,
								name: "Rhythm Djents",
								audio: true,
								midi: false,
								audiomute: false,
								midimute: true,
								associated_midi_takes: []
							},
							{
								id: 1,
								name: "Rhythm Djents 2",
								audio: true,
								midi: false,
								audiomute: false,
								midimute: true,
								associated_midi_takes: []
							},
						]
					},
					{
						name: "Clean",
						takes: [
							{
								id: 0,
								name: "Solo",
								audio: true,
								midi: false,
								audiomute: false,
								midimute: true,
								associated_midi_takes: []
							},
						]
					}
				]
			}
		]
	},
	methods: {
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
				console.log(this.synths);
			}
			else {
				alert("Failed to create synth!");
			}

		}
	}
})

console.log(app2.synths);

async function async_main() {
	await init();
	await mainloop(); // never returns
}

async function init() {
	var response = await fetch("http://localhost:8000/api/synths");
	var json = await response.json();
	console.log("fnord");
	console.log(json);
	app2.synths = json;
	console.log(app2.synths);
}

async function mainloop()
{
	var next_update_id = 0;
	while (true) {
		console.log("polling for updates since " + next_update_id);
		var response = await fetch("http://localhost:8000/api/updates?since=" + next_update_id + "&seconds=10");
		var update_list = await response.json();

		console.log(response.status);
		console.log(update_list);

		for (var update of update_list) {
			console.log(update);
			if (Number.isInteger(update.id))
			{
				next_update_id = Math.max(next_update_id, update.id + 1);
				try {
					apply_patch(update.action);
				}
				catch (e) {
					console.log("Error while applying update")
					console.log(e);
				}
			}
		}
	}
}

function apply_patch(patch) {
	console.log("Applying patch");
	console.log(patch);

	helper(
		app2.synths, patch.synths, ["name"],
		[
			[
				"chains",
				["name"],
				[
					["takes", ["name", "muted", "muted_scheduled"], []]
				]
			]
		]
	);
}

function helper(array_to_patch, patch_array, props, arrayprops) {
	console.log("Patching");
	console.log(array_to_patch);
	console.log("patch is");
	console.log(patch_array);
	console.log("props:");
	console.log(props);
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
				console.log(prop, patch[prop]);
				if (patch[prop] !== undefined) {
					object_to_patch[prop] = patch[prop];
				}
			}

			for (let arrayprop of arrayprops) {
				console.log("arrayprop", arrayprop[0], patch[arrayprop[0]]);
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
