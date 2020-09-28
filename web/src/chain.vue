<script>
module.exports = {
	props: ['name', 'takes'],
	methods: {
		showhidemidi: function(event) {
			this.show_midi = !this.show_midi;
		},
		toggle_audio: function(take) {
			take.audiomute = !take.audiomute;

			if (!take.audiomute && !take.midimute)
			{
				take.midimute = true;
				this.update_associated_midi_takes(take);
			}
		},
		toggle_midi: function(take) {
			take.midimute = !take.midimute;

			this.update_associated_midi_takes(take);
			
			if (!take.audiomute && !take.midimute)
			{
				take.audiomute = true;
			}
		},
		update_associated_midi_takes: function(take) {
			console.log(this);
			for (var id of take.associated_midi_takes) {
				var miditake = this.takes.find(t => t.id == id);
				console.log(miditake);
				miditake.midimute = take.midimute;
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
				<button v-on:click="showhidemidi">{{ show_midi ? 'hide midi' : 'show midi' }}</button>
				<div class="circle" style="background-color: blue"></div>
				<div class="circle"></div>
			</div>

	
			<div v-for="take in takes" v-bind:style="'overflow: hidden; margin-right: -0.5em; padding: 0; transition: max-height 125ms ease-out;' + ((show_midi || take.audio) ? 'max-height:2.5em' : 'max-height:0')">
			<take v-bind:reference="take" v-on:toggle_audio="toggle_audio" v-on:toggle_midi="toggle_midi" v-bind:name="take.name" v-bind:audio="take.audio" v-bind:midi="take.midi" v-bind:audiomute="take.audiomute" v-bind:midimute="take.midimute" play_audio="1"></take>
			</div>
		</div>
</template>

<style scoped>
</style>
