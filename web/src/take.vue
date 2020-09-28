<script>
module.exports = {
	props: ['name', 'audio', 'midi', 'audiomute', 'midimute', 'play_audio', 'play_progress', 'play_blinking', 'reference' ],
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
				<pie width='1.5em' v-bind:color="play_audio ? 'red' : 'blue'"></pie>
				<div>{{name}}</div>
				<div style="flex-grow: 2"></div>
				<img v-on:click="toggle_audio" v-if="audio" v-bind:src="'audio2' + (audiomute ? 'g' : '') + '.svg'" style="height: 75%" />
				<img v-on:click="toggle_midi" v-if="midi" v-bind:src="'midi2' + (midimute ? 'g' : '') + '.svg'" style="height: 75%" />
			</div>
</template>

<style scoped>
</style>
