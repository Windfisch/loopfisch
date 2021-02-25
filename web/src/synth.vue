<script>
import {ChainModel} from './model.js';
export default {
	props: ['name', 'chains', 'id', 'model'],
}
</script>

<template>
	<div class="synthbox">
		<div class="header">
			<h1>{{name}} ({{id}})</h1>
			<div style="flex-grow: 2"></div>
			<button v-on:click="model.restart_transport()">Restart transport</button>
			<button v-on:click="model.add_chain()">Add chain</button>
		</div>
		<div>
			<chain v-for="chain in chains" v-bind:id="chain.id" v-bind:synthid="id" v-bind:midi="chain.midi" v-bind:name="chain.name" v-bind:takes="chain.takes" v-bind:model="chain" v-bind:selected="chain.selected" />
		</div>
	</div>
</template>

<style scoped>
</style>
