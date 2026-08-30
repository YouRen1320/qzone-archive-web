<script setup lang="ts">
// The rail exposes the stable three-step model while detailed backend phases remain announced elsewhere.
defineProps<{
  current: number
  attention?: boolean
}>()

const steps = ['安全登录', '临时归档', '下载并清理']
</script>

<template>
  <ol class="step-rail" aria-label="归档进度">
    <li
      v-for="(step, index) in steps"
      :key="step"
      :class="{
        active: current === index + 1,
        done: current > index + 1,
        attention: attention && current === index + 1,
      }"
      :aria-current="current === index + 1 ? 'step' : undefined"
    >
      <span class="step-marker" aria-hidden="true">{{ current > index + 1 ? '✓' : index + 1 }}</span>
      <b class="step-label">{{ step }}</b>
      <span v-if="attention && current === index + 1" class="sr-only">需要处理后继续</span>
    </li>
  </ol>
</template>
