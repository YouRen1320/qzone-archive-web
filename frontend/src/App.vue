<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import PrivacyPanel from './components/PrivacyPanel.vue'
import StepRail from './components/StepRail.vue'
import { useArchiveJob } from './composables/useArchiveJob'
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

const currentStep = computed(() => {
  if (!status.value) return 1
  if (status.value.phase === 'ready') return 3
  if (status.value.loggedIn || ['queued', 'archiving', 'downloadingMedia', 'packaging', 'failed', 'cancelled', 'interrupted'].includes(status.value.phase)) return 2
  return 1
})

const mediaPercent = computed(() => mediaProgress(status.value))
const ttl = computed(() => (status.value ? remainingLabel(status.value.expiresAt, now.value) : ''))
const canStart = computed(() => status.value?.loggedIn && !active.value)
const needsLogin = computed(() => !status.value?.loggedIn && status.value?.phase !== 'ready' && !active.value)

onMounted(() => {
  void initialize()
  window.setInterval(() => (now.value = Date.now()), 30_000)
})

async function confirmDelete() {
  if (window.confirm('确认立即删除这个任务及服务器上的全部临时数据吗？')) {
    await deleteJob()
  }
}
</script>

<template>
  <div class="page-shell">
    <header class="site-header">
      <a class="brand" href="/" aria-label="拾光册首页">
        <span class="brand-mark">拾</span>
        <span><b>拾光册</b><small>QQ 空间临时归档</small></span>
      </a>
      <span class="privacy-chip"><i></i>服务器不留长期副本</span>
    </header>

    <main>
      <section class="hero">
        <p class="eyebrow">把旧时光，带回自己的设备</p>
        <h1>恢复能找到的，<em>带走属于你的。</em></h1>
        <p class="hero-copy">扫码后临时整理 QQ 仍能返回的互动记录、图片与视频，完成后打包下载到电脑或手机。</p>
      </section>

      <StepRail :current="currentStep" />

      <div v-if="error" class="alert" role="alert">
        <span>!</span><p>{{ error }}</p><button type="button" aria-label="关闭错误" @click="error = ''">×</button>
      </div>

      <section v-if="!status" class="work-card loading-card">
        <div class="spinner"></div><p>正在建立私密临时任务…</p>
      </section>

      <section v-else-if="status.phase === 'ready'" class="work-card ready-card">
        <div class="ready-seal">✓</div>
        <p class="card-kicker">归档包已准备好</p>
        <h2>{{ status.saved.toLocaleString() }} 条记录，等你带走</h2>
        <p class="status-copy">{{ status.message }}</p>
        <div class="summary-grid">
          <div><span>互动记录</span><b>{{ status.saved.toLocaleString() }}</b></div>
          <div><span>已下载媒体</span><b>{{ status.mediaDownloaded.toLocaleString() }}</b></div>
          <div><span>媒体失败</span><b>{{ status.mediaFailed.toLocaleString() }}</b></div>
        </div>
        <a class="primary-button download-button" href="/api/download">下载 ZIP 到本机</a>
        <p class="tiny-note">包内含离线网页、原始 JSONL 和独立 SQLite；不包含 QQ Cookie。</p>
        <button class="text-button danger" type="button" :disabled="busy" @click="confirmDelete">现在就删除服务器临时文件</button>
      </section>

      <section v-else-if="active" class="work-card progress-card">
        <p class="card-kicker">{{ status.phase === 'queued' ? '安全排队中' : '正在临时归档' }}</p>
        <h2>{{ status.message }}</h2>
        <div class="pulse-line"><i></i></div>
        <div class="summary-grid">
          <div><span>已读取页数</span><b>{{ status.pages.toLocaleString() }}</b></div>
          <div><span>唯一记录</span><b>{{ status.saved.toLocaleString() }}</b></div>
          <div><span>媒体进度</span><b>{{ Math.round(mediaPercent) }}%</b></div>
        </div>
        <p class="tiny-note">请保持本页面打开。即使网络短暂断开，已经提交到临时 SQLite 的分页也不会丢失。</p>
        <button class="secondary-button" type="button" :disabled="busy" @click="cancelArchive">安全停止</button>
      </section>

      <section v-else class="work-card login-card">
        <div class="card-heading">
          <div>
            <p class="card-kicker">{{ needsLogin ? '第 1 步 · QQ 安全登录' : '第 2 步 · 选择归档内容' }}</p>
            <h2>{{ needsLogin ? '用手机 QQ 扫码确认' : `已登录 ${status.maskedUin || ''}` }}</h2>
          </div>
          <span v-if="status.loggedIn" class="login-ok">已验证</span>
        </div>

        <template v-if="needsLogin">
          <p class="status-copy">{{ status.message }}</p>
          <div v-if="qrImage" class="qr-wrap">
            <div class="qr-frame"><img :src="qrImage" alt="QQ 登录二维码" /></div>
            <p>请在 QQ 中打开“扫一扫”，并在手机上确认登录。</p>
            <a :href="qrImage" download="qzone-login-qr.png">同一部手机？先保存二维码，再从相册识别</a>
          </div>
          <button v-else class="primary-button" type="button" :disabled="busy" @click="requestQr">
            {{ busy ? '正在连接 QQ…' : '生成一次性登录二维码' }}
          </button>
          <button v-if="qrImage" class="secondary-button" type="button" :disabled="busy" @click="requestQr">刷新二维码</button>
          <PrivacyPanel compact />
        </template>

        <template v-else>
          <div class="options">
            <label class="option-row">
              <input v-model="includeMedia" type="checkbox" />
              <span><b>同时下载可用图片和视频</b><small>更完整，但耗时和文件体积会明显增加</small></span>
            </label>
            <label class="field-row">
              <span><b>请求节奏</b><small>越慢越不容易触发 QQ 风控</small></span>
              <select v-model="pageDelayMs">
                <option :value="3000">稳妥 · 约 3 秒/页</option>
                <option :value="5000">保守 · 约 5 秒/页</option>
                <option :value="8000">最保守 · 约 8 秒/页</option>
              </select>
            </label>
          </div>
          <button class="primary-button" type="button" :disabled="busy || !canStart" @click="startArchive(includeMedia, pageDelayMs)">
            {{ busy ? '正在加入队列…' : '开始临时归档' }}
          </button>
          <button v-if="['failed', 'cancelled', 'interrupted'].includes(status.phase)" class="text-button" type="button" @click="requestQr">重新扫码后继续断点</button>
        </template>
      </section>

      <section v-if="status" class="task-footnote">
        <span>任务 {{ status.jobId.slice(0, 8) }}</span><span>剩余保留时间约 {{ ttl }}</span>
        <button type="button" :disabled="busy || active" @click="confirmDelete">删除任务</button>
      </section>

      <section class="boundary-grid">
        <article><span>01</span><h3>QQ 能返回多少，我们保存多少</h3><p>未进入互动列表、已经永久删除或媒体地址过期的内容，任何工具都无法保证找回。</p></article>
        <article><span>02</span><h3>所有结果归你自己保管</h3><p>下载包可直接离线浏览，也包含 JSONL 与 SQLite，方便长期备份和二次整理。</p></article>
        <article><span>03</span><h3>没有账号库，也没有云相册</h3><p>服务器只保留当前任务需要的临时文件，到期、下载后或手动删除时一并清理。</p></article>
      </section>
    </main>

    <footer>
      <p>非腾讯、QQ 或 QQ 空间官方产品。仅用于备份本人或已获授权的数据。</p>
      <a href="https://github.com/YouRen1320/qzone-archive-web" target="_blank" rel="noreferrer">查看开源代码</a>
    </footer>
  </div>
</template>
