import { flushPromises, mount, type DOMWrapper, type VueWrapper } from '@vue/test-utils'
import axe from 'axe-core'
import { computed, nextTick, ref } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import App from '../App.vue'
import type { JobPhase, JobStatus } from '../types/job'

const useArchiveJobMock = vi.hoisted(() => vi.fn())

vi.mock('../composables/useArchiveJob', () => ({
  useArchiveJob: useArchiveJobMock,
}))

const phases: JobPhase[] = [
  'awaitingLogin',
  'loggedIn',
  'queued',
  'archiving',
  'downloadingMedia',
  'packaging',
  'ready',
  'paused',
  'cancelled',
  'failed',
  'interrupted',
]

const activePhases: JobPhase[] = ['queued', 'archiving', 'downloadingMedia', 'packaging']
const mountedWrappers: VueWrapper[] = []

const phaseExpectations = [
  ['awaitingLogin', 'login-card', /用手机 QQ 扫一下/, /显示二维码/],
  ['loggedIn', 'options-card', /这次带走什么/, /开始整理/],
  ['queued', 'progress-card', /任务状态：queued/, /安全停止/],
  ['archiving', 'progress-card', /任务状态：archiving/, /安全停止/],
  ['downloadingMedia', 'progress-card', /任务状态：downloadingMedia/, /安全停止/],
  ['packaging', 'progress-card', /任务状态：packaging/, /安全停止/],
  ['ready', 'ready-card', /24 条记录，4 个媒体文件/, /保存到这台设备/],
  ['paused', 'login-card', /归档停在这里/, /显示二维码/],
  ['cancelled', 'options-card', /这次已经安全停下/, /从已有断点继续/],
  ['failed', 'login-card', /这次没有走完/, /显示二维码/],
  ['interrupted', 'login-card', /服务器回来，登录已经失效/, /显示二维码/],
] satisfies Array<[JobPhase, string, RegExp, RegExp]>

// Each phase fixture deliberately contains only public progress fields, mirroring the API contract.
function makeStatus(phase: JobPhase, overrides: Partial<JobStatus> = {}): JobStatus {
  const loggedIn = !['awaitingLogin', 'paused', 'interrupted', 'ready'].includes(phase)
  return {
    jobId: 'job-12345678-private-suffix',
    phase,
    message: `任务状态：${phase}`,
    createdAt: 1_700_000_000,
    expiresAt: Math.floor(Date.now() / 1_000) + 21_600,
    loggedIn,
    maskedUin: loggedIn ? '12****34' : null,
    pages: 3,
    fetched: 27,
    saved: 24,
    mediaTotal: 10,
    mediaDownloaded: 4,
    mediaFailed: 1,
    includeMedia: true,
    downloadReady: phase === 'ready',
    downloadedAt: null,
    ...overrides,
  }
}

// The composable double keeps every test local and guarantees that no real QQ or archive request is sent.
function installJobMock(statusValue: JobStatus | null, options: { qrImage?: string; error?: string; busy?: boolean } = {}) {
  const status = ref<JobStatus | null>(statusValue)
  const qrImage = ref(options.qrImage ?? '')
  const busy = ref(options.busy ?? false)
  const error = ref(options.error ?? '')
  const active = computed(() => !!status.value && activePhases.includes(status.value.phase))
  const controls = {
    status,
    qrImage,
    busy,
    error,
    active,
    initialize: vi.fn().mockResolvedValue(undefined),
    requestQr: vi.fn().mockResolvedValue(undefined),
    startArchive: vi.fn().mockResolvedValue(undefined),
    cancelArchive: vi.fn().mockResolvedValue(undefined),
    deleteJob: vi.fn().mockResolvedValue(undefined),
  }
  useArchiveJobMock.mockReturnValue(controls)
  return controls
}

// Mounting into document.body lets axe evaluate labels, landmarks, and relationships as users receive them.
async function mountApp() {
  const host = document.createElement('div')
  document.body.append(host)
  const wrapper = mount(App, { attachTo: host })
  mountedWrappers.push(wrapper)
  await flushPromises()
  const entrance = wrapper.find('.chapter--intro button')
  if (entrance.exists() && !(entrance.element as HTMLButtonElement).disabled) {
    await entrance.trigger('click')
    await nextTick()
  }
  return wrapper
}

function findByText(wrapper: VueWrapper, selector: string, pattern: RegExp): DOMWrapper<Element> {
  const element = wrapper.findAll(selector).find((candidate) => pattern.test(candidate.text()))
  if (!element) throw new Error(`找不到匹配 ${pattern} 的 ${selector}`)
  return element
}

beforeEach(() => {
  useArchiveJobMock.mockReset()
  // App's clock is unrelated to these tests; replacing it prevents background timers from leaking between cases.
  vi.spyOn(window, 'setInterval').mockReturnValue(1 as unknown as ReturnType<typeof window.setInterval>)
})

afterEach(() => {
  mountedWrappers.splice(0).forEach((wrapper) => wrapper.unmount())
  document.body.replaceChildren()
  vi.restoreAllMocks()
})

describe('complete task-state rendering', () => {
  it('shows an intentional restore screen while the private task is loading', async () => {
    const job = installJobMock(null)
    const wrapper = await mountApp()

    expect(job.initialize).toHaveBeenCalledOnce()
    expect(wrapper.get('main').text()).toMatch(/建立|恢复|任务/)
    expect(wrapper.get('main').text().trim()).not.toBe('')
  })

  it.each(phaseExpectations)(
    'renders the phase-specific card, status, and primary action for %s',
    async (phase, cardClass, expectedCopy, expectedAction) => {
      installJobMock(makeStatus(phase))
      const wrapper = await mountApp()

      expect(wrapper.find(`.${cardClass}`).exists()).toBe(true)
      expect(wrapper.text()).toContain(`任务状态：${phase}`)
      expect(wrapper.text()).toMatch(expectedCopy)
      expect(findByText(wrapper, 'button, a', expectedAction).text()).toMatch(expectedAction)
    },
  )

  it.each([
    ['awaitingLogin', false, '扫码'],
    ['loggedIn', true, '选择'],
    ['queued', true, '整理'],
    ['archiving', true, '整理'],
    ['downloadingMedia', true, '装包'],
    ['packaging', true, '装包'],
    ['paused', false, '扫码'],
    ['cancelled', false, '扫码'],
    ['failed', false, '扫码'],
    ['interrupted', false, '扫码'],
    ['ready', false, '保存'],
  ] satisfies Array<[JobPhase, boolean, string]>)('%s maps to the expected workflow step', async (phase, loggedIn, stepLabel) => {
    installJobMock(makeStatus(phase, { loggedIn, maskedUin: loggedIn ? '12****34' : null }))
    const wrapper = await mountApp()

    const current = wrapper.get('[aria-current="step"]')
    expect(current.text()).toContain(stepLabel)
    expect(wrapper.findAll('[aria-current="step"]')).toHaveLength(1)
  })

  it.each([
    ['paused', false, '归档停在这里', '已经落盘的分页还在'],
    ['cancelled', true, '这次已经安全停下', '已经整理好的分页还在'],
    ['failed', true, '这次没有走完', '重新扫一下验证 QQ'],
    ['interrupted', false, '服务器回来，登录已经失效', '重新扫码后，可以从已经保存的位置继续'],
  ] satisfies Array<[JobPhase, boolean, string, string]>)(
    'keeps unique recovery guidance for %s',
    async (phase, loggedIn, title, guidance) => {
      const message = `恢复状态消息：${phase}`
      installJobMock(makeStatus(phase, { loggedIn, maskedUin: loggedIn ? '12****34' : null, message }))
      const wrapper = await mountApp()

      expect(wrapper.text()).toContain(message)
      expect(wrapper.text()).toContain(title)
      expect(wrapper.text()).toContain(guidance)
      expect(wrapper.get('[aria-current="step"]').text()).toMatch(/扫码|选择/)
    },
  )

  it('continues a logged-in cancelled task with the newly selected archive arguments', async () => {
    const job = installJobMock(makeStatus('cancelled', { loggedIn: true, maskedUin: '12****34' }))
    const wrapper = await mountApp()

    await wrapper.get('input[type="checkbox"]').setValue(false)
    await wrapper.get('select').setValue('5000')
    await findByText(wrapper, 'button', /从已有断点继续/).trigger('click')

    expect(job.startArchive).toHaveBeenCalledWith(false, 5_000)
  })

  it('forces a failed task to revalidate QQ even when the backend still reports logged in', async () => {
    const job = installJobMock(makeStatus('failed', { loggedIn: true, maskedUin: '12****34' }))
    const wrapper = await mountApp()

    expect(wrapper.text()).toContain('重新扫一下，断点还在')
    expect(wrapper.text()).not.toContain('已验证')
    expect(wrapper.find('.options-card').exists()).toBe(false)
    await findByText(wrapper, 'button', /显示二维码/).trigger('click')
    expect(job.requestQr).toHaveBeenCalledOnce()
    expect(job.startArchive).not.toHaveBeenCalled()
  })

  it.each(['paused', 'interrupted'] satisfies JobPhase[])(
    'offers QR revalidation for logged-out %s recovery',
    async (phase) => {
      const job = installJobMock(makeStatus(phase, { loggedIn: false, maskedUin: null }))
      const wrapper = await mountApp()

      expect(wrapper.text()).toContain('重新扫一下，断点还在')
      await findByText(wrapper, 'button', /显示二维码/).trigger('click')
      expect(job.requestQr).toHaveBeenCalledOnce()
    },
  )
})

describe('login and archive controls', () => {
  it('generates and refreshes a one-time QR code without a network request in the test', async () => {
    const job = installJobMock(makeStatus('awaitingLogin'))
    let wrapper = await mountApp()

    await findByText(wrapper, 'button', /显示二维码/).trigger('click')
    expect(job.requestQr).toHaveBeenCalledOnce()

    wrapper.unmount()
    mountedWrappers.splice(mountedWrappers.indexOf(wrapper), 1)
    const qrJob = installJobMock(makeStatus('awaitingLogin'), {
      qrImage: 'data:image/png;base64,cXpvbmU=',
    })
    wrapper = await mountApp()

    expect(wrapper.get('img[alt*="QQ"][alt*="二维码"]').attributes('src')).toContain('data:image/png')
    expect(wrapper.get('a[download]').attributes('href')).toContain('data:image/png')
    await findByText(wrapper, 'button', /换一张二维码/).trigger('click')
    expect(qrJob.requestQr).toHaveBeenCalledOnce()
  })

  it('labels archive options and sends the chosen media and pacing values', async () => {
    const job = installJobMock(makeStatus('loggedIn'))
    const wrapper = await mountApp()
    const media = wrapper.get('input[type="checkbox"]')
    const pacing = wrapper.get('select')

    expect((media.element as HTMLInputElement).labels?.length).toBeGreaterThan(0)
    expect((pacing.element as HTMLSelectElement).labels?.length).toBeGreaterThan(0)
    expect(wrapper.text()).toContain('12****34')

    await media.setValue(false)
    await pacing.setValue('8000')
    await findByText(wrapper, 'button', /开始整理/).trigger('click')

    expect(job.startArchive).toHaveBeenCalledWith(false, 8_000)
  })
})

describe('progress, completion, and cleanup', () => {
  it.each(activePhases)('announces %s progress and allows a safe stop', async (phase) => {
    const status = makeStatus(phase)
    const job = installJobMock(status)
    const wrapper = await mountApp()

    expect(wrapper.get('[aria-live="polite"]').text()).toContain(status.message)
    const progress = wrapper.get('[role="progressbar"]')
    expect(progress.attributes('aria-valuemin')).toBe('0')
    expect(progress.attributes('aria-valuemax')).toBe('100')
    if (phase === 'downloadingMedia') {
      expect(progress.attributes('aria-valuenow')).toBe('50')
    } else {
      expect(progress.attributes('aria-valuenow')).toBeUndefined()
    }

    await findByText(wrapper, 'button', /安全停止|停止归档/).trigger('click')
    expect(job.cancelArchive).toHaveBeenCalledOnce()
  })

  it('presents ready statistics and a direct local ZIP download', async () => {
    installJobMock(makeStatus('ready', {
      message: '归档包已经准备好',
      saved: 1_234,
      mediaDownloaded: 98,
      mediaFailed: 7,
      downloadReady: true,
    }))
    const wrapper = await mountApp()

    expect(wrapper.text()).toContain((1_234).toLocaleString())
    expect(wrapper.text()).toContain((98).toLocaleString())
    expect(wrapper.text()).toContain((7).toLocaleString())
    expect(wrapper.get('a[href="/api/download"]').text()).toContain('保存到这台设备')
    expect(wrapper.get('[aria-current="step"]').text()).toContain('保存')
  })

  it('dismisses a reported error through an explicitly named control', async () => {
    installJobMock(makeStatus('awaitingLogin'), { error: 'QQ 暂时没有响应' })
    const wrapper = await mountApp()

    expect(wrapper.get('[role="alert"]').text()).toContain('QQ 暂时没有响应')
    await wrapper.get('button[aria-label*="关闭错误"]').trigger('click')
    await nextTick()
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
  })

  it('cancels and then confirms deletion from the ready card', async () => {
    const job = installJobMock(makeStatus('ready', { loggedIn: false, maskedUin: null }))
    const confirm = vi.spyOn(window, 'confirm')
      .mockReturnValueOnce(false)
      .mockReturnValueOnce(true)
    const wrapper = await mountApp()
    const deleteButton = findByText(wrapper, 'button', /立即删除服务器临时文件/)

    await deleteButton.trigger('click')
    expect(job.deleteJob).not.toHaveBeenCalled()

    await deleteButton.trigger('click')
    await flushPromises()
    expect(confirm).toHaveBeenCalledTimes(2)
    expect(job.deleteJob).toHaveBeenCalledOnce()
  })
})

describe('baseline accessibility', () => {
  it.each(['awaitingLogin', 'loggedIn', 'downloadingMedia', 'ready', 'interrupted'] satisfies JobPhase[])(
    'has no axe violations that jsdom can evaluate reliably for %s',
    async (phase) => {
      installJobMock(makeStatus(phase))
      const wrapper = await mountApp()
      const result = await axe.run(wrapper.element, {
        rules: {
          // jsdom has no layout or computed paint, so color contrast is verified in browser review instead.
          'color-contrast': { enabled: false },
        },
      })

      expect(result.violations).toEqual([])
    },
  )
})
