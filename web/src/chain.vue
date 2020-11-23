<script>
module.exports = {
	props: ['name', 'takes', 'midi', 'id', 'synthid', 'model'],
	methods: {
		async record_audio() {
			await this.new_take("Audio");
		},
		async new_take(type) {
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
					this.model.takes.push(json);
				}
			}
			else {
				alert("Failed to create take!");
			}
		},
		async record_midi() {
			await this.new_take("Midi");
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
	data: function() {
		return {
			show_midi: true
		}
	}
}
</script>

<template>
		<div class="chainbox">
			<div class="header">
				<h1>{{name}}</h1>
				<div style="flex-grow: 2"></div>
				<button v-if="needs_show_midi_button()" v-on:click="showhidemidi">{{ show_midi ? 'hide midi' : 'show midi' }}</button>
				<button v-on:click="record_audio">rec audio</button>
				<button v-on:click="record_midi">rec midi</button>
			</div>

	
			<div v-for="take in takes" v-bind:style="'overflow: hidden; margin-right: -0.5em; padding: 0; transition: max-height 125ms ease-out;' + ((show_midi || take.type==='Audio') ? 'max-height:2.5em' : 'max-height:0')">
			<take v-bind:reference="take" v-on:toggle_audio="toggle_audio" v-on:toggle_midi="toggle_midi" v-bind:name="take.name" v-bind:audio="take.type==='Audio'" v-bind:midi="midi" v-bind:audiomute="take.audiomute" v-bind:midimute="take.midimute" play_audio="1"></take>
			</div>
		</div>
</template>

<style scoped>
</style>
