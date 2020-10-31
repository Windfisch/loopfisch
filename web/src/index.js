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

	function foo() {
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

var app2 = new Vue({
	el: '#app',
	data: {
		message: "Hello World",
		count: 0,
		user_id: "<not registered yet>",
		synths: [
			{
				name: "Deepmind 13",
				id: 0,
				chains: [
					{
						name: "Pad",
						takes: [
							{
								id: 0,
								name: "Flausch",
								audio: true,
								midi: true,
								audiomute: true,
								midimute: true,
								associated_midi_takes: [1]
							},
							{
								id: 3,
								name: "gefiltertes Flausch",
								audio: true,
								midi: true,
								audiomute: false,
								midimute: true,
								associated_midi_takes: [1,2]
							},
							{
								id: 1,
								name: "Flausch (MIDI)",
								audio: false,
								midi: true,
								audiomute: false,
								midimute: true
							},
							{
								id: 2,
								name: "Filter controller",
								audio: false,
								midi: true,
								audiomute: false,
								midimute: true
							},
						]
					},
					{
						name: "Lead",
						takes: [
							{
								id: 0,
								name: "Lead",
								audio: true,
								midi: true,
								audiomute: true,
								midimute: true
							}
						]
					},
					{
						name: "Bass",
						takes: [
							{
								id: 0,
								name: "Intro",
								audio: true,
								midi: true,
								audiomute: true,
								midimute: true,
								associated_midi_takes: [2,3]
							},
							{
								id: 1,
								name: "Main line",
								audio: true,
								midi: true,
								audiomute: false,
								midimute: true,
								associated_midi_takes: [3,4,5]
							},
							{
								id: 2,
								name: "Intro (MIDI)",
								audio: false,
								midi: true,
								audiomute: false,
								midimute: true
							},
							{
								id: 3,
								name: "Filter",
								audio: false,
								midi: true,
								audiomute: false,
								midimute: true
							},
							{
								id: 4,
								name: "Main line (MIDI)",
								audio: false,
								midi: true,
								audiomute: false,
								midimute: true
							},
							{
								id: 5,
								name: "Envelope decay",
								audio: false,
								midi: true,
								audiomute: false,
								midimute: true
							},
						]
					}
				]
			},
			{
				name: "Weird MIDI-Only thingy",
				chains: [
					{
						name: "Something",
						takes: [
							{
								id: 0,
								name: "Some take",
								audio: false,
								midi: true,
								audiomute: false,
								midimute: true,
								associated_midi_takes: []
							},
						]
					}
				]
			},
			{
				name: "Guitar",
				chains: [
					{
						name: "Distorted",
						takes: [
							{
								id: 0,
								name: "Rhythm Djents",
								audio: true,
								midi: false,
								audiomute: false,
								midimute: true,
								associated_midi_takes: []
							},
							{
								id: 1,
								name: "Rhythm Djents 2",
								audio: true,
								midi: false,
								audiomute: false,
								midimute: true,
								associated_midi_takes: []
							},
						]
					},
					{
						name: "Clean",
						takes: [
							{
								id: 0,
								name: "Solo",
								audio: true,
								midi: false,
								audiomute: false,
								midimute: true,
								associated_midi_takes: []
							},
						]
					}
				]
			}
		]
	},
	methods: {
	}
})

console.log(app2.synths);

async function init() {
	var response = await fetch("http://localhost:8000/api/synths");
	var json = await response.json();
	console.log("fnord");
	console.log(json);
	app2.synths = json;
	console.log(app2.synths);
}

init();

function mainloop()
{

}

window.setTimeout(mainloop, 100);
