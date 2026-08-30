import type { JobStatus } from './types/job'

export function mediaProgress(status: JobStatus | null): number {
  if (!status || status.mediaTotal === 0) return 0
  return Math.min(100, ((status.mediaDownloaded + status.mediaFailed) / status.mediaTotal) * 100)
}

export function remainingLabel(expiresAt: number, current = Date.now()): string {
  const seconds = Math.max(0, expiresAt - Math.floor(current / 1000))
  if (seconds < 60) return `${seconds} 秒`
  const minutes = Math.ceil(seconds / 60)
  if (minutes < 60) return `${minutes} 分钟`
  return `${Math.ceil(minutes / 60)} 小时`
}

export function isActive(status: JobStatus | null): boolean {
  return !!status && ['queued', 'archiving', 'downloadingMedia', 'packaging'].includes(status.phase)
}
