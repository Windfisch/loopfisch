<script>
module.exports = {
	props: ['name'],
	methods: {
		showhidemidi: function(event) {
			this.show_midi = !this.show_midi;
		},
		toggle_audio: function(take) {
			take.audiomute = !take.audiomute;

			if (!take.audiomute && !take.midimute)
			{
				take.midimute = true;
				for (var miditake of take.associated_midi_takes) {
					miditake.midimute = take.midimute;
				}
			}
		},
		toggle_midi: function(take) {
			take.midimute = !take.midimute;

			for (var miditake of take.associated_midi_takes) {
				miditake.midimute = take.midimute;
			}
			
			if (!take.audiomute && !take.midimute)
			{
				take.audiomute = true;
			}
		}
	},
	data: function() {
		var take3 = { name: "Flausch (MIDI)", audio: false, midi: true, audiomute: false, midimute: true};
		var take4 = { name: "Filter control", audio: false, midi: true, audiomute: false, midimute: false};
		var take1 = { name: "Flausch", audio: true, midi: true, audiomute: true, midimute: false, associated_midi_takes:[take3] };
		var take2 = { name: "gefiltertes Flausch", audio: true, midi: true, audiomute: false, midimute: false, associated_midi_takes:[take3, take4]};
		return {
			show_midi: "true",
			takes: [
				take1, take2, take3, take4
			]
		}
	}
}
</script>

<template>
		<div class="chainbox">
			<div class="header">
				<h1>{{name}}</h1>
				<div style="flex-grow: 2"></div>
				<div v-on:click="showhidemidi">{{ show_midi ? '[hide midi]' : '[show midi]' }}</div>
				<div class="circle" style="background-color: blue"></div>
				<div class="circle"></div>
			</div>

	
			<div v-for="take in takes" v-bind:style="'overflow: hidden; margin-right: -0.5em; padding: 0; transition: max-height 125ms ease-out;' + ((show_midi || take.audio) ? 'max-height:2.5em' : 'max-height:0')">
			<take  v-bind:reference="take" v-on:toggle_audio="toggle_audio" v-on:toggle_midi="toggle_midi" v-bind:name="take.name" v-bind:audio="take.audio" v-bind:midi="take.midi" v-bind:audiomute="take.audiomute" v-bind:midimute="take.midimute" play_audio="1"></take>
			</div>
		</div>
</template>

<style scoped>
</style>
