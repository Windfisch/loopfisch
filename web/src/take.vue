<script>
export default {
	props: ['name', 'audio', 'midi', 'play_progress', 'play_blinking', 'reference', 'selected' ],
	computed: {
		midimute: function() {
			if (this.reference.type == "Audio") {
				var result = true;
				for (var id of this.reference.associated_midi_takes) {
					var miditake = this.$parent.model.takes.find(t => t.id == id);
					result = result && miditake.muted;
				}
				return result;
			}
			else {
				return this.reference.muted;
			}
		},
		audiomute: function() {
			return this.reference.type == "Audio" ? this.reference.muted : true;
		},
		playback_progress: function () {
			return ((this.$root.playback_time - this.reference.playing_since) % this.reference.duration) / this.reference.duration;
		},
		pie_color: function () {
			if (!this.audiomute) {
				return "red";
			}
			else if (!this.midimute) {
				return "blue";
			}
			else {
				return "white";
			}
		}
	},
	methods: {
		toggle_audio: function(event) {
			this.$emit('toggle_audio', this.reference);
		},
		toggle_midi: function(event) {
			this.$emit('toggle_midi', this.reference);
		},
	}
}
</script>

<template>
			<div class="takebox" :class="{'miditake': !audio, 'audiotake': audio, 'selected': selected}" >
				<div style="width:2em; margin: 0; padding:0; text-align:center">
					<pie class="blinking" width='0.75em' value=1 v-if="reference.state=='Waiting'" v-bind:color="pie_color"></pie>
					<pie width='0.75em' value=1 v-else-if="reference.state=='Recording'" v-bind:color="pie_color"></pie>
					<pie width='1.5em' v-bind:value="playback_progress" v-else v-bind:color="pie_color"></pie>
				</div>
				<div>{{name}} ({{reference.id}}), {{reference.state}}</div>
				<div style="flex-grow: 2"></div>
				<img v-on:click="toggle_audio" v-if="audio" v-bind:src="'audio2' + (audiomute ? 'g' : '') + '.svg'" style="height: 75%" />
				<img v-on:click="toggle_midi" v-if="midi" v-bind:src="'midi2' + (midimute ? 'g' : '') + '.svg'" style="height: 75%" />
			</div>
</template>

<style scoped>
</style>
