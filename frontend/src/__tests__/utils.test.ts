import { describe, expect, it } from 'vitest'
import type { JobStatus } from '../types/job'
import { mediaProgress, remainingLabel } from '../utils'

const status: JobStatus = {
  jobId: 'job',
  phase: 'downloadingMedia',
  message: 'working',
  createdAt: 1,
  expiresAt: 10,
  loggedIn: true,
  maskedUin: '12****34',
  pages: 1,
  fetched: 2,
  saved: 2,
  mediaTotal: 4,
  mediaDownloaded: 2,
  mediaFailed: 1,
  includeMedia: true,
  downloadReady: false,
  downloadedAt: null,
}

describe('progress presentation', () => {
  it('counts failed media as completed work', () => {
    expect(mediaProgress(status)).toBe(75)
  })

  it('formats expiration without exposing timestamps', () => {
    expect(remainingLabel(3_700, 100_000)).toBe('1 小时')
    expect(remainingLabel(100, 100_000)).toBe('0 秒')
  })
})
