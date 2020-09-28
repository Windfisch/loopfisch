import pie from './pie.vue';
import take from './take.vue';
import chain from './chain.vue';
import synth from './synth.vue';
import Vue from 'vue';
import axios from 'axios';

Vue.component('pie', pie);
Vue.component("take", take);
Vue.component("chain", chain);
Vue.component("synth", synth);

var app = new Vue({
	el: '#app',
	data: {
		message: "Hello World",
		count: 0,
		user_id: "<not registered yet>"
	},
	methods: {
	}
})

function mainloop()
{

}

window.setTimeout(mainloop, 100);
