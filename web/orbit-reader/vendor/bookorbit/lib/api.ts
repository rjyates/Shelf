/**
 * Shelf HTTP adapter for the Book Orbit reader.
 * Replaces Book Orbit's JWT api() client (AGPL-3.0-only upstream):
 * https://github.com/bookorbit/bookorbit
 *
 * Shelf authenticates the reader with an HttpOnly cookie (and a one-shot bearer
 * exchange on the initial WebView load). Unsupported Book Orbit endpoints are
 * answered from localStorage so the full reader chrome can mount.
 */

export class NetworkError extends Error {
  constructor(message = 'Network error') {
    super(message)
    this.name = 'NetworkError'
  }
}

let _accessToken: string | null = null
let _onAuthFailure: (() => void) | null = null

export function setAccessToken(token: string | null): void {
  _accessToken = token
}

export function getAccessToken(): string | null {
  return _accessToken
}

export async function getValidToken(): Promise<string | null> {
  return _accessToken
}

export function setOnAuthFailure(fn: (() => void) | null): void {
  _onAuthFailure = fn
}

export function isServerUnavailableStatus(status: number): boolean {
  return status === 502 || status === 503 || status === 504
}

export async function refreshAccessToken(): Promise<string> {
  throw new Error('Shelf sessions do not refresh JWT access tokens.')
}

export async function fetchWithAuthProxyRecovery(
  input: RequestInfo | URL,
  init?: RequestInit,
): Promise<Response> {
  return api(input, init)
}

type Json = null | boolean | number | string | Json[] | { [key: string]: Json }

function storageKey(kind: string, bookId: number): string {
  return `shelf:orbit-reader:${kind}:${bookId}`
}

function readJson<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key)
    return raw ? (JSON.parse(raw) as T) : fallback
  } catch {
    return fallback
  }
}

function writeJson(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch {
    // quota
  }
}

function jsonResponse(body: Json, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function emptyResponse(status = 204): Response {
  return new Response(null, { status })
}

/** One catalogue row from Shelf. EPUB fields are flattened (`#[serde(flatten)]`), not nested. */
type ShelfBookRow = {
  id: number
  title?: string
  author?: string
  format?: string
  percentage?: number | null
  position?: string | null
  has_cover?: boolean
  epub?: {
    percentage?: number | null
    position?: string | null
    has_cover?: boolean
  } | null
}

/** Resolve EPUB progress/cover from a Shelf book row (flat or nested). */
export function shelfEpubFields(book: ShelfBookRow): {
  percentage: number
  position: string | null
  has_cover: boolean
} | null {
  const nested = book.epub
  const isEpub =
    book.format === 'epub' ||
    nested != null ||
    typeof book.percentage === 'number' ||
    typeof book.position === 'string' ||
    book.has_cover === true
  if (!isEpub) return null
  return {
    percentage: nested?.percentage ?? book.percentage ?? 0,
    position: nested?.position ?? book.position ?? null,
    has_cover: nested?.has_cover ?? book.has_cover === true,
  }
}

/** Map Shelf catalogue rows into the Book Orbit BookDetail fields the reader reads. */
function shelfBookToDetail(book: ShelfBookRow & { title: string; author: string }): Json {
  const epub = shelfEpubFields(book)
  return {
    id: book.id,
    title: book.title,
    authors: book.author ? [{ name: book.author }] : [],
    files: [
      {
        id: book.id,
        format: 'epub',
        size: 0,
      },
    ],
    coverVersion: epub?.has_cover ? 1 : null,
    coverSource: epub?.has_cover ? 'embedded' : null,
  }
}

async function handleLocal(url: URL, init: RequestInit | undefined): Promise<Response | null> {
  const path = url.pathname
  const method = (init?.method || 'GET').toUpperCase()

  // Single-book detail: Shelf only lists books.
  const bookMatch = path.match(/^\/api\/v1\/books\/(\d+)$/)
  if (bookMatch && method === 'GET') {
    const id = Number(bookMatch[1])
    const list = await fetch('/api/v1/books', { credentials: 'same-origin' })
    if (!list.ok) return list
    const books = (await list.json()) as Array<ShelfBookRow & { title: string; author: string }>
    const book = books.find((item) => item.id === id)
    if (!book) return jsonResponse({ error: 'Not found' }, 404)
    return jsonResponse(shelfBookToDetail(book))
  }

  // Cover thumbnail alias.
  const thumbMatch = path.match(/^\/api\/v1\/books\/(\d+)\/thumbnail$/)
  if (thumbMatch && method === 'GET') {
    return fetch(`/api/v1/books/${thumbMatch[1]}/cover`, {
      credentials: 'same-origin',
      headers: init?.headers,
    })
  }

  // Whole-file serve used by Foliate's non-streaming open path.
  const serveMatch = path.match(/^\/api\/v1\/books\/files\/(\d+)\/serve$/)
  if (serveMatch && method === 'GET') {
    return fetch(`/api/v1/books/${serveMatch[1]}/file`, {
      credentials: 'same-origin',
      headers: init?.headers,
    })
  }

  // Reading progress list/get.
  if (path === '/api/v1/reading-progress' && method === 'GET') {
    const fileId = Number(url.searchParams.get('fileId') || url.searchParams.get('bookId') || 0)
    if (!fileId) return jsonResponse({ items: [] })
    const list = await fetch('/api/v1/books', { credentials: 'same-origin' })
    if (!list.ok) return list
    const books = (await list.json()) as ShelfBookRow[]
    const book = books.find((item) => item.id === fileId)
    const epub = book ? shelfEpubFields(book) : null
    if (!epub) return jsonResponse({ items: [] })
    // Book Orbit UI uses 0–100; Shelf stores 0–1.
    const shelfPct = epub.percentage ?? 0
    return jsonResponse({
      items: [
        {
          fileId,
          percentage: shelfPct <= 1 ? shelfPct * 100 : shelfPct,
          updatedAt: null,
          text: {
            cfi: epub.position ?? null,
            page: null,
            updatedAt: null,
          },
          narration: null,
          kobo: null,
          koreader: null,
        },
      ],
    })
  }

  if (path === '/api/v1/reading-progress' && method === 'PUT') {
    const body = init?.body ? (JSON.parse(String(init.body)) as {
      writes?: Array<{ fileId: number; percentage?: number; text?: { cfi?: string | null } }>
    }) : {}
    const write = body.writes?.[0]
    if (!write?.fileId) return jsonResponse({ error: 'Missing write' }, 400)
    const position = write.text?.cfi
    const rawPct = typeof write.percentage === 'number' ? write.percentage : 0
    // Book Orbit posts 0–100; Shelf POST /location requires 0..=1.
    const percentage = Math.min(1, Math.max(0, rawPct > 1 ? rawPct / 100 : rawPct))
    if (!position || !position.startsWith('epubcfi(')) return emptyResponse()
    const res = await fetch(`/api/v1/books/${write.fileId}/location`, {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ position, percentage }),
    })
    return res.ok ? emptyResponse() : res
  }

  // Reader preferences / defaults — local only for now.
  const prefMatch = path.match(/^\/api\/v1\/reader\/preferences\/(\d+)$/)
  if (prefMatch) {
    const key = storageKey('preferences', Number(prefMatch[1]))
    if (method === 'GET') return jsonResponse(readJson(key, null))
    if (method === 'PATCH' || method === 'PUT') {
      const patch = init?.body ? JSON.parse(String(init.body)) : {}
      const current = readJson<Record<string, unknown>>(key, {})
      writeJson(key, { ...current, ...patch })
      return jsonResponse(readJson(key, {}))
    }
    if (method === 'DELETE') {
      localStorage.removeItem(key)
      return emptyResponse()
    }
  }

  if (path === '/api/v1/reader/defaults' || path.startsWith('/api/v1/reader/defaults/')) {
    const key = storageKey('defaults', 0)
    if (method === 'GET') return jsonResponse(readJson(key, {}))
    if (method === 'PATCH' || method === 'PUT') {
      const patch = init?.body ? JSON.parse(String(init.body)) : {}
      const current = readJson<Record<string, unknown>>(key, {})
      writeJson(key, { ...current, ...patch })
      return jsonResponse(readJson(key, {}))
    }
    if (method === 'DELETE') {
      localStorage.removeItem(key)
      return emptyResponse()
    }
  }

  // Bookmarks — Book Orbit's useBookmarks assigns `await res.json()` to an array ref.
  const bookmarksMatch = path.match(/^\/api\/v1\/books\/(\d+)\/bookmarks(?:\/(\d+))?$/)
  if (bookmarksMatch) {
    const bookId = Number(bookmarksMatch[1])
    const bookmarkId = bookmarksMatch[2] ? Number(bookmarksMatch[2]) : null
    const key = storageKey('bookmarks', bookId)
    type Bookmark = {
      id: number
      bookId: number
      cfi: string | null
      title: string
      note?: string | null
      createdAt: string
    }
    const items = readJson<Bookmark[]>(key, [])
    if (method === 'GET') return jsonResponse(items)
    if (method === 'POST') {
      const body = init?.body ? JSON.parse(String(init.body)) : {}
      const next: Bookmark = {
        id: Date.now(),
        bookId,
        cfi: body.cfi ?? null,
        title: body.title ?? body.chapterTitle ?? 'Bookmark',
        note: body.note ?? null,
        createdAt: new Date().toISOString(),
      }
      writeJson(key, [...items, next])
      return jsonResponse(next, 201)
    }
    if (bookmarkId && method === 'DELETE') {
      writeJson(
        key,
        items.filter((item) => item.id !== bookmarkId),
      )
      return emptyResponse()
    }
    if (bookmarkId && (method === 'PATCH' || method === 'PUT')) {
      const body = init?.body ? JSON.parse(String(init.body)) : {}
      const updated = items.map((item) =>
        item.id === bookmarkId ? { ...item, ...body, id: bookmarkId, bookId } : item,
      )
      writeJson(key, updated)
      return jsonResponse(updated.find((item) => item.id === bookmarkId) ?? body)
    }
  }

  // Annotations — paginated `{ items, total }` when page= is present; bare array otherwise.
  const annotationsMatch = path.match(/^\/api\/v1\/books\/(\d+)\/annotations(?:\/(\d+))?$/)
  if (annotationsMatch) {
    const bookId = Number(annotationsMatch[1])
    const annotationId = annotationsMatch[2] ? Number(annotationsMatch[2]) : null
    const key = storageKey('annotations', bookId)
    type Annotation = Record<string, Json> & { id: number }
    const items = readJson<Annotation[]>(key, [])
    if (method === 'GET') {
      if (url.searchParams.has('page') || url.searchParams.has('readerFileId')) {
        return jsonResponse({ items, page: 1, pageSize: Math.max(items.length, 1), total: items.length })
      }
      return jsonResponse(items)
    }
    if (method === 'POST') {
      const body = (init?.body ? JSON.parse(String(init.body)) : {}) as Record<string, Json>
      const next = { ...body, id: Date.now() } as Annotation
      writeJson(key, [...items, next])
      return jsonResponse(next, 201)
    }
    if (annotationId && method === 'DELETE') {
      writeJson(
        key,
        items.filter((item) => item.id !== annotationId),
      )
      return emptyResponse()
    }
    if (annotationId && (method === 'PATCH' || method === 'PUT')) {
      const body = (init?.body ? JSON.parse(String(init.body)) : {}) as Record<string, Json>
      const updated = items.map((item) =>
        item.id === annotationId ? ({ ...item, ...body, id: annotationId } as Annotation) : item,
      )
      writeJson(key, updated)
      return jsonResponse(updated.find((item) => item.id === annotationId) ?? body)
    }
  }

  // Font lists must be arrays — Book Orbit does `await res.json()` then `.map(...)`.
  if (path === '/api/v1/fonts' || path === '/api/v1/server-fonts') {
    if (method === 'GET') return jsonResponse([])
    return emptyResponse()
  }
  if (path === '/api/v1/user-preferences/server-fonts') {
    if (method === 'GET') return jsonResponse({ settings: { hiddenFamilies: [] } })
    return emptyResponse()
  }

  // TTS, sessions, media-overlay playlists, EPUB info/stream — not on Shelf yet.
  if (
    path.startsWith('/api/v1/fonts') ||
    path.startsWith('/api/v1/server-fonts') ||
    path.startsWith('/api/v1/tts') ||
    path.includes('/sessions') ||
    path.includes('/media-overlay') ||
    path.startsWith('/api/v1/epub/')
  ) {
    if (method === 'GET') {
      return jsonResponse(path.includes('voices') || path.includes('providers') ? [] : {})
    }
    return emptyResponse()
  }

  return null
}

export async function api(input: RequestInfo | URL, init: RequestInit = {}): Promise<Response> {
  const url =
    typeof input === 'string'
      ? new URL(input, window.location.origin)
      : input instanceof URL
        ? input
        : new URL(input.url, window.location.origin)

  try {
    const local = await handleLocal(url, init)
    if (local) return local
  } catch (error) {
    if (error instanceof TypeError) throw new NetworkError()
    throw error
  }

  const headers = new Headers(init.headers || {})
  if (_accessToken && !headers.has('Authorization')) {
    headers.set('Authorization', `Bearer ${_accessToken}`)
  }

  try {
    const response = await fetch(url, {
      ...init,
      headers,
      credentials: 'same-origin',
    })
    if (response.status === 401) _onAuthFailure?.()
    return response
  } catch {
    throw new NetworkError()
  }
}
