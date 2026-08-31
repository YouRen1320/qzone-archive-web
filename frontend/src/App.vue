<script setup lang="ts">
import { computed, onMounted, onScopeDispose, ref, watch } from 'vue'
import ArchiveViewer from './components/ArchiveViewer.vue'
import JiangnanStage from './components/JiangnanStage.vue'
import MemoryGallery from './components/MemoryGallery.vue'
import PrivacyPanel from './components/PrivacyPanel.vue'
import { useArchiveJob } from './composables/useArchiveJob'
import { useRainSound } from './composables/useRainSound'
import { fetchArchiveManifest } from './services/archiveApi'
import type { ArchiveManifest } from './types/archive'
import type { JobPhase } from './types/job'
import { mediaProgress, remainingLabel } from './utils'

// App owns the real archive workflow. The visual scene receives only non-sensitive status and never calls the API.
const {
  status,
  qrImage,
  busy,
  error,
  active,
  initialize,
  ensureJob,
  requestQr,
  startArchive,
  cancelArchive,
  deleteJob,
} = useArchiveJob()
const rainSound = useRainSound()
const includeMedia = ref(true)
const pageDelayMs = ref(3_000)
const entered = ref(false)
const maxReachedChapter = ref(0)
const now = ref(Date.now())
const archiveManifest = ref<ArchiveManifest | null>(null)
const viewerLoading = ref(false)
let clockTimer: number | undefined

const chapterLabels = ['门外', '扫码', '选择', '整理', '装包', '保存']

const scenePhase = computed<JobPhase | null>(() => (entered.value ? status.value?.phase ?? null : null))
const mediaPercent = computed(() => mediaProgress(status.value))
const ttl = computed(() => (status.value ? remainingLabel(status.value.expiresAt, now.value) : ''))
const canStart = computed(() => Boolean(status.value?.loggedIn && !active.value))
const needsLogin = computed(() =>
  Boolean(
    status.value &&
      (status.value.phase === 'failed' || !status.value.loggedIn) &&
      status.value.phase !== 'ready' &&
      !active.value,
  ),
)

// Phase-to-scene mapping follows the next real action, while maxReachedChapter remembers completed visual ground.
const chapterIndex = computed(() => {
  if (!entered.value || !status.value) return 0
  switch (status.value.phase) {
    case 'awaitingLogin':
    case 'failed':
    case 'interrupted':
      return 1
    case 'loggedIn':
      return 2
    case 'paused':
    case 'cancelled':
      return status.value.loggedIn ? 2 : 1
    case 'queued':
    case 'archiving':
      return 3
    case 'downloadingMedia':
    case 'packaging':
      return 4
    case 'ready':
      return 5
  }
})

const chapterTone = computed(() => ([0, 3, 5].includes(chapterIndex.value) ? 'ink' : 'paper'))
const progressValue = computed(() =>
  status.value?.phase === 'downloadingMedia' ? Math.round(mediaPercent.value) : undefined,
)
const progressLabel = computed(() => {
  switch (status.value?.phase) {
    case 'queued': return '等船开'
    case 'archiving': return '读记录'
    case 'downloadingMedia': return '取媒体'
    case 'packaging': return '装归档'
    default: return '准备'
  }
})
const activeTitle = computed(() => {
  switch (status.value?.phase) {
    case 'queued': return '船还没开，先等一会。'
    case 'archiving': return `第 ${status.value.pages.toLocaleString()} 页，正在回来。`
    case 'downloadingMedia': return '照片和视频，正在跟上。'
    case 'packaging': return '正在装好。'
    default: return '正在整理。'
  }
})

// Recovery copy tells the user exactly what remains safe and which action comes next.
const recoveryNotice = computed(() => {
  switch (status.value?.phase) {
    case 'paused':
      return { title: '归档停在这里。', guidance: '已经落盘的分页还在，确认选项后可以从断点继续。' }
    case 'cancelled':
      return { title: '这次已经安全停下。', guidance: '已经整理好的分页还在，再次开始会优先使用断点。' }
    case 'failed':
      return { title: '这次没有走完。', guidance: '重新扫一下验证 QQ，已有断点会保留到任务到期。' }
    case 'interrupted':
      return { title: '服务器回来，登录已经失效。', guidance: '重新扫码后，可以从已经保存的位置继续。' }
    default:
      return null
  }
})

const liveAnnouncement = computed(() => {
  if (!status.value) return '可以开始新的临时归档'
  if (status.value.phase === 'ready') {
    return `归档包已准备好，共 ${status.value.saved.toLocaleString()} 条记录。${status.value.message}`
  }
  return recoveryNotice.value
    ? `${recoveryNotice.value.title}${status.value.message}`
    : status.value.message
})

watch(status, (current) => {
  // A restored task bypasses the decorative entrance and resumes at its real server phase.
  if (current && current.phase !== 'awaitingLogin') entered.value = true
}, { immediate: true })

watch(() => status.value?.phase, (phase) => {
  // A ready task opens its private manifest automatically, including after a page reload.
  if (phase === 'ready') void openCurrentArchive()
  else archiveManifest.value = null
}, { immediate: true })

watch(chapterIndex, (current) => {
  maxReachedChapter.value = Math.max(maxReachedChapter.value, current)
}, { immediate: true })

onMounted(() => {
  // Looking for an existing cookie restores a task without creating a new one on the landing scene.
  void initialize(false)
  clockTimer = window.setInterval(() => (now.value = Date.now()), 30_000)
})

onScopeDispose(() => {
  if (clockTimer !== undefined) window.clearInterval(clockTimer)
})

async function confirmDelete() {
  if (window.confirm('确认立即删除这个任务及服务器上的全部临时数据吗？')) {
    await deleteJob()
    archiveManifest.value = null
  }
}

async function enterArchiveFlow() {
  const current = await ensureJob()
  if (current) entered.value = true
}

async function openCurrentArchive() {
  if (archiveManifest.value || viewerLoading.value) return
  viewerLoading.value = true
  error.value = ''
  try {
    archiveManifest.value = await fetchArchiveManifest()
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : '回忆册暂时无法打开'
  } finally {
    viewerLoading.value = false
  }
}
</script>

<template>
  <ArchiveViewer
    v-if="archiveManifest"
    :manifest="archiveManifest"
    :expires-label="ttl"
    :busy="busy"
    @delete="confirmDelete"
  />

  <div v-else class="app-stage" :class="`tone-${chapterTone}`" :data-current="chapterIndex" :aria-busy="busy || viewerLoading">
    <a class="skip-link" href="#archive-operation">跳到归档操作</a>

    <JiangnanStage :phase="scenePhase" :logged-in="status?.loggedIn ?? false" :progress="mediaPercent / 100" />

    <header class="stage-header">
      <a class="brand pressable" href="/" aria-label="拾光册首页">
        <span class="brand-mark" aria-hidden="true"></span>
        <span><strong>拾光册</strong><small>临时归档</small></span>
      </a>

      <div class="stage-tools">
        <p class="privacy-line">
          <span aria-hidden="true"></span>
          {{ status ? `约 ${ttl} 后自动清空` : '任务到期自动清空' }}
        </p>
        <button
          class="sound-toggle pressable"
          type="button"
          :aria-label="rainSound.enabled.value ? '关闭雨声' : '播放雨声'"
          :aria-pressed="rainSound.enabled.value"
          :disabled="!rainSound.available.value"
          @click="rainSound.toggle"
        >
          <span class="sound-icon" aria-hidden="true"><i></i><i></i><i></i></span>
          <span>{{ rainSound.label.value }}</span>
        </button>
      </div>
    </header>

    <main id="archive-operation" class="chapter-layer" tabindex="-1">
      <p class="sr-only" role="status" aria-live="polite" aria-atomic="true">{{ liveAnnouncement }}</p>

      <div v-if="error" class="alert" role="alert" aria-atomic="true">
        <span aria-hidden="true">!</span><p>{{ error }}</p>
        <button type="button" aria-label="关闭错误提示" @click="error = ''">关闭</button>
      </div>

      <section v-if="!entered" class="chapter chapter--intro" aria-labelledby="intro-title">
        <p class="chapter-place"><span>00</span>门外</p>
        <h1 id="intro-title">雨还没停。</h1>
        <p class="chapter-copy">门里还有一段记录。</p>
        <button class="primary-action pressable" type="button" :disabled="busy" @click="enterArchiveFlow">
          {{ busy ? '正在开门' : '进去看看' }}
        </button>
        <p class="chapter-note">只整理 QQ 仍能返回的内容，完成后直接在这里查看</p>
      </section>

      <section v-else-if="!status" class="chapter chapter--loading" aria-label="正在准备归档任务">
        <p class="chapter-place"><span>00</span>门外</p><h1>正在开门。</h1>
        <p class="chapter-copy">临时任务准备好以后，会从这里开始。</p>
      </section>

      <section v-else-if="status.phase === 'ready'" class="chapter ready-card" aria-labelledby="ready-title">
        <p class="chapter-place"><span>05</span>清晨</p><h1 id="ready-title">正在翻开。</h1>
        <p class="chapter-copy">{{ status.saved.toLocaleString() }} 条记录，{{ status.mediaDownloaded.toLocaleString() }} 个媒体文件。</p>
        <dl class="result-facts" aria-label="归档结果统计">
          <div><dt>记录</dt><dd>{{ status.saved.toLocaleString() }}</dd></div>
          <div><dt>媒体</dt><dd>{{ status.mediaDownloaded.toLocaleString() }}</dd></div>
          <div><dt>未取回</dt><dd>{{ status.mediaFailed.toLocaleString() }}</dd></div>
        </dl>
        <button v-if="!viewerLoading" class="primary-action pressable" type="button" @click="openCurrentArchive">重新打开记录</button>
        <a class="download-action pressable" href="/api/download">保存完整 ZIP 备份</a>
        <p class="inline-status">页面会自动进入记录；ZIP 只用于长期备份。</p>
        <button class="text-action danger pressable" type="button" :disabled="busy" @click="confirmDelete">立即删除服务器临时文件</button>
      </section>

      <section v-else-if="active" class="chapter progress-card" aria-labelledby="progress-title">
        <p class="chapter-place"><span>{{ String(chapterIndex).padStart(2, '0') }}</span>{{ chapterIndex === 4 ? '灯下' : '船中' }}</p>
        <h1 id="progress-title">{{ activeTitle }}</h1><p class="chapter-copy">{{ status.message }}</p>

        <progress v-if="progressValue !== undefined" class="progress-line" :value="progressValue" max="100" role="progressbar" aria-label="媒体下载进度" aria-valuemin="0" aria-valuemax="100" :aria-valuenow="progressValue">{{ progressValue }}%</progress>
        <div v-else class="progress-line is-indeterminate" role="progressbar" aria-label="归档处理进度" aria-valuemin="0" aria-valuemax="100" :aria-valuetext="status.message"><span aria-hidden="true"></span></div>

        <div class="progress-reading"><strong>{{ progressValue === undefined ? progressLabel : `${progressValue}%` }}</strong><span>{{ progressLabel }}</span></div>
        <dl class="progress-facts" aria-label="当前归档统计">
          <div><dt>页数</dt><dd>{{ status.pages.toLocaleString() }}</dd></div>
          <div><dt>记录</dt><dd>{{ status.saved.toLocaleString() }}</dd></div>
          <div><dt>媒体</dt><dd>{{ status.mediaDownloaded.toLocaleString() }}</dd></div>
        </dl>
        <MemoryGallery v-if="chapterIndex === 4" />
        <button class="text-action danger pressable" type="button" :disabled="busy" @click="cancelArchive">安全停止</button>
        <p class="inline-status">保持页面打开。已经写入临时 SQLite 的分页不会因短暂断线丢失。</p>
      </section>

      <section v-else-if="needsLogin" class="chapter login-card" aria-labelledby="login-title">
        <p class="chapter-place"><span>01</span>廊下</p>
        <h1 id="login-title">{{ recoveryNotice ? '重新扫一下，断点还在。' : '用手机 QQ 扫一下。' }}</h1>
        <p class="chapter-copy">{{ recoveryNotice?.guidance || '只认这一次登录，任务结束就清掉。' }}</p>

        <div v-if="recoveryNotice" class="recovery-notice" role="status"><b>{{ recoveryNotice.title }}</b><span>{{ status.message }}</span></div>
        <div class="scan-flow">
          <div v-if="qrImage" class="qr-sheet"><img :src="qrImage" width="184" height="184" alt="用于本次临时任务的 QQ 登录二维码" /></div>
          <div class="scan-actions">
            <button class="primary-action pressable" type="button" :disabled="busy" @click="requestQr">{{ busy ? '正在连接 QQ' : qrImage ? '换一张二维码' : '显示二维码' }}</button>
            <a v-if="qrImage" class="text-action qr-save-link pressable" :href="qrImage" download="qzone-login-qr.png">同一部手机？保存后从相册识别</a>
            <p class="inline-status">{{ status.message }}</p>
          </div>
        </div>
        <PrivacyPanel compact />
      </section>

      <section v-else class="chapter options-card" aria-labelledby="options-title">
        <p class="chapter-place"><span>02</span>桥边</p>
        <h1 id="options-title">{{ recoveryNotice ? '从停下的地方接着走。' : '这次带走什么？' }}</h1>
        <p class="chapter-copy">{{ recoveryNotice?.guidance || '还能找到的图片和视频，也可以一起装进归档包。' }}</p>

        <div v-if="recoveryNotice" class="recovery-notice" role="status"><b>{{ recoveryNotice.title }}</b><span>{{ status.message }}</span></div>
        <form class="archive-options" @submit.prevent="startArchive(includeMedia, pageDelayMs)">
          <label class="option-line" for="include-media"><span><b>图片和视频</b><small>只保存 QQ 仍能返回的文件</small></span><input id="include-media" v-model="includeMedia" type="checkbox" /></label>
          <label class="field-line" for="page-delay"><span><b>请求节奏</b><small>慢一点，更不容易触发风控</small></span><select id="page-delay" v-model="pageDelayMs"><option :value="3000">稳妥 · 约 3 秒一页</option><option :value="5000">保守 · 约 5 秒一页</option><option :value="8000">更慢 · 约 8 秒一页</option></select></label>
          <button class="primary-action pressable" type="submit" :disabled="busy || !canStart">{{ busy ? '正在加入队列' : recoveryNotice ? '从已有断点继续' : '开始整理' }}</button>
          <p class="inline-status">已验证 {{ status.maskedUin || '当前 QQ' }}，设置只用于这次任务</p>
        </form>
      </section>

      <section v-if="status && entered" class="task-meta" aria-label="临时任务信息">
        <span>任务 {{ status.jobId.slice(0, 8) }}</span><span id="task-retention">约 {{ ttl }} 后清空</span>
        <button type="button" aria-describedby="task-retention" :disabled="busy || active" @click="confirmDelete">删除任务</button>
      </section>
    </main>

    <nav class="chapter-nav" aria-label="归档旅程">
      <ol>
        <li v-for="(label, index) in chapterLabels" :key="label" :class="{ current: chapterIndex === index, reached: maxReachedChapter >= index }" :aria-current="chapterIndex === index ? 'step' : undefined">
          <span aria-hidden="true"></span><b>{{ label }}</b>
        </li>
      </ol>
    </nav>

    <p class="legal-line">非腾讯官方产品，只归档本人或已获授权的数据。 <a href="https://github.com/YouRen1320/qzone-archive-web" target="_blank" rel="noreferrer">开源代码</a></p>
  </div>
</template>
