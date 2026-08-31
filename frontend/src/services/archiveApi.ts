import type { ArchiveManifest, ArchivePage, ArchivePageQuery } from '../types/archive'

// These endpoints expose only the current cookie-owned task. QQ credentials never cross this API.
export async function fetchArchiveManifest(signal?: AbortSignal): Promise<ArchiveManifest> {
  return requestJson<ArchiveManifest>('/api/archive/viewer/manifest', signal)
}

export async function fetchArchivePage(query: ArchivePageQuery, signal?: AbortSignal): Promise<ArchivePage> {
  const params = new URLSearchParams({ offset: String(query.offset), limit: String(query.limit) })
  if (query.search) params.set('q', query.search)
  if (query.category) params.set('category', query.category)
  if (query.year) params.set('year', String(query.year))
  return requestJson<ArchivePage>(`/api/archive/viewer/records?${params}`, signal)
}

export function archiveMediaUrl(path: string): string {
  const normalized = path.replaceAll('\\', '/')
  if (!normalized.startsWith('media/') || normalized.includes('/../') || normalized.includes('//')) {
    throw new Error('归档中包含无效的媒体路径')
  }
  return `/api/archive/viewer/media/${normalized.slice('media/'.length).split('/').map(encodeURIComponent).join('/')}`
}

async function requestJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const response = await fetch(path, { credentials: 'same-origin', cache: 'no-store', signal })
  if (!response.ok) {
    const body = await response.json().catch(() => null) as { error?: { message?: string } } | null
    throw new Error(body?.error?.message || `打开回忆册失败（${response.status}）`)
  }
  return response.json() as Promise<T>
}
