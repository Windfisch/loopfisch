<script>
module.exports = {
	props: ['name', 'audio', 'midi', 'play_audio', 'play_progress', 'play_blinking', 'reference' ],
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
			return this.reference.muted;
		},
		playback_progress: function () {
			return (this.$root.playback_time % 4000) / 4000;
		},
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
			<div class="takebox" v-bind:style="audio ? '' : 'background-color: #ccf' ">
				<div style="width:2em; margin: 0; padding:0; text-align:center">
					<pie class="blinking" width='0.75em' value=1 v-if="reference.state=='Waiting'" v-bind:color="play_audio ? 'red' : 'blue'"></pie>
					<pie width='0.75em' value=1 v-else-if="reference.state=='Recording'" v-bind:color="play_audio ? 'red' : 'blue'"></pie>
					<pie width='1.5em' v-bind:value="playback_progress" v-else v-bind:color="play_audio ? 'red' : 'blue'"></pie>
				</div>
				<div>{{name}}, {{reference.state}}</div>
				<div style="flex-grow: 2"></div>
				<img v-on:click="toggle_audio" v-if="audio" v-bind:src="'audio2' + (audiomute ? 'g' : '') + '.svg'" style="height: 75%" />
				<img v-on:click="toggle_midi" v-if="midi" v-bind:src="'midi2' + (midimute ? 'g' : '') + '.svg'" style="height: 75%" />
			</div>
</template>

<style scoped>
</style>
