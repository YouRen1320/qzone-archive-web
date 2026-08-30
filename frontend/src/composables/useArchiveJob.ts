import { computed, onScopeDispose, ref } from 'vue'
import type { ApiErrorBody, JobStatus, LoginResult } from '../types/job'

const POLL_INTERVAL_MS = 2_000

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    credentials: 'same-origin',
    cache: 'no-store',
    ...init,
    headers: init?.body
      ? { 'Content-Type': 'application/json', ...init.headers }
      : init?.headers,
  })
  if (!response.ok) {
    const body = (await response.json().catch(() => ({}))) as ApiErrorBody
    throw new Error(body.error?.message || `请求失败（${response.status}）`)
  }
  if (response.status === 204) return undefined as T
  return response.json() as Promise<T>
}

// This composable owns all remote side effects: private job creation, QR polling, SSE updates, and deletion.
export function useArchiveJob() {
  const status = ref<JobStatus | null>(null)
  const qrImage = ref('')
  const busy = ref(false)
  const error = ref('')
  let loginTimer: number | undefined
  let eventSource: EventSource | undefined

  const active = computed(() =>
    !!status.value && ['queued', 'archiving', 'downloadingMedia', 'packaging'].includes(status.value.phase),
  )

  async function initialize() {
    busy.value = true
    error.value = ''
    try {
      status.value = await request<JobStatus>('/api/job').catch(() =>
        request<JobStatus>('/api/jobs', { method: 'POST' }),
      )
      connectEvents()
    } catch (reason) {
      error.value = messageOf(reason)
    } finally {
      busy.value = false
    }
  }

  function connectEvents() {
    eventSource?.close()
    eventSource = new EventSource('/api/events', { withCredentials: true })
    eventSource.addEventListener('status', (event) => {
      try {
        status.value = JSON.parse((event as MessageEvent<string>).data) as JobStatus
      } catch {
        // A malformed event is ignored; the next server update remains authoritative.
      }
    })
  }

  async function requestQr() {
    busy.value = true
    error.value = ''
    stopLoginPolling()
    try {
      const response = await request<{ qrImage: string; message: string }>('/api/login/qr', {
        method: 'POST',
      })
      qrImage.value = response.qrImage
      loginTimer = window.setInterval(() => void pollLogin(), POLL_INTERVAL_MS)
    } catch (reason) {
      error.value = messageOf(reason)
    } finally {
      busy.value = false
    }
  }

  async function pollLogin() {
    try {
      const result = await request<LoginResult>('/api/login/poll', { method: 'POST' })
      if (result.status === 'success') {
        stopLoginPolling()
        qrImage.value = ''
      } else if (result.status === 'expired' || result.status === 'error') {
        stopLoginPolling()
      }
    } catch (reason) {
      stopLoginPolling()
      error.value = messageOf(reason)
    }
  }

  async function startArchive(includeMedia: boolean, pageDelayMs: number) {
    busy.value = true
    error.value = ''
    try {
      status.value = await request<JobStatus>('/api/archive', {
        method: 'POST',
        body: JSON.stringify({ includeMedia, pageDelayMs }),
      })
    } catch (reason) {
      error.value = messageOf(reason)
    } finally {
      busy.value = false
    }
  }

  async function cancelArchive() {
    busy.value = true
    error.value = ''
    try {
      status.value = await request<JobStatus>('/api/archive/cancel', { method: 'POST' })
    } catch (reason) {
      error.value = messageOf(reason)
    } finally {
      busy.value = false
    }
  }

  async function deleteJob() {
    busy.value = true
    error.value = ''
    try {
      stopLoginPolling()
      eventSource?.close()
      await request<void>('/api/job', { method: 'DELETE' })
      status.value = null
      qrImage.value = ''
      await initialize()
    } catch (reason) {
      error.value = messageOf(reason)
    } finally {
      busy.value = false
    }
  }

  function stopLoginPolling() {
    if (loginTimer !== undefined) {
      window.clearInterval(loginTimer)
      loginTimer = undefined
    }
  }

  onScopeDispose(() => {
    stopLoginPolling()
    eventSource?.close()
  })

  return {
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
  }
}

function messageOf(reason: unknown): string {
  return reason instanceof Error ? reason.message : '发生了未知错误，请稍后重试'
}
