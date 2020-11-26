<script>
module.exports = {
	props: ['value'],
	watch: {
		value: function(val) {
			this.bpm = val.bpm;
			this.beats = val.beats;
		}
	},
	methods: {
		set_or_edit() {
			if (this.bpm_editing) {
				this.$emit("input", { bpm: parseFloat(this.bpm), beats: parseInt(this.beats) });
			}
			this.bpm_editing = !this.bpm_editing
			console.log(this.bpm_editing);
		},
		tap() {
			var now = Date.now() / 1000.0;

			if (this.taps.length > 0) {
				if (now > this.taps[this.taps.length-1] + 5) {
					this.taps = [];
				}
			}
			if (this.taps.length >= 2) {
				var time_between_taps = (this.taps[this.taps.length-1] - this.taps[0]) / (this.taps.length-1);
				if (now >= this.taps[this.taps.length-1] + 2*time_between_taps) {
					this.taps = []
				}
			}

			this.taps.push(now);

			while (this.taps.length > 8) {
				this.taps.shift();
			}

			if (this.taps.length >= 2) {
				var time_between_taps = (this.taps[this.taps.length-1] - this.taps[0]) / (this.taps.length-1);
				this.bpm = 60 / time_between_taps;
			}

			this.bpm_editing = true;
		}
	},
	data: function() {
		return {
			bpm: this.value.bpm,
			beats: this.value.beats,
			bpm_editing: false,
			taps: []
		}
	}
}
</script>

<template>
	<div>
		<input v-model="bpm" v-bind:disabled="!bpm_editing" type="number" min="1" max="999"/> bpm x
		<input v-model="beats" v-bind:disabled="!bpm_editing" type="number" min="1" max="99"/> beats
		<button v-on:click="set_or_edit">{{ bpm_editing ? "Set" : "Edit" }}</button>
		<button v-on:mousedown="tap">Tap</button>
	</div>
</template>

<style scoped>
</style>

