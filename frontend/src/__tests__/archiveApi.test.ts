import { afterEach, describe, expect, it, vi } from 'vitest'
import { archiveMediaUrl, fetchArchivePage } from '../services/archiveApi'

afterEach(() => vi.restoreAllMocks())

describe('archive reader API', () => {
  it('serializes pagination and optional filters without exposing local files', async () => {
    const fetchMock = vi.spyOn(window, 'fetch').mockResolvedValue(new Response(JSON.stringify({
      items: [], total: 0, offset: 30, nextOffset: null, years: [2025],
    }), { status: 200, headers: { 'content-type': 'application/json' } }))

    await fetchArchivePage({ offset: 30, limit: 30, search: '雨 声', category: 'other', year: 2025 })

    expect(fetchMock).toHaveBeenCalledOnce()
    const [url, options] = fetchMock.mock.calls[0]
    expect(String(url)).toContain('offset=30')
    expect(String(url)).toContain('q=%E9%9B%A8+%E5%A3%B0')
    expect(options).toMatchObject({ credentials: 'same-origin', cache: 'no-store' })
  })

  it('encodes safe task media paths and rejects traversal', () => {
    expect(archiveMediaUrl('media/photo 1.webp')).toBe('/api/archive/viewer/media/photo%201.webp')
    expect(() => archiveMediaUrl('media/../secret')).toThrow('无效')
  })
})
