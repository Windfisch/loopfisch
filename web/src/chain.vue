<script>
import {TakeModel} from './model.js';
export default {
	props: ['name', 'takes', 'midi', 'id', 'synthid', 'model', 'selected'],
	methods: {
		showhidemidi: function(event) {
			this.show_midi = !this.show_midi;
		},
		needs_show_midi_button() {
			return this.has_midi_takes() && this.has_audio_takes()
		},
		has_audio_takes() {
			return this.takes.filter(t => t.type === "Audio").length > 0;
		},
		has_midi_takes() {
			return this.midi;
		},
		toggle_audiomute(take) {
			this.model.set_take_audiomute(take, !this.model.get_take_audiomute(take));
		},
		toggle_midimute(take) {
			this.model.set_take_midimute(take, !this.model.get_take_midimute(take));
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
				<button v-bind:style="'background-color: ' + (model.echo ? 'lightpink;' : 'white;')" v-on:click="model.set_echo(!model.echo)">echo</button>
				<button v-if="needs_show_midi_button()" v-on:click="showhidemidi">{{ show_midi ? 'hide midi' : 'show midi' }}</button>
				<button v-on:click="model.toggle_record_audio()">{{ model.has_recording_takes() ? "stop rec" : "rec audio" }}</button>
				<button v-on:click="model.toggle_record_midi()">{{ model.has_recording_takes() ? "stop rec" : "rec midi" }}</button>
			</div>

			<div v-for="take in takes" v-bind:style="'overflow: hidden; margin-right: -0.5em; padding: 0; transition: max-height 125ms ease-out;' + ((show_midi || take.type==='Audio') ? 'max-height:2.5em' : 'max-height:0')">
			<take v-bind:model="take" v-on:toggle_audio="toggle_audiomute(take)" v-on:toggle_midi="toggle_midimute(take)" v-bind:name="take.name" v-bind:audio="take.type==='Audio'" v-bind:midi="midi" v-bind:selected="take.selected"></take>
			</div>
		</div>
</template>
