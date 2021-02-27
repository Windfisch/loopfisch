<script>
export default {
	props: ['name', 'audio', 'midi', 'play_progress', 'play_blinking', 'model', 'selected', 'index_in_chain'],
	computed: {
		midimute: function() {
			return this.model.parent_chain.get_take_midimute(this.model);
		},
		audiomute: function() {
			return this.model.parent_chain.get_take_audiomute(this.model);
		},
		playback_progress: function () {
			return ((this.$root.playback_time - this.model.playing_since) % this.model.duration) / this.model.duration;
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
		},
		letter: function () {
			return "asdfghjkl"[this.index_in_chain];
		}
	},
	methods: {
		toggle_audio: function(event) {
			this.$emit('toggle_audio', this.model);
		},
		toggle_midi: function(event) {
			this.$emit('toggle_midi', this.model);
		},
		change_name(new_name) {
			this.model.update_name(new_name);
		}
	}
}
</script>

<template>
			<div class="takebox" :class="{'miditake': !audio, 'audiotake': audio, 'selected': selected}" >
				<div style="width:2em; margin: 0; padding:0; text-align:center">
					<pie class="blinking" width='0.75em' value=1 v-if="model.state=='Waiting'" v-bind:color="pie_color"></pie>
					<pie width='0.75em' value=1 v-else-if="model.state=='Recording'" v-bind:color="pie_color"></pie>
					<pie width='1.5em' v-bind:value="playback_progress" v-else v-bind:color="pie_color"></pie>
				</div>
				<div style="flex-grow:2;">
					<div style='display:inline-block; background-color: #333; color: white; width: 1.2em; text-align: center; font-family: monospace'>{{letter}}</div> 
					<editlabel :value="name" v-on:input="change_name"/>
				</div>
				<img v-on:click="toggle_audio" v-if="audio" v-bind:src="'audio2' + (audiomute ? 'g' : '') + '.svg'" style="height: 75%" />
				<img v-on:click="toggle_midi" v-if="midi" v-bind:src="'midi2' + (midimute ? 'g' : '') + '.svg'" style="height: 75%" />
			</div>
</template>
