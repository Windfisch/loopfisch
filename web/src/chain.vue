<script>
import {TakeModel} from './model.js';
export default {
	props: ['name', 'takes', 'midi', 'id', 'synthid', 'model', 'selected'],
	methods: {
		async new_take(type) { // FIXME these should be methods of ChainModel
			var post = await fetch(
				"http://localhost:8000/api/synths/" + this.synthid
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
				if (this.model.takes.find(x => x.id === json.id) === undefined) {
					this.model.takes.push(new TakeModel(json));
				}
			}
			else {
				alert("Failed to create take!");
			}
		},
		async record_audio() {
			if (this.has_recording_takes) {
				await this.stop_recording();
			}
			else {
				await this.new_take("Audio");
			}
		},
		async record_midi() {
			if (this.has_recording_takes) {
				await this.stop_recording();
			}
			else {
				await this.new_take("Midi");
			}
		},
		async stop_recording() {
			for (var take of this.takes.filter(t => t.state === "Recording")) {
				fetch(
					"http://localhost:8000/api/synths/" + this.synthid
					+ "/chains/" + this.id +
					"/takes/" + take.id +
					"/finish_recording",
					{ method: 'POST', mode: 'cors' }
				);
			}
		},
		has_audio_takes() {
			return this.takes.filter(t => t.type === "Audio").length > 0;
		},
		has_midi_takes() {
			return this.midi;
		},
		needs_show_midi_button() {
			return this.has_midi_takes() && this.has_audio_takes()
		},
		showhidemidi: function(event) {
			this.show_midi = !this.show_midi;
		},
		toggle_audio: function(take) {
			take.muted = !take.muted;
			
			if (!take.audiomute && !take.midimute) {
				this.update_associated_midi_takes(take, true);
			}
			
			this.send_mute_patch(take);
		},
		toggle_midi: function(take) {
			if (take.type == "Audio") {
				this.update_associated_midi_takes(take, !this.read_associated_midi_takes(take));
				if (!take.audiomute && !take.midimute) {
					take.muted = true;
				}
			}
			else {
				take.muted = !take.muted;
			}
			this.send_mute_patch(take);
		},
		toggle_echo: async function() {
			console.log(this.model.echo);
			this.model.echo = !this.model.echo;

			for (var chain of this.$parent.model.chains) {
				if (chain != this.model) {
					chain.echo = false;
				}
			}
			
			var patch = [];
			for (var chain of this.$parent.model.chains) {
				patch.push({id: chain.id, echo: chain.echo});
			}

			var req = await fetch(
				"http://localhost:8000/api/synths/" + this.synthid
				+ "/chains", {
				method: 'PATCH',
				headers: { 'Content-Type': 'application/json' },
				redirect: 'follow',
				mode: 'cors',
				body: JSON.stringify(patch)
			});
		},
		send_mute_patch: async function(take) {
			var synthid = this.$parent.model.id;
			var chainid = this.model.id;

			var patch = [];
			patch.push({id: take.id, muted: take.muted});
			for (var id of take.associated_midi_takes) {
				var miditake = this.takes.find(t => t.id == id);
				patch.push({id: id, muted: miditake.muted});
			}

			console.log("sending patch");
			console.log(patch);

			var req = await fetch(
				"http://localhost:8000/api/synths/" + this.synthid
				+ "/chains/" + this.id + "/takes", {
				method: 'PATCH',
				headers: { 'Content-Type': 'application/json' },
				redirect: 'follow',
				mode: 'cors',
				body: JSON.stringify(patch)
			});
			console.log(req.status);
		},
		read_associated_midi_takes: function(take) {
			var result = true;
			for (var id of take.associated_midi_takes) {
				var miditake = this.takes.find(t => t.id == id);
				result = result && miditake.muted;
			}
			return result;
		},
		update_associated_midi_takes: function(take, value) {
			for (var id of take.associated_midi_takes) {
				var miditake = this.takes.find(t => t.id == id);
				miditake.muted = value;
			}
		}
	},
	computed: {
		has_recording_takes: function() {
			return this.takes.filter(t => t.state === "Recording").length > 0;
		}
	},
	data: function() {
		return {
			show_midi: true
		}
	}
}
</script>

<template>
		<div class="chainbox" :class="{ 'selected': selected == true }">
			<div class="header">
				<h1>{{name}}</h1>
				<div style="flex-grow: 2"></div>
				<button v-bind:style="'background-color: ' + (model.echo ? 'lightpink;' : 'white;')" v-on:click="toggle_echo">echo</button>
				<button v-if="needs_show_midi_button()" v-on:click="showhidemidi">{{ show_midi ? 'hide midi' : 'show midi' }}</button>
				<button v-on:click="record_audio">{{ has_recording_takes ? "stop rec" : "rec audio" }}</button>
				<button v-on:click="record_midi">{{ has_recording_takes ? "stop rec" : "rec midi" }}</button>
			</div>

	
			<div v-for="take in takes" v-bind:style="'overflow: hidden; margin-right: -0.5em; padding: 0; transition: max-height 125ms ease-out;' + ((show_midi || take.type==='Audio') ? 'max-height:2.5em' : 'max-height:0')">
			<take v-bind:reference="take" v-on:toggle_audio="toggle_audio" v-on:toggle_midi="toggle_midi" v-bind:name="take.name" v-bind:audio="take.type==='Audio'" v-bind:midi="midi" v-bind:selected="take.selected"></take>
			</div>
		</div>
</template>

<style scoped>
</style>
