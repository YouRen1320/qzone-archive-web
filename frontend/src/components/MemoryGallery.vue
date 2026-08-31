<script setup lang="ts">
import { nextTick, ref } from 'vue'

interface MemoryItem {
  source: string
  alt: string
  title: string
  width: number
  height: number
}

// These are clearly labelled atmosphere stills from the interface, never user archive previews.
const memories: MemoryItem[] = [
  { source: '/assets/jiangnan/memory-01-eaves.webp', alt: '屋檐边连续落下的雨滴', title: '屋檐 · 18:42', width: 1537, height: 1023 },
  { source: '/assets/jiangnan/memory-02-ripples.webp', alt: '河面上交叠的雨圈和窗影', title: '河面 · 18:47', width: 1536, height: 1024 },
  { source: '/assets/jiangnan/memory-03-stone.webp', alt: '潮湿石板上被水打碎的灯影', title: '石板 · 19:03', width: 1536, height: 1024 },
  { source: '/assets/jiangnan/memory-04-bamboo.webp', alt: '贴着白墙的竹叶和将落的水滴', title: '竹影 · 19:11', width: 1536, height: 1024 },
  { source: '/assets/jiangnan/memory-05-rope.webp', alt: '系在石柱上的旧船绳', title: '船绳 · 19:26', width: 1536, height: 1024 },
  { source: '/assets/jiangnan/memory-06-window.webp', alt: '旧木窗玻璃上的雨水', title: '旧窗 · 19:38', width: 1536, height: 1024 },
  { source: '/assets/jiangnan/memory-07-umbrella.webp', alt: '靠在潮湿木门边的合拢纸伞', title: '纸伞 · 19:46', width: 1536, height: 1024 },
  { source: '/assets/jiangnan/memory-08-teacup.webp', alt: '雨窗旁湿木台上的青瓷茶盏', title: '茶盏 · 20:02', width: 1536, height: 1024 },
]

const viewer = ref<HTMLDialogElement | null>(null)
const currentIndex = ref(0)
let opener: HTMLElement | null = null
let swipeStart: { x: number; y: number } | null = null

function showMemory(index: number) {
  currentIndex.value = (index + memories.length) % memories.length
}

async function openMemory(index: number, event: MouseEvent) {
  opener = event.currentTarget instanceof HTMLElement ? event.currentTarget : null
  showMemory(index)
  await nextTick()
  if (typeof viewer.value?.showModal === 'function') viewer.value.showModal()
  else viewer.value?.setAttribute('open', '')
  viewer.value?.focus({ preventScroll: true })
}

function closeViewer() {
  if (typeof viewer.value?.close === 'function') viewer.value.close()
  else viewer.value?.removeAttribute('open')
}

function restoreFocus() {
  opener?.focus({ preventScroll: true })
  opener = null
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'ArrowLeft') showMemory(currentIndex.value - 1)
  if (event.key === 'ArrowRight') showMemory(currentIndex.value + 1)
  if (event.key === 'Escape') {
    event.preventDefault()
    closeViewer()
  }
}

function startSwipe(event: PointerEvent) {
  swipeStart = { x: event.clientX, y: event.clientY }
}

function finishSwipe(event: PointerEvent) {
  if (!swipeStart) return
  const horizontal = event.clientX - swipeStart.x
  const vertical = event.clientY - swipeStart.y
  swipeStart = null
  if (Math.abs(horizontal) < 48 || Math.abs(horizontal) < Math.abs(vertical)) return
  showMemory(currentIndex.value + (horizontal < 0 ? 1 : -1))
}
</script>

<template>
  <div class="atmosphere-gallery">
    <div class="gallery-heading">
      <b>雨路取景</b>
      <span>界面氛围影像，不是你的归档内容</span>
    </div>
    <ol class="memory-deck" aria-label="八张雨路氛围影像">
      <li v-for="(memory, index) in memories" :key="memory.source">
        <button type="button" :aria-label="`打开${memory.title}影像`" @click="openMemory(index, $event)">
          <figure>
            <img
              :src="memory.source"
              :width="memory.width"
              :height="memory.height"
              :alt="memory.alt"
              loading="lazy"
              decoding="async"
            />
            <figcaption>{{ memory.title }}</figcaption>
          </figure>
        </button>
      </li>
    </ol>

    <dialog
      ref="viewer"
      class="memory-viewer"
      tabindex="-1"
      aria-labelledby="memory-viewer-title"
      @close="restoreFocus"
      @keydown="handleKeydown"
      @click.self="closeViewer"
    >
      <div class="memory-viewer-shell">
        <button class="memory-viewer-close" type="button" aria-label="关闭影像" @click="closeViewer">关闭</button>
        <figure class="memory-viewer-figure">
          <img
            :src="memories[currentIndex].source"
            :width="memories[currentIndex].width"
            :height="memories[currentIndex].height"
            :alt="memories[currentIndex].alt"
            @pointerdown="startSwipe"
            @pointerup="finishSwipe"
            @pointercancel="swipeStart = null"
          />
          <figcaption>
            <p class="memory-viewer-kicker">
              <span>雨路取景</span>
              <span>{{ String(currentIndex + 1).padStart(2, '0') }} / {{ String(memories.length).padStart(2, '0') }}</span>
            </p>
            <h3 id="memory-viewer-title">{{ memories[currentIndex].title }}</h3>
            <p>{{ memories[currentIndex].alt }}</p>
            <small>这是界面氛围影像，不会读取或展示你的归档照片。</small>
          </figcaption>
        </figure>
        <div class="memory-viewer-nav" aria-label="切换影像">
          <button type="button" @click="showMemory(currentIndex - 1)">上一张</button>
          <span aria-hidden="true"></span>
          <button type="button" @click="showMemory(currentIndex + 1)">下一张</button>
        </div>
      </div>
    </dialog>
  </div>
</template>

<style scoped>
.atmosphere-gallery {
  margin-top: 20px;
}

.gallery-heading {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 18px;
  color: var(--chapter-color);
}

.gallery-heading b {
  font-size: 12px;
  font-weight: 650;
}

.gallery-heading span {
  color: color-mix(in srgb, var(--chapter-color) 62%, transparent);
  font-size: 10px;
  line-height: 1.5;
  text-align: right;
}

.memory-deck {
  position: relative;
  width: min(900px, 72vw);
  height: 162px;
  margin: 12px 0 0;
  padding: 0;
  list-style: none;
  perspective: 1000px;
}

.memory-deck li {
  position: absolute;
  top: 0;
  left: 0;
  width: clamp(112px, 12.4vw, 174px);
  transform: translateX(calc(var(--memory-index) * 74%)) rotate(var(--memory-tilt));
}

.memory-deck li:nth-child(1) { --memory-index: 0; --memory-tilt: -3deg; }
.memory-deck li:nth-child(2) { --memory-index: 1; --memory-tilt: -1.5deg; top: 10px; }
.memory-deck li:nth-child(3) { --memory-index: 2; --memory-tilt: 1deg; top: 4px; }
.memory-deck li:nth-child(4) { --memory-index: 3; --memory-tilt: -1deg; top: 14px; }
.memory-deck li:nth-child(5) { --memory-index: 4; --memory-tilt: 2deg; top: 5px; }
.memory-deck li:nth-child(6) { --memory-index: 5; --memory-tilt: -2deg; top: 12px; }
.memory-deck li:nth-child(7) { --memory-index: 6; --memory-tilt: 1.5deg; top: 2px; }
.memory-deck li:nth-child(8) { --memory-index: 7; --memory-tilt: 3deg; top: 11px; }

.memory-deck button {
  display: block;
  width: 100%;
  min-height: 44px;
  padding: 0;
  border: 0;
  background: transparent;
  cursor: zoom-in;
  transition-property: transform;
  transition-duration: 160ms;
  transition-timing-function: cubic-bezier(0.16, 1, 0.3, 1);
}

.memory-deck button:active,
.memory-viewer button:active {
  transform: scale(0.97);
}

.memory-deck figure {
  margin: 0;
  padding: 6px 6px 20px;
  background: #d9d7d0;
  color: #242622;
  box-shadow: 0 14px 34px rgb(0 0 0 / 28%);
}

.memory-deck img {
  display: block;
  width: 100%;
  height: auto;
  aspect-ratio: 3 / 2;
  object-fit: cover;
  outline: 1px solid rgb(0 0 0 / 10%);
  outline-offset: -1px;
}

.memory-deck figcaption {
  margin-top: 5px;
  overflow: hidden;
  font-size: 8px;
  line-height: 1;
  letter-spacing: 0.03em;
  white-space: nowrap;
}

.memory-viewer {
  width: 100vw;
  max-width: none;
  height: 100svh;
  max-height: none;
  margin: 0;
  padding: 0;
  overflow: hidden;
  border: 0;
  background: rgb(8 10 9 / 96%);
  color: #d7d7d2;
}

.memory-viewer::backdrop {
  background: rgb(8 10 9 / 92%);
}

.memory-viewer-shell {
  position: relative;
  display: grid;
  width: 100%;
  height: 100%;
  padding: max(70px, calc(env(safe-area-inset-top) + 54px)) max(42px, env(safe-area-inset-right)) max(46px, calc(env(safe-area-inset-bottom) + 34px)) max(42px, env(safe-area-inset-left));
  align-items: center;
}

.memory-viewer-close {
  position: absolute;
  z-index: 2;
  top: max(18px, env(safe-area-inset-top));
  right: max(20px, env(safe-area-inset-right));
  min-width: 56px;
  min-height: 44px;
  padding: 0 12px;
  border: 0;
  border-radius: 4px;
  background: rgb(215 215 210 / 9%);
  box-shadow: inset 0 0 0 1px rgb(215 215 210 / 22%);
  color: inherit;
  cursor: pointer;
}

.memory-viewer-figure {
  display: grid;
  width: 100%;
  margin: 0;
  grid-template-columns: minmax(0, 1fr) minmax(230px, 300px);
  align-items: end;
  gap: clamp(28px, 5vw, 76px);
}

.memory-viewer-figure img {
  display: block;
  width: 100%;
  max-height: min(76svh, 780px);
  object-fit: contain;
  filter: drop-shadow(0 28px 70px rgb(0 0 0 / 38%));
  touch-action: pan-y;
}

.memory-viewer-figure figcaption > p:last-of-type {
  margin: 14px 0 8px;
  color: rgb(215 215 210 / 72%);
  font-size: 13px;
  line-height: 1.65;
}

.memory-viewer-figure small {
  color: rgb(215 215 210 / 52%);
  font-size: 10px;
}

.memory-viewer-kicker {
  display: flex;
  margin: 0 0 18px;
  justify-content: space-between;
  gap: 24px;
  color: rgb(215 215 210 / 62%);
  font-size: 10px;
  letter-spacing: 0.08em;
}

.memory-viewer-figure h3 {
  margin: 0;
  font-size: clamp(30px, 3.2vw, 46px);
  font-weight: 510;
  line-height: 1.05;
}

.memory-viewer-nav {
  position: absolute;
  right: max(42px, env(safe-area-inset-right));
  bottom: max(26px, env(safe-area-inset-bottom));
  display: grid;
  width: min(300px, calc(100vw - 84px));
  grid-template-columns: auto 1fr auto;
  align-items: center;
  gap: 14px;
}

.memory-viewer-nav span {
  height: 1px;
  background: rgb(215 215 210 / 24%);
}

.memory-viewer-nav button {
  min-height: 44px;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  cursor: pointer;
}

@media (hover: hover) and (pointer: fine) {
  .memory-deck button:hover {
    transform: translateY(-4px);
  }
}

@media (max-width: 720px) {
  .gallery-heading span {
    max-width: 22ch;
  }

  .memory-deck {
    display: flex;
    width: calc(100vw - var(--safe-left) - var(--safe-right));
    height: 116px;
    margin-top: 10px;
    gap: 8px;
    overflow-x: auto;
    overflow-y: hidden;
    perspective: none;
    overscroll-behavior-inline: contain;
    scroll-snap-type: x mandatory;
    scrollbar-width: none;
  }

  .memory-deck::-webkit-scrollbar {
    display: none;
  }

  .memory-deck li,
  .memory-deck li:nth-child(n) {
    position: relative;
    top: auto;
    left: auto;
    width: 118px;
    flex: 0 0 118px;
    scroll-snap-align: start;
    transform: none;
  }

  .memory-viewer-shell {
    padding: max(72px, calc(env(safe-area-inset-top) + 58px)) var(--safe-right) max(84px, calc(env(safe-area-inset-bottom) + 70px)) var(--safe-left);
  }

  .memory-viewer-figure {
    display: block;
  }

  .memory-viewer-figure img {
    max-height: 56svh;
  }

  .memory-viewer-figure figcaption {
    padding-top: 18px;
  }

  .memory-viewer-figure h3 {
    font-size: 28px;
  }

  .memory-viewer-nav {
    right: var(--safe-right);
    bottom: max(18px, env(safe-area-inset-bottom));
    left: var(--safe-left);
    width: auto;
  }
}

@media (prefers-reduced-motion: reduce) {
  .memory-deck button {
    transition: none;
  }
}
</style>
