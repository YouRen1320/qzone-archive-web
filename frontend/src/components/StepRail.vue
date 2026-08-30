<script setup lang="ts">
// The rail gives a stable mental model while the server transitions through detailed phases.
defineProps<{
  current: number
}>()

const steps = ['安全登录', '临时归档', '下载并清理']
</script>

<template>
  <ol class="step-rail" aria-label="归档进度">
    <li v-for="(step, index) in steps" :key="step" :class="{ active: current === index + 1, done: current > index + 1 }">
      <span>{{ current > index + 1 ? '✓' : index + 1 }}</span>
      <b>{{ step }}</b>
    </li>
  </ol>
</template>
