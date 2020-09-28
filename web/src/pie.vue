<script>
module.exports = {
	props: {
		value: { type: Number, default: 0.2 },
		color: { type: String, default: "red" },
		bgcolor: { type: String },
		bgopacity: { type: Number, default: 0.15 },
		innerradius: { type: Number, default: 0 },
	}
}
</script>

<template>
		<svg viewBox="0 0 2 2">
			<circle r="1" cx="1" cy="1" v-bind:fill-opacity="bgopacity"  v-bind:fill="bgcolor ? bgcolor : color" />
			<circle v-bind:transform="(value < 0 ? 'scale(-1 1) translate(-2 0)' : '') +'rotate(-90 1 1)'" v-bind:r="0.5 + innerradius/2" cx="1" cy="1" fill-opacity="0"  v-bind:stroke="color" v-bind:stroke-width="1-innerradius" v-bind:stroke-dasharray="3.141592654*Math.abs(value) + ' ' + 4" />
		</svg>
</template>

<style scoped>
	div {
		display: flex;
		align-items: center
	}
</style>


