<script setup lang="ts">
import { ref, toRef, watch } from 'vue'
import { useSceneFx } from '../composables/useSceneFx'
import type { JiangnanSceneId } from '../data/sceneSpecs'

const props = withDefaults(
  defineProps<{
    scene: JiangnanSceneId
    progress?: number
  }>(),
  { progress: 0 },
)

const emit = defineEmits<{
  readyChange: [ready: boolean]
}>()

const host = ref<HTMLElement | null>(null)
const canvas = ref<HTMLCanvasElement | null>(null)

// This component owns only the decorative surface; the composable owns every scene side effect and cleanup.
const { rendererReady } = useSceneFx({
  host,
  canvas,
  scene: toRef(props, 'scene'),
  progress: toRef(props, 'progress'),
})

watch(rendererReady, (ready) => emit('readyChange', ready), { immediate: true })
</script>

<template>
  <div ref="host" class="water-lens" aria-hidden="true">
    <canvas ref="canvas" class="water-lens__canvas"></canvas>
  </div>
</template>

<style scoped>
.water-lens,
.water-lens__canvas {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
}

.water-lens__canvas {
  display: block;
}
</style>
