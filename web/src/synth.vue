<script>
module.exports = {
	props: ['name', 'chains', 'id', 'model'],
	methods: {
		async add_chain_clicked() {
			var post = await fetch("http://localhost:8000/api/synths/" + this.id + "/chains", {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				redirect: 'follow',
				mode: 'cors',
				body: JSON.stringify({
					"name": "New Chain"
				})
			});
			if (post.status == 201) {
				path = post.headers.get('Location');
				console.log(path);

				var response = await fetch("http://localhost:8000"+path);
				if (response.status !== 200) {
					console.log("whoopsie :o");
					return;
				}

				json = await response.json();
				console.log(json);
				if (this.model.chains.find(x => x.id === json.id) === undefined) {
					this.model.chains.push(json);
				}
				console.log(this.model);
			}
			else {
				alert("Failed to create chain!");
			}

		}
	}
}
</script>

<template>
	<div class="synthbox">
		<div class="header">
			<h1>{{name}} ({{id}})</h1>
			<div style="flex-grow: 2"></div>
			<button v-on:click="add_chain_clicked">Add chain</button>
		</div>
		<div>
			<chain v-for="chain in chains" v-bind:midi="chain.midi" v-bind:name="chain.name" v-bind:takes="chain.takes" />
		</div>
	</div>
</template>

<style scoped>
</style>
