export type ArchiveCategory = 'self' | 'other' | 'guestbook'

// ArchiveRecord is the versioned, non-executable viewing contract shared by server and local ZIP sources.
export interface ArchiveRecord {
  id: number
  cellId: string
  publishedAt: number
  content: string | null
  authorName: string | null
  category: ArchiveCategory
  media: string[]
}

export interface ArchiveManifest {
  formatVersion: 1 | 2
  generatedAt: number
  complete: boolean
  records: number
  mediaDownloaded: number
  mediaFailed: number
  source: string
  notice: string
  recordsFile?: string
  mediaRoot?: string
}

export interface ArchiveMediaHandle {
  url: string
  size: number | null
  release: () => void
}

export interface ArchiveSession {
  kind: 'server' | 'local'
  label: string
  manifest: ArchiveManifest
  records: ArchiveRecord[]
  mediaSize: (path: string) => number | null
  openMedia: (path: string) => Promise<ArchiveMediaHandle>
  close: () => Promise<void>
}
