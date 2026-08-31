<script setup lang="ts">
import { computed, ref } from 'vue'
import WaterLens from './WaterLens.vue'
import { JIANGNAN_SCENES, sceneForPhase } from '../data/sceneSpecs'
import type { JobPhase } from '../types/job'

const props = withDefaults(
  defineProps<{
    phase?: JobPhase | null
    loggedIn?: boolean
    progress?: number
  }>(),
  {
    phase: null,
    loggedIn: false,
    progress: 0,
  },
)

// The stage translates non-sensitive job state into art direction and never reads archive data or APIs.
const scene = computed(() => sceneForPhase(props.phase, props.loggedIn))
const sceneSpec = computed(() => JIANGNAN_SCENES[scene.value])
const safeProgress = computed(() =>
  Math.min(1, Math.max(0, Number.isFinite(props.progress) ? props.progress : 0)),
)
const rendererReady = ref(false)
</script>

<template>
  <div
    class="jiangnan-stage"
    :class="{ 'is-renderer-ready': rendererReady }"
    :data-scene="scene"
    aria-hidden="true"
    inert
  >
    <picture class="jiangnan-stage__fallback">
      <source
        media="(max-width: 720px)"
        :srcset="sceneSpec.mobileSrc"
        width="941"
        height="1672"
      />
      <img
        :src="sceneSpec.desktopSrc"
        alt=""
        width="1672"
        height="941"
        decoding="async"
        fetchpriority="high"
      />
    </picture>

    <WaterLens
      class="jiangnan-stage__lens"
      :scene="scene"
      :progress="safeProgress"
      @ready-change="rendererReady = $event"
    />
    <div class="jiangnan-stage__scrim"></div>
    <div class="jiangnan-stage__grain"></div>
  </div>
</template>

<style scoped>
.jiangnan-stage__fallback,
.jiangnan-stage__fallback img,
.jiangnan-stage__lens,
.jiangnan-stage__scrim,
.jiangnan-stage__grain {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
}

.jiangnan-stage {
  position: fixed;
  z-index: 0;
  inset: 0;
  width: 100%;
  height: 100%;
  overflow: hidden;
  isolation: isolate;
  contain: paint;
  background: #171a18;
  pointer-events: none;
}

.jiangnan-stage__fallback {
  z-index: 0;
  display: block;
  margin: 0;
  opacity: 1;
  transition-property: opacity;
  transition-duration: 380ms;
  transition-timing-function: cubic-bezier(0.16, 1, 0.3, 1);
}

.jiangnan-stage__fallback img {
  display: block;
  object-fit: cover;
  object-position: center;
}

.jiangnan-stage__lens {
  z-index: 1;
  opacity: 0;
  transition-property: opacity;
  transition-duration: 480ms;
  transition-timing-function: cubic-bezier(0.16, 1, 0.3, 1);
}

.jiangnan-stage.is-renderer-ready .jiangnan-stage__fallback {
  opacity: 0;
}

.jiangnan-stage.is-renderer-ready .jiangnan-stage__lens {
  opacity: 1;
}

.jiangnan-stage__scrim {
  z-index: 2;
}

.jiangnan-stage[data-scene='0'] .jiangnan-stage__scrim {
  background:
    radial-gradient(circle at 77% 46%, rgb(226 226 218 / 42%) 0, rgb(226 226 218 / 14%) 24%, transparent 48%),
    linear-gradient(90deg, transparent 0 48%, rgb(19 21 19 / 10%) 100%);
}

.jiangnan-stage[data-scene='1'] .jiangnan-stage__scrim {
  background: linear-gradient(90deg, rgb(12 14 12 / 68%) 0, rgb(12 14 12 / 35%) 31%, transparent 60%);
}

.jiangnan-stage[data-scene='2'] .jiangnan-stage__scrim {
  background: linear-gradient(90deg, rgb(10 12 10 / 70%) 0, rgb(10 12 10 / 38%) 34%, transparent 63%);
}

.jiangnan-stage[data-scene='3'] .jiangnan-stage__scrim {
  background: linear-gradient(90deg, rgb(220 221 214 / 64%) 0, rgb(220 221 214 / 29%) 34%, transparent 63%);
}

.jiangnan-stage[data-scene='4'] .jiangnan-stage__scrim {
  background: linear-gradient(90deg, rgb(12 13 12 / 72%) 0, rgb(12 13 12 / 4%) 53%, rgb(12 13 12 / 20%) 100%);
}

.jiangnan-stage[data-scene='5'] .jiangnan-stage__scrim {
  background:
    linear-gradient(90deg, transparent 0 45%, rgb(225 225 217 / 22%) 68%, rgb(225 225 217 / 60%) 100%),
    linear-gradient(0deg, rgb(225 225 217 / 25%) 0, transparent 48%);
}

.jiangnan-stage__grain {
  z-index: 3;
  opacity: 0.05;
  mix-blend-mode: soft-light;
  background-image:
    repeating-radial-gradient(circle at 20% 30%, rgb(255 255 255 / 36%) 0 0.55px, transparent 0.8px 3.5px),
    repeating-radial-gradient(circle at 74% 61%, rgb(0 0 0 / 30%) 0 0.45px, transparent 0.7px 3px);
  background-size: 7px 7px, 9px 9px;
}

@media (max-width: 720px) {
  .jiangnan-stage[data-scene='0'] .jiangnan-stage__scrim {
    background:
      linear-gradient(0deg, rgb(16 18 16 / 62%) 0, transparent 45%),
      radial-gradient(circle at 52% 38%, rgb(226 226 218 / 20%) 0, transparent 42%);
  }

  .jiangnan-stage[data-scene='1'] .jiangnan-stage__scrim,
  .jiangnan-stage[data-scene='2'] .jiangnan-stage__scrim,
  .jiangnan-stage[data-scene='4'] .jiangnan-stage__scrim {
    background: linear-gradient(0deg, rgb(10 12 10 / 74%) 0, rgb(10 12 10 / 28%) 38%, transparent 67%);
  }

  .jiangnan-stage[data-scene='3'] .jiangnan-stage__scrim,
  .jiangnan-stage[data-scene='5'] .jiangnan-stage__scrim {
    background: linear-gradient(0deg, rgb(223 223 216 / 72%) 0, rgb(223 223 216 / 26%) 38%, transparent 67%);
  }

  .jiangnan-stage__grain {
    opacity: 0.035;
  }
}

@media (prefers-reduced-motion: reduce) {
  .jiangnan-stage__fallback,
  .jiangnan-stage__lens {
    transition-duration: 0.01ms;
  }
}

@media (forced-colors: active), print {
  .jiangnan-stage {
    display: none;
  }
}
</style>
