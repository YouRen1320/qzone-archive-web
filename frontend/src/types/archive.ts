export type ArchiveCategory = 'self' | 'other' | 'guestbook'

// ArchiveRecord is the versioned, non-executable contract returned from task-local SQLite.
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

export interface ArchivePage {
  items: ArchiveRecord[]
  total: number
  offset: number
  nextOffset: number | null
  years: number[]
}

export interface ArchivePageQuery {
  offset: number
  limit: number
  search: string
  category: ArchiveCategory | ''
  year: number | ''
}
