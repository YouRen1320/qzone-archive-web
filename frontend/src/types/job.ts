export type JobPhase =
  | 'awaitingLogin'
  | 'loggedIn'
  | 'queued'
  | 'archiving'
  | 'downloadingMedia'
  | 'packaging'
  | 'ready'
  | 'paused'
  | 'cancelled'
  | 'failed'
  | 'interrupted'

// JobStatus contains only non-sensitive progress; QQ cookies never cross the API boundary.
export interface JobStatus {
  jobId: string
  phase: JobPhase
  message: string
  createdAt: number
  expiresAt: number
  loggedIn: boolean
  maskedUin: string | null
  pages: number
  fetched: number
  saved: number
  mediaTotal: number
  mediaDownloaded: number
  mediaFailed: number
  includeMedia: boolean
  downloadReady: boolean
  downloadedAt: number | null
}

export interface LoginResult {
  status: 'waiting' | 'scanned' | 'expired' | 'success' | 'error'
  message: string
  maskedUin: string | null
}

export interface ApiErrorBody {
  error?: {
    code?: string
    message?: string
  }
}
