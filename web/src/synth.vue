<script>
module.exports = {
	props: ['name', 'chains'],
}
</script>

<template>
	<div class="synthbox">
		<h1>{{name}}</h1>
		<chain v-for="chain in chains" v-bind:name="chain.name" />
	</div>
</template>

<style scoped>
</style>
