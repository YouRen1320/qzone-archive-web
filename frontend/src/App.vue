<script setup lang="ts">
import { computed, onMounted, onScopeDispose, ref } from 'vue'
import PrivacyPanel from './components/PrivacyPanel.vue'
import StepRail from './components/StepRail.vue'
import { useArchiveJob } from './composables/useArchiveJob'
import type { JobPhase } from './types/job'
import { mediaProgress, remainingLabel } from './utils'

// App maps backend phases onto three user-facing steps and never stores private task credentials in JavaScript.
const {
  status,
  qrImage,
  busy,
  error,
  active,
  initialize,
  requestQr,
  startArchive,
  cancelArchive,
  deleteJob,
} = useArchiveJob()

const includeMedia = ref(true)
const pageDelayMs = ref(3_000)
const now = ref(Date.now())
let clockTimer: number | undefined

type FlowStep = 1 | 2 | 3

// Explicit phase mapping keeps recovery states at the interrupted workflow step instead of sending users backwards.
const phaseStep: Record<JobPhase, FlowStep> = {
  awaitingLogin: 1,
  loggedIn: 2,
  queued: 2,
  archiving: 2,
  downloadingMedia: 2,
  packaging: 2,
  ready: 3,
  paused: 2,
  cancelled: 2,
  failed: 2,
  interrupted: 2,
}

const attentionPhases = new Set<JobPhase>(['paused', 'cancelled', 'failed', 'interrupted'])

const currentStep = computed<FlowStep>(() =>
  status.value ? (phaseStep[status.value.phase] ?? (status.value.loggedIn ? 2 : 1)) : 1,
)
const mediaPercent = computed(() => mediaProgress(status.value))
const ttl = computed(() => (status.value ? remainingLabel(status.value.expiresAt, now.value) : ''))
const canStart = computed(() => status.value?.loggedIn && !active.value)
const needsLogin = computed(() =>
  // A failed QQ request can leave the backend's in-memory login flag stale, so failure recovery always revalidates QQ.
  Boolean(
    status.value &&
      (status.value.phase === 'failed' || !status.value.loggedIn) &&
      status.value.phase !== 'ready' &&
      !active.value,
  ),
)
const needsAttention = computed(() => Boolean(status.value && attentionPhases.has(status.value.phase)))

// Recovery notices distinguish a stopped run from a fresh login while keeping the next action concrete.
const recoveryNotice = computed(() => {
  switch (status.value?.phase) {
    case 'paused':
      return {
        title: '归档已暂停',
        guidance: '已保存的断点仍在；验证 QQ 并重新确认本页选项后，可以继续归档。',
      }
    case 'cancelled':
      return {
        title: '归档已安全停止',
        guidance: '已完成的分页仍保留在这个临时任务中，再次开始时会优先使用可用断点。',
      }
    case 'failed':
      return {
        title: '这次归档没有完成',
        guidance: '临时数据和可用断点仍会保留到任务到期；重新扫码验证后再继续，避免旧登录状态误判。',
      }
    case 'interrupted':
      return {
        title: '服务器恢复后等待继续',
        guidance: '为保护账号，原登录会话已失效；重新扫码后可以从已保存断点继续。',
      }
    default:
      return null
  }
})

const loginCardKicker = computed(() => {
  if (needsLogin.value) {
    return needsAttention.value ? '继续第 2 步 · 重新验证 QQ' : '第 1 步 · QQ 安全登录'
  }
  return needsAttention.value ? '第 2 步 · 从断点继续' : '第 2 步 · 选择归档内容'
})

const loginCardTitle = computed(() => {
  if (needsLogin.value) {
    return needsAttention.value ? '重新扫码，继续已有断点' : '用手机 QQ 扫码确认'
  }
  return needsAttention.value
    ? `准备继续${status.value?.maskedUin ? ` ${status.value.maskedUin}` : '归档'}`
    : `已登录 ${status.value?.maskedUin || ''}`
})

const liveAnnouncement = computed(() => {
  if (!status.value) return '正在建立私密临时任务'
  if (status.value.phase === 'ready') {
    return `归档包已准备好，共 ${status.value.saved.toLocaleString()} 条记录。${status.value.message}`
  }
  if (recoveryNotice.value) {
    return `${recoveryNotice.value.title}。${status.value.message}`
  }
  return status.value.message
})

// Media downloads expose determinate progress; other active phases remain an honest indeterminate progress bar.
const progressValue = computed(() =>
  status.value?.phase === 'downloadingMedia' ? Math.round(mediaPercent.value) : undefined,
)

onMounted(() => {
  void initialize()
  clockTimer = window.setInterval(() => (now.value = Date.now()), 30_000)
})

onScopeDispose(() => {
  if (clockTimer !== undefined) window.clearInterval(clockTimer)
})

async function confirmDelete() {
  if (window.confirm('确认立即删除这个任务及服务器上的全部临时数据吗？')) {
    await deleteJob()
  }
}
</script>

<template>
  <div class="page-shell">
    <a class="skip-link" href="#workflow">跳到归档操作</a>
    <div class="garden-atmosphere" aria-hidden="true"></div>

    <header class="site-header">
      <a class="brand" href="/" aria-label="拾光册首页">
        <span class="brand-mark" aria-hidden="true">拾</span>
        <span><b>拾光册</b><small>QQ 空间临时归档</small></span>
      </a>
      <span class="privacy-chip"><i aria-hidden="true"></i>服务器不留长期副本</span>
    </header>

    <main>
      <section class="hero" aria-labelledby="hero-title">
        <div class="hero-inner">
          <div class="hero-copy-block">
            <p class="eyebrow">把旧时光，带回自己的设备</p>
            <h1 id="hero-title"><span>恢复能找到的，</span><em>带走属于你的。</em></h1>
            <p class="hero-copy">扫码后临时整理 QQ 仍能返回的互动记录、图片与视频，完成后打包下载到电脑或手机。</p>
          </div>
          <div class="hero-scene" aria-hidden="true">
            <div class="moon-gate"><div class="moon-gate-view"></div></div>
            <span class="garden-eave"></span>
            <span class="garden-window"></span>
          </div>
        </div>
      </section>

      <section
        id="workflow"
        class="workflow-stage"
        aria-label="QQ 空间临时归档流程"
        :aria-busy="busy"
        tabindex="-1"
      >
        <div class="workflow-lattice" aria-hidden="true"></div>
        <StepRail :current="currentStep" :attention="needsAttention" />

        <p
          v-if="status && !active && !recoveryNotice"
          class="sr-only"
          role="status"
          aria-live="polite"
          aria-atomic="true"
        >
          {{ liveAnnouncement }}
        </p>

        <div v-if="error" class="alert" role="alert" aria-atomic="true">
          <span aria-hidden="true">!</span>
          <p>{{ error }}</p>
          <button type="button" aria-label="关闭错误提示" @click="error = ''">×</button>
        </div>

        <section v-if="!status" class="work-card loading-card" aria-label="正在准备归档任务">
          <div class="spinner" aria-hidden="true"></div>
          <p role="status" aria-live="polite">正在建立私密临时任务…</p>
        </section>

        <section v-else-if="status.phase === 'ready'" class="work-card ready-card" aria-labelledby="ready-title">
          <div class="ready-seal" aria-hidden="true">✓</div>
          <p class="card-kicker">归档包已准备好</p>
          <h2 id="ready-title">{{ status.saved.toLocaleString() }} 条记录，等你带走</h2>
          <p class="status-copy">{{ status.message }}</p>
          <dl class="summary-grid" aria-label="归档结果统计">
            <div><dt>互动记录</dt><dd>{{ status.saved.toLocaleString() }}</dd></div>
            <div><dt>已下载媒体</dt><dd>{{ status.mediaDownloaded.toLocaleString() }}</dd></div>
            <div><dt>媒体失败</dt><dd>{{ status.mediaFailed.toLocaleString() }}</dd></div>
          </dl>
          <div class="card-actions ready-actions">
            <a class="primary-button download-button" href="/api/download">下载 ZIP 到本机</a>
            <p class="tiny-note">包内含离线网页、原始 JSONL 和独立 SQLite；不包含 QQ Cookie。</p>
            <button class="text-button danger" type="button" :disabled="busy" @click="confirmDelete">
              现在就删除服务器临时文件
            </button>
          </div>
        </section>

        <section v-else-if="active" class="work-card progress-card" aria-labelledby="progress-title">
          <p class="card-kicker">{{ status.phase === 'queued' ? '安全排队中' : '正在临时归档' }}</p>
          <div class="progress-message" role="status" aria-live="polite" aria-atomic="true">
            <h2 id="progress-title">{{ status.message }}</h2>
          </div>
          <div
            class="pulse-line"
            role="progressbar"
            aria-label="归档处理进度"
            aria-valuemin="0"
            aria-valuemax="100"
            :aria-valuenow="progressValue"
          >
            <i
              aria-hidden="true"
              :style="progressValue === undefined ? undefined : { width: `${progressValue}%` }"
            ></i>
          </div>
          <dl class="summary-grid" aria-label="当前归档进度">
            <div><dt>已读取页数</dt><dd>{{ status.pages.toLocaleString() }}</dd></div>
            <div><dt>唯一记录</dt><dd>{{ status.saved.toLocaleString() }}</dd></div>
            <div><dt>媒体进度</dt><dd>{{ Math.round(mediaPercent) }}%</dd></div>
          </dl>
          <p class="tiny-note">请保持本页面打开。即使网络短暂断开，已经提交到临时 SQLite 的分页也不会丢失。</p>
          <div class="card-actions">
            <button class="secondary-button" type="button" :disabled="busy" @click="cancelArchive">安全停止</button>
          </div>
        </section>

        <section v-else class="work-card login-card" aria-labelledby="login-title">
          <div class="card-heading">
            <div>
              <p class="card-kicker">{{ loginCardKicker }}</p>
              <h2 id="login-title">{{ loginCardTitle }}</h2>
            </div>
            <span v-if="status.loggedIn && !needsLogin" class="login-ok">已验证</span>
          </div>

          <div v-if="recoveryNotice" class="recovery-notice" :class="`is-${status.phase}`" role="status">
            <span class="recovery-mark" aria-hidden="true">!</span>
            <div>
              <strong>{{ recoveryNotice.title }}</strong>
              <p>{{ status.message }}</p>
              <small>{{ recoveryNotice.guidance }}</small>
            </div>
          </div>

          <template v-if="needsLogin">
            <p v-if="!recoveryNotice" class="status-copy">{{ status.message }}</p>
            <div v-if="qrImage" class="qr-wrap">
              <div class="qr-frame">
                <img :src="qrImage" alt="用于本次临时任务的 QQ 登录二维码" />
              </div>
              <p>请在 QQ 中打开“扫一扫”，并在手机上确认登录。</p>
              <a class="qr-save-link" :href="qrImage" download="qzone-login-qr.png">
                同一部手机？先保存二维码，再从相册识别
              </a>
            </div>
            <div class="card-actions login-actions">
              <button v-if="!qrImage" class="primary-button" type="button" :disabled="busy" @click="requestQr">
                {{ busy ? '正在连接 QQ…' : '生成一次性登录二维码' }}
              </button>
              <button v-else class="secondary-button" type="button" :disabled="busy" @click="requestQr">
                刷新二维码
              </button>
            </div>
            <PrivacyPanel compact />
          </template>

          <template v-else>
            <div class="options">
              <label class="option-row" for="include-media">
                <input id="include-media" v-model="includeMedia" type="checkbox" />
                <span><b>同时下载可用图片和视频</b><small>更完整，但耗时和文件体积会明显增加</small></span>
              </label>
              <label class="field-row" for="page-delay">
                <span><b>请求节奏</b><small>越慢越不容易触发 QQ 风控</small></span>
                <select id="page-delay" v-model="pageDelayMs">
                  <option :value="3000">稳妥 · 约 3 秒/页</option>
                  <option :value="5000">保守 · 约 5 秒/页</option>
                  <option :value="8000">最保守 · 约 8 秒/页</option>
                </select>
              </label>
            </div>
            <div class="card-actions archive-actions">
              <button
                class="primary-button"
                type="button"
                :disabled="busy || !canStart"
                @click="startArchive(includeMedia, pageDelayMs)"
              >
                {{ busy ? '正在加入队列…' : needsAttention ? '从已有断点继续' : '开始临时归档' }}
              </button>
              <button v-if="needsAttention" class="text-button" type="button" :disabled="busy" @click="requestQr">
                重新扫码验证 QQ
              </button>
            </div>
          </template>
        </section>

        <section v-if="status" class="task-footnote" aria-label="临时任务信息">
          <span>任务 {{ status.jobId.slice(0, 8) }}</span>
          <span id="task-retention">剩余保留时间约 {{ ttl }}</span>
          <button type="button" aria-describedby="task-retention" :disabled="busy || active" @click="confirmDelete">
            删除任务
          </button>
        </section>
      </section>

      <section class="boundary-grid" aria-label="归档边界与隐私说明">
        <article>
          <span aria-hidden="true">01</span>
          <h3>QQ 能返回多少，我们保存多少</h3>
          <p>未进入互动列表、已经永久删除或媒体地址过期的内容，任何工具都无法保证找回。</p>
        </article>
        <article>
          <span aria-hidden="true">02</span>
          <h3>所有结果归你自己保管</h3>
          <p>下载包可直接离线浏览，也包含 JSONL 与 SQLite，方便长期备份和二次整理。</p>
        </article>
        <article>
          <span aria-hidden="true">03</span>
          <h3>没有账号库，也没有云相册</h3>
          <p>服务器只保留当前任务需要的临时文件，到期、下载后或手动删除时一并清理。</p>
        </article>
      </section>
    </main>

    <footer>
      <p>非腾讯、QQ 或 QQ 空间官方产品。仅用于备份本人或已获授权的数据。</p>
      <a href="https://github.com/YouRen1320/qzone-archive-web" target="_blank" rel="noreferrer">查看开源代码</a>
    </footer>
  </div>
</template>
