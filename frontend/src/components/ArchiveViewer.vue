<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import ArchiveMedia from './ArchiveMedia.vue'
import { fetchArchivePage } from '../services/archiveApi'
import type { ArchiveCategory, ArchiveManifest, ArchiveRecord } from '../types/archive'

const props = defineProps<{ manifest: ArchiveManifest; expiresLabel: string; busy?: boolean }>()
const emit = defineEmits<{ delete: [] }>()

const search = ref('')
const category = ref<ArchiveCategory | ''>('')
const year = ref<number | ''>('')
const records = ref<ArchiveRecord[]>([])
const years = ref<number[]>([])
const total = ref(props.manifest.records)
const nextOffset = ref<number | null>(0)
const loading = ref(false)
const loadError = ref('')
let controller: AbortController | undefined
let searchTimer: number | undefined

const hasMore = computed(() => nextOffset.value !== null)

// Filters restart the immutable server-side page query; typing is debounced to avoid noisy requests.
watch([search, category, year], () => {
  if (searchTimer !== undefined) window.clearTimeout(searchTimer)
  searchTimer = window.setTimeout(() => void loadPage(true), 250)
})

onMounted(() => void loadPage(true))
onBeforeUnmount(() => {
  controller?.abort()
  if (searchTimer !== undefined) window.clearTimeout(searchTimer)
})

async function loadPage(reset = false) {
  if (loading.value && !reset) return
  if (reset) {
    controller?.abort()
    records.value = []
    nextOffset.value = 0
  }
  if (nextOffset.value === null) return
  const request = new AbortController()
  controller = request
  loading.value = true
  loadError.value = ''
  try {
    const page = await fetchArchivePage({
      offset: nextOffset.value,
      limit: 30,
      search: search.value.trim(),
      category: category.value,
      year: year.value,
    }, request.signal)
    if (request.signal.aborted) return
    records.value = reset ? page.items : [...records.value, ...page.items]
    total.value = page.total
    nextOffset.value = page.nextOffset
    years.value = page.years
  } catch (reason) {
    if (request.signal.aborted) return
    loadError.value = reason instanceof Error ? reason.message : '这些记录暂时没有翻开'
  } finally {
    if (controller === request) loading.value = false
  }
}

function categoryLabel(value: ArchiveCategory): string {
  return { self: '自己的动态', other: '好友动态', guestbook: '留言' }[value]
}

function formatDate(record: ArchiveRecord): { day: string; year: string; full: string } {
  if (!record.publishedAt) return { day: '--.--', year: '时间未知', full: '时间未知' }
  const date = new Date(record.publishedAt * 1000)
  return {
    day: `${String(date.getMonth() + 1).padStart(2, '0')}.${String(date.getDate()).padStart(2, '0')}`,
    year: String(date.getFullYear()),
    full: date.toLocaleString('zh-CN', { dateStyle: 'long', timeStyle: 'short' }),
  }
}
</script>

<template>
  <div class="archive-reader">
    <header class="reader-header">
      <div class="reader-brand">
        <span class="reader-brand__mark" aria-hidden="true"></span>
        <span><strong>拾光册</strong><small>我的回忆册</small></span>
      </div>
      <div class="reader-header__actions">
        <a class="reader-download pressable" href="/api/download">保存 ZIP</a>
        <button class="reader-close pressable" type="button" :disabled="busy" @click="emit('delete')">删除临时数据</button>
      </div>
    </header>

    <main>
      <section class="reader-cover" aria-labelledby="reader-title">
        <picture>
          <source media="(max-width: 720px)" srcset="/assets/jiangnan/scene-05-dawn-mobile.webp" />
          <img src="/assets/jiangnan/scene-05-dawn.webp" alt="雨后清晨的江南河岸" />
        </picture>
        <div class="reader-cover__veil" aria-hidden="true"></div>
        <div class="reader-cover__content">
          <p class="reader-kicker"><span>06</span>屋内</p>
          <h1 id="reader-title">原来，<br />那些日子还在。</h1>
          <p>你刚刚找回的记录，都在下面。</p>
          <dl class="reader-summary" aria-label="归档概览">
            <div><dt>记录</dt><dd>{{ manifest.records.toLocaleString() }}</dd></div>
            <div><dt>媒体</dt><dd>{{ manifest.mediaDownloaded.toLocaleString() }}</dd></div>
            <div><dt>生成于</dt><dd>{{ new Date(manifest.generatedAt * 1000).toLocaleDateString('zh-CN') }}</dd></div>
          </dl>
          <p class="reader-privacy"><span aria-hidden="true"></span>当前页直接读取本次临时归档，约 {{ expiresLabel }} 后自动清理。</p>
        </div>
      </section>

      <section class="reader-library" aria-label="归档记录">
        <div class="reader-tools">
          <label class="reader-search">
            <span class="sr-only">搜索昵称或内容</span>
            <input v-model="search" type="search" placeholder="搜索一句话、一个名字" autocomplete="off" />
          </label>
          <label>
            <span class="sr-only">按年份筛选</span>
            <select v-model="year">
              <option value="">所有年份</option>
              <option v-for="item in years" :key="item" :value="item">{{ item }} 年</option>
            </select>
          </label>
          <label>
            <span class="sr-only">按记录类型筛选</span>
            <select v-model="category">
              <option value="">所有记录</option>
              <option value="self">自己的动态</option>
              <option value="other">好友动态</option>
              <option value="guestbook">留言</option>
            </select>
          </label>
          <p aria-live="polite">{{ loading && !records.length ? '正在翻开' : `找到 ${total.toLocaleString()} 条` }}</p>
        </div>

        <div v-if="records.length" class="reader-timeline">
          <article v-for="record in records" :key="`${record.id}-${record.cellId}`" class="memory-entry">
            <time :datetime="record.publishedAt ? new Date(record.publishedAt * 1000).toISOString() : undefined">
              <strong>{{ formatDate(record).day }}</strong>
              <span>{{ formatDate(record).year }}</span>
            </time>
            <div class="memory-entry__body">
              <p class="memory-entry__meta">
                <span>{{ categoryLabel(record.category) }}</span>
                <b>{{ record.authorName || '名字没有留下' }}</b>
                <time>{{ formatDate(record).full }}</time>
              </p>
              <p class="memory-entry__copy">{{ record.content || '这条记录没有留下文字。' }}</p>
              <div v-if="record.media.length" class="memory-entry__media" :data-count="Math.min(record.media.length, 4)">
                <ArchiveMedia v-for="path in record.media" :key="path" :path="path" />
              </div>
            </div>
          </article>
        </div>

        <div v-else-if="loadError" class="reader-empty" role="alert">
          <p>{{ loadError }}</p>
          <button type="button" class="text-action pressable" @click="loadPage(true)">再试一次</button>
        </div>

        <div v-else-if="!loading" class="reader-empty">
          <p>这一页没有找到记录。</p>
          <button type="button" class="text-action pressable" @click="search = ''; category = ''; year = ''">清空筛选</button>
        </div>

        <button v-if="hasMore" class="reader-more pressable" type="button" :disabled="loading" @click="loadPage(false)">
          {{ loading ? '正在翻页' : '再往前翻 30 条' }}
        </button>
      </section>
    </main>

    <footer class="reader-footer">
      <p>QQ 还能返回多少，归档中就保存多少。</p>
      <a class="text-action pressable" href="/api/download">保存完整 ZIP 备份</a>
    </footer>
  </div>
</template>

<style scoped>
.archive-reader {
  min-height: 100dvh;
  background: #e8e3d8;
  color: #171a18;
  font-family: -apple-system, BlinkMacSystemFont, "SF Pro Display", "PingFang SC", "Hiragino Sans GB", sans-serif;
}

.reader-header {
  position: fixed;
  inset: 0 0 auto;
  z-index: 20;
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 84px;
  padding: 0 clamp(22px, 4vw, 64px);
  color: #f0eee8;
  mix-blend-mode: difference;
}

.reader-brand,
.reader-header__actions,
.reader-header__actions a,
.reader-header__actions button {
  display: flex;
  align-items: center;
}

.reader-brand {
  gap: 11px;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
}

.reader-brand__mark {
  width: 31px;
  height: 31px;
  border: 1px solid currentColor;
  border-radius: 50%;
  background: radial-gradient(circle at 34% 34%, currentColor 0 2px, transparent 3px), radial-gradient(circle at 68% 68%, currentColor 0 1px, transparent 2px);
}

.reader-brand strong,
.reader-brand small {
  display: block;
}

.reader-brand strong {
  font-size: 14px;
  letter-spacing: .12em;
}

.reader-brand small {
  margin-top: 2px;
  font-size: 9px;
  opacity: .66;
}

.reader-header__actions {
  gap: 20px;
}

.reader-download,
.reader-close {
  min-height: 44px;
  border: 0;
  background: transparent;
  color: inherit;
  font: 12px/1 inherit;
  text-decoration: none;
  cursor: pointer;
}

.reader-download {
  border-bottom: 1px solid currentColor;
}

.reader-cover {
  position: relative;
  min-height: 76svh;
  overflow: hidden;
  background: #858985;
}

.reader-cover > picture,
.reader-cover > picture > img,
.reader-cover__veil {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
}

.reader-cover > picture > img {
  object-fit: cover;
  object-position: center;
}

.reader-cover__veil {
  background: linear-gradient(90deg, rgba(19, 23, 20, .18) 0 45%, rgba(232, 227, 216, .18) 70%, rgba(232, 227, 216, .72));
}

.reader-cover__content {
  position: relative;
  z-index: 1;
  width: min(560px, calc(100% - 48px));
  margin-left: auto;
  padding: clamp(150px, 21vh, 230px) clamp(28px, 7vw, 108px) 90px 0;
}

.reader-kicker {
  display: flex;
  align-items: center;
  gap: 14px;
  margin: 0 0 28px;
  color: #414641;
  font-size: 11px;
  letter-spacing: .22em;
}

.reader-kicker span {
  font-variant-numeric: tabular-nums;
}

.reader-kicker::after {
  width: 40px;
  height: 1px;
  background: currentColor;
  content: "";
  opacity: .45;
}

.reader-cover h1 {
  max-width: 9em;
  margin: 0;
  font-size: clamp(38px, 5vw, 68px);
  font-weight: 520;
  line-height: 1.08;
}

.reader-cover__content > p:not(.reader-kicker, .reader-privacy) {
  margin: 24px 0 0;
  color: #4b504b;
  font-size: 14px;
}

.reader-summary {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  margin: 54px 0 0;
  border-block: 1px solid rgba(23, 26, 24, .24);
}

.reader-summary div {
  padding: 15px 10px 17px 0;
}

.reader-summary dt {
  color: #686d68;
  font-size: 10px;
}

.reader-summary dd {
  margin: 7px 0 0;
  font-size: 16px;
  font-variant-numeric: tabular-nums;
}

.reader-privacy {
  display: flex;
  gap: 8px;
  align-items: center;
  margin: 18px 0 0;
  color: #515651;
  font-size: 11px;
}

.reader-privacy span {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: #5b4032;
}

.reader-library {
  width: min(1080px, calc(100% - 48px));
  margin: 0 auto;
  padding: 0 0 120px;
}

.reader-tools {
  position: sticky;
  top: 0;
  z-index: 10;
  display: grid;
  grid-template-columns: minmax(260px, 1fr) 150px 150px auto;
  gap: 18px;
  align-items: center;
  padding: 17px 0;
  border-bottom: 1px solid rgba(23, 26, 24, .2);
  background: rgba(232, 227, 216, .96);
}

.reader-tools input,
.reader-tools select {
  width: 100%;
  min-height: 44px;
  padding: 0 2px;
  border: 0;
  border-bottom: 1px solid rgba(23, 26, 24, .42);
  border-radius: 0;
  background: transparent;
  color: inherit;
  font: 13px/1.4 inherit;
}

.reader-tools input:focus-visible,
.reader-tools select:focus-visible {
  outline: 2px solid #5b4032;
  outline-offset: 3px;
}

.reader-tools > p {
  margin: 0;
  color: #656a65;
  font-size: 11px;
  text-align: right;
  white-space: nowrap;
}

.reader-timeline {
  margin-top: 54px;
}

.memory-entry {
  display: grid;
  grid-template-columns: 92px minmax(0, 1fr);
  gap: clamp(28px, 5vw, 72px);
  padding: 0 0 68px;
}

.memory-entry > time {
  position: sticky;
  top: 96px;
  align-self: start;
  padding-top: 3px;
  color: #686d68;
  font-variant-numeric: tabular-nums;
}

.memory-entry > time strong,
.memory-entry > time span {
  display: block;
}

.memory-entry > time strong {
  color: #171a18;
  font-size: 21px;
  font-weight: 520;
}

.memory-entry > time span {
  margin-top: 3px;
  font-size: 10px;
  letter-spacing: .18em;
}

.memory-entry__body {
  min-width: 0;
  padding-bottom: 68px;
  border-bottom: 1px solid rgba(23, 26, 24, .18);
}

.memory-entry__meta {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 16px;
  align-items: baseline;
  margin: 0;
  color: #696e69;
  font-size: 11px;
}

.memory-entry__meta > span {
  color: #704f3e;
}

.memory-entry__meta b {
  color: #343834;
  font-weight: 610;
}

.memory-entry__meta time {
  margin-left: auto;
}

.memory-entry__copy {
  max-width: 760px;
  margin: 24px 0 0;
  color: #222622;
  font-size: clamp(16px, 1.8vw, 20px);
  line-height: 1.85;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.memory-entry__media {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 5px;
  margin-top: 28px;
}

.memory-entry__media[data-count="1"] {
  grid-template-columns: minmax(0, 720px);
}

.memory-entry__media[data-count="3"] > :first-child {
  grid-row: span 2;
}

.reader-more {
  display: block;
  min-height: 48px;
  margin: 10px auto 0;
  padding: 0 4px;
  border: 0;
  border-bottom: 1px solid #5b4032;
  background: transparent;
  color: #5b4032;
  font: 13px/1 inherit;
  cursor: pointer;
}

.reader-empty {
  padding: 130px 0;
  text-align: center;
}

.reader-empty p {
  color: #626762;
}

.reader-footer {
  display: flex;
  justify-content: space-between;
  gap: 24px;
  padding: 30px clamp(24px, 6vw, 86px) 46px;
  border-top: 1px solid rgba(23, 26, 24, .18);
  color: #5d625d;
  font-size: 11px;
}

.reader-footer p {
  margin: 0;
}

@media (max-width: 720px) {
  .reader-header {
    height: 72px;
    padding: 0 18px;
  }

  .reader-header__actions .reader-download {
    display: flex;
  }

  .reader-cover {
    min-height: 82svh;
  }

  .reader-cover__veil {
    background: linear-gradient(0deg, rgba(20, 23, 21, .72), rgba(20, 23, 21, .04) 68%);
  }

  .reader-cover__content {
    width: auto;
    margin: 0;
    padding: 46svh 20px 54px;
    color: #e8e3d8;
  }

  .reader-kicker,
  .reader-cover__content > p:not(.reader-kicker, .reader-privacy),
  .reader-privacy {
    color: #cacbc6;
  }

  .reader-cover h1 {
    font-size: clamp(36px, 11.8vw, 50px);
  }

  .reader-summary {
    margin-top: 38px;
    border-color: rgba(232, 227, 216, .28);
  }

  .reader-summary dt {
    color: #b7bab5;
  }

  .reader-library {
    width: min(100% - 32px, 560px);
    padding: 0 0 90px;
  }

  .reader-tools {
    position: static;
    grid-template-columns: 1fr 1fr;
    gap: 12px 14px;
  }

  .reader-search,
  .reader-tools > p {
    grid-column: 1 / -1;
  }

  .reader-tools > p {
    text-align: left;
  }

  .reader-timeline {
    margin-top: 42px;
  }

  .memory-entry {
    grid-template-columns: 52px minmax(0, 1fr);
    gap: 16px;
    padding-bottom: 44px;
  }

  .memory-entry > time {
    position: static;
  }

  .memory-entry > time strong {
    font-size: 16px;
  }

  .memory-entry__body {
    padding-bottom: 44px;
  }

  .memory-entry__meta time {
    width: 100%;
    margin-left: 0;
  }

  .memory-entry__copy {
    margin-top: 17px;
    font-size: 16px;
    line-height: 1.75;
  }

  .memory-entry__media {
    grid-template-columns: 1fr;
    margin-top: 20px;
  }

  .memory-entry__media[data-count="3"] > :first-child {
    grid-row: auto;
  }

  .reader-footer {
    display: grid;
    padding-inline: 20px;
  }
}

@media (prefers-reduced-motion: reduce) {
  .archive-reader *,
  .archive-reader *::before,
  .archive-reader *::after {
    scroll-behavior: auto !important;
  }
}

@media (forced-colors: active) {
  .reader-cover__veil {
    background: Canvas;
    opacity: .86;
  }

  .reader-header {
    mix-blend-mode: normal;
    color: CanvasText;
  }
}
</style>
