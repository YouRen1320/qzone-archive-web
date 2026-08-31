import { effectScope, type EffectScope } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useArchiveJob } from '../composables/useArchiveJob'
import type { JobPhase, JobStatus, LoginResult } from '../types/job'

const POLL_INTERVAL_MS = 2_000
const scopes: EffectScope[] = []

// The fixture deliberately mirrors only the public job contract; credentials never enter these tests.
function makeStatus(phase: JobPhase, overrides: Partial<JobStatus> = {}): JobStatus {
  const loggedIn = !['awaitingLogin', 'paused', 'interrupted', 'ready'].includes(phase)
  return {
    jobId: `job-${phase}`,
    phase,
    message: `任务状态：${phase}`,
    createdAt: 1_700_000_000,
    expiresAt: 1_700_021_600,
    lastActivityAt: 1_700_000_000,
    runStartedAt: null,
    lastProgressAt: null,
    queuedAhead: 0,
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

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function emptyResponse(status = 204): Response {
  return new Response(null, { status })
}

type StoredListener = (event: Event) => void

// This in-memory EventSource records lifecycle effects and lets tests deliver server events deterministically.
class MockEventSource {
  static instances: MockEventSource[] = []

  readonly url: string
  readonly withCredentials: boolean
  readonly close = vi.fn()
  private readonly listeners = new Map<string, StoredListener[]>()

  constructor(url: string | URL, init?: EventSourceInit) {
    this.url = String(url)
    this.withCredentials = init?.withCredentials ?? false
    MockEventSource.instances.push(this)
  }

  addEventListener(type: string, listener: EventListenerOrEventListenerObject | null) {
    if (!listener) return
    const invoke: StoredListener = typeof listener === 'function'
      ? listener
      : (event) => listener.handleEvent(event)
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), invoke])
  }

  emit(type: string, data: string) {
    const event = new MessageEvent(type, { data })
    this.listeners.get(type)?.forEach((listener) => listener(event))
  }
}

function createJob() {
  const scope = effectScope()
  scopes.push(scope)
  const job = scope.run(() => useArchiveJob())
  if (!job) throw new Error('useArchiveJob must run inside the active test scope')
  return { job, scope }
}

function mockFetchSequence(...responses: Array<Response | Error>) {
  const fetchMock = vi.mocked(fetch)
  responses.forEach((response) => {
    if (response instanceof Error) fetchMock.mockRejectedValueOnce(response)
    else fetchMock.mockResolvedValueOnce(response)
  })
  return fetchMock
}

function loginResult(status: LoginResult['status'], message: string = status): LoginResult {
  return { status, message, maskedUin: status === 'success' ? '12****34' : null }
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal('fetch', vi.fn())
  vi.stubGlobal('EventSource', MockEventSource)
  MockEventSource.instances = []
})

afterEach(() => {
  scopes.splice(0).forEach((scope) => scope.stop())
  vi.clearAllTimers()
  vi.useRealTimers()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('private task initialization and events', () => {
  it('falls back from GET recovery to POST creation, then connects the private status stream', async () => {
    const created = makeStatus('awaitingLogin')
    const fetchMock = mockFetchSequence(
      jsonResponse({ error: { message: '任务不存在' } }, 404),
      jsonResponse(created, 201),
    )
    const { job } = createJob()

    await job.initialize()

    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(fetchMock.mock.calls[0]).toEqual([
      '/api/job',
      expect.objectContaining({ credentials: 'same-origin', cache: 'no-store' }),
    ])
    expect(fetchMock.mock.calls[1]).toEqual([
      '/api/jobs',
      expect.objectContaining({ method: 'POST', credentials: 'same-origin', cache: 'no-store' }),
    ])
    expect(job.status.value).toEqual(created)
    expect(job.error.value).toBe('')
    expect(job.busy.value).toBe(false)
    expect(MockEventSource.instances).toHaveLength(1)
    expect(MockEventSource.instances[0]).toMatchObject({ url: '/api/events', withCredentials: true })
  })

  it('accepts valid status events and ignores malformed status payloads', async () => {
    const initial = makeStatus('loggedIn')
    const updated = makeStatus('archiving', { pages: 8, saved: 72 })
    mockFetchSequence(jsonResponse(initial))
    const { job } = createJob()
    await job.initialize()
    const source = MockEventSource.instances[0]

    source.emit('status', JSON.stringify(updated))
    expect(job.status.value).toEqual(updated)

    source.emit('status', '{not-json')
    expect(job.status.value).toEqual(updated)
    expect(job.error.value).toBe('')
  })
})

describe('one-time QR polling', () => {
  it('clears the QR image and stops polling after a successful login', async () => {
    const fetchMock = mockFetchSequence(
      jsonResponse({ qrImage: 'data:image/png;base64,cXpvbmU=', message: '请扫码' }),
      jsonResponse(loginResult('success')),
    )
    const { job } = createJob()

    await job.requestQr()
    expect(job.qrImage.value).toContain('data:image/png')
    expect(vi.getTimerCount()).toBe(1)

    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS)

    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(fetchMock.mock.calls[1]).toEqual([
      '/api/login/poll',
      expect.objectContaining({ method: 'POST', credentials: 'same-origin' }),
    ])
    expect(job.qrImage.value).toBe('')
    expect(job.error.value).toBe('')
    expect(vi.getTimerCount()).toBe(0)

    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS * 3)
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('stops polling an expired QR without discarding the image before the UI can replace it', async () => {
    const fetchMock = mockFetchSequence(
      jsonResponse({ qrImage: 'data:image/png;base64,ZXhwaXJlZA==', message: '请扫码' }),
      jsonResponse(loginResult('expired', '二维码已过期')),
    )
    const { job } = createJob()

    await job.requestQr()
    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS)

    expect(job.qrImage.value).toContain('data:image/png')
    expect(job.error.value).toBe('')
    expect(vi.getTimerCount()).toBe(0)
    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS * 3)
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('stops polling when QQ returns an explicit login error state', async () => {
    const fetchMock = mockFetchSequence(
      jsonResponse({ qrImage: 'data:image/png;base64,cXEtZXJyb3I=', message: '请扫码' }),
      jsonResponse(loginResult('error', 'QQ 拒绝了本次登录')),
    )
    const { job } = createJob()

    await job.requestQr()
    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS)

    expect(job.qrImage.value).toContain('data:image/png')
    expect(vi.getTimerCount()).toBe(0)
    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS * 3)
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('reports a polling exception and prevents any further login requests', async () => {
    const fetchMock = mockFetchSequence(
      jsonResponse({ qrImage: 'data:image/png;base64,ZXJyb3I=', message: '请扫码' }),
      jsonResponse({ error: { message: 'QQ 登录服务暂时不可用' } }, 503),
    )
    const { job } = createJob()

    await job.requestQr()
    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS)

    expect(job.error.value).toBe('QQ 登录服务暂时不可用')
    expect(vi.getTimerCount()).toBe(0)
    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS * 3)
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })
})

describe('resource cleanup', () => {
  it('closes the event stream and clears QR polling when its Vue scope is disposed', async () => {
    mockFetchSequence(
      jsonResponse(makeStatus('awaitingLogin')),
      jsonResponse({ qrImage: 'data:image/png;base64,c2NvcGU=', message: '请扫码' }),
    )
    const { job, scope } = createJob()
    await job.initialize()
    await job.requestQr()
    const source = MockEventSource.instances[0]
    expect(vi.getTimerCount()).toBe(1)

    scope.stop()

    expect(source.close).toHaveBeenCalledOnce()
    expect(vi.getTimerCount()).toBe(0)
  })

  it('deletes old state and closes its resources without allocating a replacement task', async () => {
    const oldStatus = makeStatus('awaitingLogin', { jobId: 'job-old' })
    const fetchMock = mockFetchSequence(
      jsonResponse(oldStatus),
      jsonResponse({ qrImage: 'data:image/png;base64,b2xk', message: '请扫码' }),
      emptyResponse(),
    )
    const { job } = createJob()
    await job.initialize()
    await job.requestQr()
    const oldSource = MockEventSource.instances[0]
    expect(job.qrImage.value).not.toBe('')
    expect(vi.getTimerCount()).toBe(1)

    await job.deleteJob()

    expect(fetchMock.mock.calls.map(([path]) => path)).toEqual([
      '/api/job',
      '/api/login/qr',
      '/api/job',
    ])
    expect(fetchMock.mock.calls[2][1]).toEqual(expect.objectContaining({ method: 'DELETE' }))
    expect(oldSource.close).toHaveBeenCalled()
    expect(MockEventSource.instances).toHaveLength(1)
    expect(job.status.value).toBeNull()
    expect(job.qrImage.value).toBe('')
    expect(job.error.value).toBe('')
    expect(job.busy.value).toBe(false)
    expect(vi.getTimerCount()).toBe(0)
  })

  it('can look for a restored task without creating one for a local-only viewer', async () => {
    const fetchMock = mockFetchSequence(jsonResponse({ error: { message: '任务不存在' } }, 404))
    const { job } = createJob()

    await job.initialize(false)

    expect(fetchMock).toHaveBeenCalledOnce()
    expect(job.status.value).toBeNull()
    expect(job.error.value).toBe('')
    expect(MockEventSource.instances).toHaveLength(0)
  })
})
