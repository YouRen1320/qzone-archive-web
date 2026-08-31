import { Uint8ArrayReader, Uint8ArrayWriter, ZipWriter } from '@zip.js/zip.js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { openLocalArchive, openServerArchive } from '../services/archiveSource'

const record = {
  id: 1,
  cellId: 'cell-1',
  publishedAt: 1_700_000_000,
  content: '雨落在旧屋檐上',
  authorName: '自己',
  category: 'self',
  media: ['media/photo one.jpg'],
}

function manifest(overrides: Record<string, unknown> = {}) {
  return {
    formatVersion: 2,
    generatedAt: 1_700_000_100,
    complete: true,
    records: 1,
    mediaDownloaded: 1,
    mediaFailed: 0,
    source: 'QQ Zone mobile interaction feed',
    notice: 'Only content returned by QQ at archive time can be included.',
    recordsFile: 'records.json',
    mediaRoot: 'media/',
    ...overrides,
  }
}

async function archiveFile(entries: Record<string, string>, name = '拾光册.zip'): Promise<File> {
  if (!Blob.prototype.arrayBuffer) {
    Object.defineProperty(Blob.prototype, 'arrayBuffer', {
      configurable: true,
      value(this: Blob) {
        const source = this
        return new Promise<ArrayBuffer>((resolve, reject) => {
          const reader = new FileReader()
          reader.onload = () => resolve(reader.result as ArrayBuffer)
          reader.onerror = () => reject(reader.error)
          reader.readAsArrayBuffer(source)
        })
      },
    })
  }
  if (!Blob.prototype.stream) {
    Object.defineProperty(Blob.prototype, 'stream', {
      configurable: true,
      value(this: Blob) {
        const source = this
        return new ReadableStream<Uint8Array>({
          start(controller) {
            const reader = new FileReader()
            reader.onload = () => {
              controller.enqueue(new Uint8Array(reader.result as ArrayBuffer))
              controller.close()
            }
            reader.onerror = () => controller.error(reader.error)
            reader.readAsArrayBuffer(source)
          },
        })
      },
    })
  }
  const writer = new ZipWriter(new Uint8ArrayWriter(), { useWebWorkers: false })
  for (const [path, value] of Object.entries(entries)) {
    await writer.add(path, new Uint8ArrayReader(new TextEncoder().encode(value)), { useWebWorkers: false })
  }
  const bytes = await writer.close()
  return new File([bytes], name, { type: 'application/zip' })
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('local ZIP reader', () => {
  it('opens the structured v2 export without uploading or executing archive content', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)
    const file = await archiveFile({
      'manifest.json': JSON.stringify(manifest()),
      'records.json': JSON.stringify([record]),
      'media/photo one.jpg': 'image bytes',
    })

    const session = await openLocalArchive(file)

    expect(session.kind).toBe('local')
    expect(session.label).toBe('拾光册.zip')
    expect(session.records).toEqual([record])
    expect(session.mediaSize('media/photo one.jpg')).toBeGreaterThan(0)
    expect(fetchMock).not.toHaveBeenCalled()
    await session.close()
  })

  it('reads a v1 data.js export as JSON text instead of evaluating JavaScript', async () => {
    const legacyManifest = manifest({
      formatVersion: 1,
      recordsFile: undefined,
      mediaRoot: undefined,
    })
    const file = await archiveFile({
      'manifest.json': JSON.stringify(legacyManifest),
      'data.js': `window.__QZONE_ARCHIVE_DATA__=${JSON.stringify([record])};`,
    }, '旧版备份.zip')

    const session = await openLocalArchive(file)

    expect(session.records[0]?.content).toBe(record.content)
    await session.close()
  })

  it('rejects inconsistent manifests and media path traversal', async () => {
    const mismatched = await archiveFile({
      'manifest.json': JSON.stringify(manifest({ records: 2 })),
      'records.json': JSON.stringify([record]),
    })
    await expect(openLocalArchive(mismatched)).rejects.toThrow('记录数量不一致')

    const unsafe = await archiveFile({
      'manifest.json': JSON.stringify(manifest()),
      'records.json': JSON.stringify([{ ...record, media: ['media/../secret.txt'] }]),
    })
    await expect(openLocalArchive(unsafe)).rejects.toThrow('无效的媒体路径')
  })
})

describe('ready task reader', () => {
  it('uses private same-origin JSON endpoints and encodes media path segments', async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(new Response(JSON.stringify(manifest()), { status: 200 }))
      .mockResolvedValueOnce(new Response(JSON.stringify([record]), { status: 200 }))
    vi.stubGlobal('fetch', fetchMock)

    const session = await openServerArchive()
    const media = await session.openMedia('media/photo one.jpg')

    expect(fetchMock.mock.calls.map(([path]) => path)).toEqual([
      '/api/archive/viewer/manifest',
      '/api/archive/viewer/records',
    ])
    expect(media.url).toBe('/api/archive/viewer/media/photo%20one.jpg')
  })
})
