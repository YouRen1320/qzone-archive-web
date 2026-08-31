<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, useTemplateRef } from 'vue'
import { archiveMediaUrl } from '../services/archiveApi'

const props = defineProps<{ path: string }>()
const root = useTemplateRef<HTMLElement>('root')
const shouldLoad = ref(false)
const failed = ref(false)
let observer: IntersectionObserver | undefined

const isVideo = computed(() => /\.(mp4|m4v|mov|webm)$/i.test(props.path))
const url = computed(() => archiveMediaUrl(props.path))

// Media URLs are assigned only near the viewport, keeping large archives light on phones.
onMounted(() => {
  if (!('IntersectionObserver' in window) || !root.value) {
    shouldLoad.value = true
    return
  }
  observer = new IntersectionObserver((entries) => {
    if (entries.some((entry) => entry.isIntersecting)) {
      shouldLoad.value = true
      observer?.disconnect()
    }
  }, { rootMargin: '500px 0px' })
  observer.observe(root.value)
})

onBeforeUnmount(() => observer?.disconnect())
</script>

<template>
  <figure ref="root" class="archive-media" :class="{ 'is-video': isVideo }">
    <video v-if="shouldLoad && isVideo && !failed" :src="url" controls preload="metadata" playsinline @error="failed = true">
      当前浏览器无法播放这个视频。
    </video>
    <img v-else-if="shouldLoad && !failed" :src="url" alt="归档中的照片" loading="lazy" decoding="async" @error="failed = true" />
    <div v-else-if="failed" class="archive-media__state is-error" role="status">这个媒体文件暂时无法打开</div>
    <div v-else class="archive-media__state" aria-hidden="true">照片正在靠近</div>
  </figure>
</template>

<style scoped>
.archive-media { min-height: 180px; margin: 0; overflow: hidden; background: #c9c6bd; }
.archive-media img,
.archive-media video { display: block; width: 100%; height: 100%; min-height: 180px; max-height: 620px; object-fit: cover; }
.archive-media.is-video video { object-fit: contain; background: #171a18; }
.archive-media__state { display: grid; min-height: 180px; place-content: center; padding: 24px; color: #5d615d; text-align: center; }
.archive-media__state.is-error { color: #7d4036; }
</style>
