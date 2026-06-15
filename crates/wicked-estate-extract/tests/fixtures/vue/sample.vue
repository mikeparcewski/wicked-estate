<script setup>
import { ref, computed } from 'vue'

const count = ref(0)
const doubled = computed(() => count.value * 2)

function increment() {
  count.value++
}

function reset() {
  count.value = 0
}
</script>

<template>
  <div>
    <p>Count: {{ count }} (doubled: {{ doubled }})</p>
    <button @click="increment">+1</button>
    <button @click="reset">Reset</button>
    <ul>
      <li v-for="n in count" :key="n">Item {{ n }}</li>
    </ul>
  </div>
</template>

<style scoped>
button { margin: 4px; }
</style>
