import {
  BlobReader,
  BlobWriter,
  configure,
  TextWriter,
  ZipReader,
  type FileEntry,
} from '@zip.js/zip.js'
import wasmURI from '@zip.js/zip.js/dist/zip-module.wasm?url'
import workerURI from '@zip.js/zip.js/dist/zip-web-worker.js?url'
import type {
  ArchiveCategory,
  ArchiveManifest,
  ArchiveMediaHandle,
  ArchiveRecord,
  ArchiveSession,
} from '../types/archive'

const MAX_ARCHIVE_BYTES = 8 * 1024 * 1024 * 1024
const MAX_ENTRIES = 25_000
const MAX_MANIFEST_BYTES = 1024 * 1024
const MAX_RECORDS_BYTES = 128 * 1024 * 1024
const MAX_RECORDS = 200_000
const LEGACY_DATA_PREFIX = 'window.__QZONE_ARCHIVE_DATA__='

// Keep large ZIP decompression away from the UI thread and serve the worker from our own origin.
configure({ workerURI, wasmURI })

export async function openServerArchive(): Promise<ArchiveSession> {
  const [manifest, records] = await Promise.all([
    requestJson<unknown>('/api/archive/viewer/manifest'),
    requestJson<unknown>('/api/archive/viewer/records'),
  ])
  return {
    kind: 'server',
    label: '这次刚整理好的回忆册',
    manifest: normalizeManifest(manifest),
    records: normalizeRecords(records),
    mediaSize: () => null,
    openMedia: async (path) => ({
      url: `/api/archive/viewer/media/${encodeMediaPath(path)}`,
      size: null,
      release: () => undefined,
    }),
    close: async () => undefined,
  }
}

// Local archives are inspected as random-access Blobs. No entry is uploaded or executed.
export async function openLocalArchive(file: File): Promise<ArchiveSession> {
  if (!isZipLike(file)) throw new Error('请选择拾光册导出的 ZIP 文件')
  if (file.size <= 0 || file.size > MAX_ARCHIVE_BYTES) {
    throw new Error('这个 ZIP 的大小超出本地查看器支持范围')
  }

  const reader = new ZipReader(new BlobReader(file), {
    strictness: 'strict',
    filenameValidation: 'strict',
    useWebWorkers: typeof Worker !== 'undefined',
  })
  try {
    const entries = await reader.getEntries({ strictness: 'strict', filenameValidation: 'strict' })
    if (entries.length > MAX_ENTRIES) throw new Error('ZIP 内文件数量异常，已停止打开')
    const files = new Map<string, FileEntry>()
    for (const entry of entries) {
      if (entry.directory) continue
      if (entry.encrypted || entry.symlink) throw new Error('不支持加密文件或符号链接')
      if (files.has(entry.filename)) throw new Error('ZIP 中存在重复文件名')
      files.set(entry.filename, entry)
    }

    const manifestEntry = requiredEntry(files, 'manifest.json', MAX_MANIFEST_BYTES)
    const manifest = normalizeManifest(JSON.parse(await manifestEntry.getData(new TextWriter())))
    if (manifest.formatVersion >= 2 && manifest.recordsFile && manifest.recordsFile !== 'records.json') {
      throw new Error('归档清单指向了不受支持的数据文件')
    }
    if (manifest.mediaRoot && manifest.mediaRoot !== 'media/') {
      throw new Error('归档清单包含不受支持的媒体目录')
    }
    const recordsEntry = manifest.formatVersion >= 2 && files.has(manifest.recordsFile || 'records.json')
      ? requiredEntry(files, manifest.recordsFile || 'records.json', MAX_RECORDS_BYTES)
      : requiredEntry(files, 'data.js', MAX_RECORDS_BYTES)
    const recordsText = await recordsEntry.getData(new TextWriter())
    const records = normalizeRecords(
      recordsEntry.filename === 'data.js' ? parseLegacyData(recordsText) : JSON.parse(recordsText),
    )
    if (records.length !== manifest.records) {
      throw new Error('归档清单与记录数量不一致')
    }

    return {
      kind: 'local',
      label: file.name,
      manifest,
      records,
      mediaSize: (path) => files.get(normalizeMediaPath(path))?.uncompressedSize ?? null,
      openMedia: (path) => openZipMedia(files, path),
      close: async () => reader.close(),
    }
  } catch (reason) {
    await reader.close().catch(() => undefined)
    throw reason instanceof Error ? reason : new Error('无法读取这个 ZIP 归档')
  }
}

async function openZipMedia(files: Map<string, FileEntry>, path: string): Promise<ArchiveMediaHandle> {
  const normalized = normalizeMediaPath(path)
  const entry = files.get(normalized)
  if (!entry) throw new Error('这个媒体文件不在归档包中')
  const blob = await entry.getData(new BlobWriter(mediaMime(normalized)))
  const url = URL.createObjectURL(blob)
  return {
    url,
    size: entry.uncompressedSize,
    release: () => URL.revokeObjectURL(url),
  }
}

async function requestJson<T>(path: string): Promise<T> {
  const response = await fetch(path, { credentials: 'same-origin', cache: 'no-store' })
  if (!response.ok) {
    const body = await response.json().catch(() => null) as { error?: { message?: string } } | null
    throw new Error(body?.error?.message || `打开回忆册失败（${response.status}）`)
  }
  return response.json() as Promise<T>
}

function requiredEntry(files: Map<string, FileEntry>, name: string, maximum: number): FileEntry {
  const entry = files.get(name)
  if (!entry) throw new Error(`归档包缺少 ${name}`)
  if (entry.uncompressedSize > maximum) throw new Error(`${name} 的大小异常，已停止打开`)
  return entry
}

function parseLegacyData(value: string): unknown {
  const trimmed = value.trim()
  if (!trimmed.startsWith(LEGACY_DATA_PREFIX) || !trimmed.endsWith(';')) {
    throw new Error('旧版归档的数据格式不受支持')
  }
  return JSON.parse(trimmed.slice(LEGACY_DATA_PREFIX.length, -1))
}

function normalizeManifest(value: unknown): ArchiveManifest {
  if (!isObject(value)) throw new Error('归档清单格式无效')
  const version = numberAt(value, 'formatVersion')
  if (version !== 1 && version !== 2) throw new Error(`暂不支持归档格式 v${version || '?'}`)
  return {
    formatVersion: version,
    generatedAt: numberAt(value, 'generatedAt'),
    complete: Boolean(value.complete),
    records: numberAt(value, 'records'),
    mediaDownloaded: numberAt(value, 'mediaDownloaded'),
    mediaFailed: numberAt(value, 'mediaFailed'),
    source: stringAt(value, 'source'),
    notice: stringAt(value, 'notice'),
    recordsFile: typeof value.recordsFile === 'string' ? value.recordsFile : undefined,
    mediaRoot: typeof value.mediaRoot === 'string' ? value.mediaRoot : undefined,
  }
}

function normalizeRecords(value: unknown): ArchiveRecord[] {
  if (!Array.isArray(value) || value.length > MAX_RECORDS) throw new Error('归档记录数量异常')
  return value.map((item, index) => {
    if (!isObject(item)) throw new Error(`第 ${index + 1} 条记录格式无效`)
    const media = Array.isArray(item.media)
      ? item.media.filter((path): path is string => typeof path === 'string').map(normalizeMediaPath)
      : []
    return {
      id: numberAt(item, 'id'),
      cellId: stringAt(item, 'cellId'),
      publishedAt: numberAt(item, 'publishedAt'),
      content: typeof item.content === 'string' ? item.content.slice(0, 200_000) : null,
      authorName: typeof item.authorName === 'string' ? item.authorName.slice(0, 300) : null,
      category: normalizeCategory(item.category),
      media,
    }
  })
}

function normalizeCategory(value: unknown): ArchiveCategory {
  return value === 'self' || value === 'guestbook' ? value : 'other'
}

function normalizeMediaPath(value: string): string {
  const normalized = value.replaceAll('\\', '/')
  if (
    normalized.length <= 'media/'.length ||
    !normalized.startsWith('media/') ||
    normalized.includes('/../') ||
    normalized.includes('//')
  ) {
    throw new Error('归档中包含无效的媒体路径')
  }
  return normalized
}

function encodeMediaPath(value: string): string {
  return normalizeMediaPath(value)
    .slice('media/'.length)
    .split('/')
    .map(encodeURIComponent)
    .join('/')
}

function mediaMime(path: string): string {
  const extension = path.split('.').pop()?.toLowerCase()
  const types: Record<string, string> = {
    jpg: 'image/jpeg', jpeg: 'image/jpeg', png: 'image/png', gif: 'image/gif',
    webp: 'image/webp', avif: 'image/avif', mp4: 'video/mp4', m4v: 'video/mp4',
    mov: 'video/quicktime', webm: 'video/webm',
  }
  return types[extension || ''] || 'application/octet-stream'
}

function isZipLike(file: File): boolean {
  return file.name.toLowerCase().endsWith('.zip') || ['application/zip', 'application/x-zip-compressed'].includes(file.type)
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function numberAt(value: Record<string, unknown>, key: string): number {
  const result = value[key]
  if (typeof result !== 'number' || !Number.isFinite(result) || result < 0) {
    throw new Error(`归档字段 ${key} 无效`)
  }
  return result
}

function stringAt(value: Record<string, unknown>, key: string): string {
  const result = value[key]
  if (typeof result !== 'string') throw new Error(`归档字段 ${key} 无效`)
  return result
}
