<!-- shamelessly stolen and adapted from https://stackoverflow.com/questions/45050119/click-to-edit-text-field-with-vue/51560218#51560218 -->
<template>
  <div style="display:inline-block;">
    <input type="text"
           v-if="edit"
           :value="valueLocal"
           @blur="valueLocal = $event.target.value; edit = false; $emit('input', valueLocal);"
           @keyup.enter="valueLocal = $event.target.value; edit = false; $emit('input', valueLocal);"
		   @focus="$event.target.select();"
           v-focus=""
             />
        <p v-else="" @dblclick="edit = true;">
          {{valueLocal}}
        </p>
    </div>
</template>

<script>
  export default {

  props: ['value'],

  data () {
  return {
      edit: false,
      valueLocal: this.value
    }
  },

  watch: {
    value: function() {
      this.valueLocal = this.value;
    }
  },

  directives: {
    focus: {
      inserted (el) {
        el.focus()
      }
    }
  }

}
</script>
