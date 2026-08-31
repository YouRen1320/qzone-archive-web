<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, useTemplateRef } from 'vue'
import type { ArchiveMediaHandle, ArchiveSession } from '../types/archive'

const props = defineProps<{
  path: string
  session: ArchiveSession
}>()

const root = useTemplateRef<HTMLElement>('root')
const handle = ref<ArchiveMediaHandle | null>(null)
const loading = ref(false)
const error = ref('')
let observer: IntersectionObserver | undefined

const isVideo = computed(() => /\.(mp4|m4v|mov|webm)$/i.test(props.path))
const mediaSize = computed(() => props.session.mediaSize(props.path))
const requiresConsent = computed(() => isVideo.value && (mediaSize.value ?? 0) > 200 * 1024 * 1024)
const sizeLabel = computed(() => mediaSize.value ? formatBytes(mediaSize.value) : '')

// Local ZIP entries become object URLs only when their media tile approaches the viewport.
async function load() {
  if (handle.value || loading.value) return
  loading.value = true
  error.value = ''
  try {
    handle.value = await props.session.openMedia(props.path)
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : '媒体文件无法打开'
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  if (requiresConsent.value) return
  if (!('IntersectionObserver' in window) || !root.value) {
    void load()
    return
  }
  observer = new IntersectionObserver((entries) => {
    if (entries.some((entry) => entry.isIntersecting)) {
      observer?.disconnect()
      void load()
    }
  }, { rootMargin: '500px 0px' })
  observer.observe(root.value)
})

onBeforeUnmount(() => {
  observer?.disconnect()
  handle.value?.release()
})

function formatBytes(value: number): string {
  if (value >= 1024 ** 3) return `${(value / 1024 ** 3).toFixed(1)} GB`
  if (value >= 1024 ** 2) return `${Math.round(value / 1024 ** 2)} MB`
  return `${Math.round(value / 1024)} KB`
}
</script>

<template>
  <figure ref="root" class="archive-media" :class="{ 'is-video': isVideo }">
    <video v-if="isVideo && handle" :src="handle.url" controls preload="metadata" playsinline>
      当前浏览器无法播放这个视频。
    </video>
    <img v-else-if="handle" :src="handle.url" alt="归档中的照片" loading="lazy" decoding="async" />
    <button
      v-else-if="requiresConsent && !loading && !error"
      class="archive-media__load pressable"
      type="button"
      @click="load"
    >
      <span>加载这段视频</span>
      <small>{{ sizeLabel }}，仅在本机解压</small>
    </button>
    <div v-else-if="loading" class="archive-media__state" role="status">正在从归档中取出媒体</div>
    <div v-else-if="error" class="archive-media__state is-error" role="status">{{ error }}</div>
    <div v-else class="archive-media__state" aria-hidden="true">照片正在靠近</div>
  </figure>
</template>

<style scoped>
.archive-media {
  min-height: 180px;
  margin: 0;
  overflow: hidden;
  background: #c9c6bd;
}

.archive-media img,
.archive-media video {
  display: block;
  width: 100%;
  height: 100%;
  min-height: 180px;
  max-height: 620px;
  object-fit: cover;
}

.archive-media.is-video video {
  object-fit: contain;
  background: #171a18;
}

.archive-media__state,
.archive-media__load {
  display: grid;
  min-height: 180px;
  place-content: center;
  padding: 24px;
  border: 0;
  background: transparent;
  color: #5d615d;
  font: inherit;
  text-align: center;
}

.archive-media__load {
  width: 100%;
  cursor: pointer;
  color: #171a18;
}

.archive-media__load span {
  font-weight: 650;
}

.archive-media__load small {
  margin-top: 5px;
  color: #666a66;
}

.archive-media__state.is-error {
  color: #7d4036;
}
</style>
