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

function jsonResponse(body: Json, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function emptyResponse(status = 204): Response {
  return new Response(null, { status })
}

/** Queue a failed write for Flutter's sync outbox (ShelfHost). */
function shelfHostOutbox(method: string, path: string, body: unknown): void {
  const host = (globalThis as { ShelfHost?: { postMessage: (message: string) => void } })
    .ShelfHost
  if (!host?.postMessage) return
  const apiPath = path.startsWith('/api/v1/')
    ? path.slice('/api/v1/'.length)
    : path.replace(/^\//, '')
  host.postMessage(JSON.stringify({ type: 'outbox', method, path: apiPath, body }))
}

function isPersistPath(path: string): boolean {
  return (
    path.includes('/location') ||
    path.includes('/bookmarks') ||
    path.includes('/annotations') ||
    path.includes('/reader/preferences') ||
    path.includes('/reader/defaults') ||
    /\/books\/\d+\/progress$/.test(path)
  )
}

function parseBody(init: RequestInit | undefined): unknown {
  if (init?.body == null) return null
  try {
    return JSON.parse(String(init.body))
  } catch {
    return null
  }
}

function queuePersistFailure(
  method: string,
  path: string,
  init: RequestInit | undefined,
  status?: number,
): void {
  if (method === 'GET' || !isPersistPath(path)) return
  if (status != null && status < 500) return
  shelfHostOutbox(method, path, parseBody(init))
}

const LOCAL_MIGRATE_FLAG = 'shelf:orbit-reader:migrated-v1'

/** One-shot: push pre-M4 localStorage marks/prefs to Shelf, then clear those keys. */
export async function migrateLocalReaderData(): Promise<void> {
  if (typeof localStorage === 'undefined') return
  if (localStorage.getItem(LOCAL_MIGRATE_FLAG)) return
  const keys = Object.keys(localStorage)
  for (const key of keys) {
    const bookmark = key.match(/^shelf:orbit-reader:bookmarks:(\d+)$/)
    if (bookmark) {
      try {
        const items = JSON.parse(localStorage.getItem(key) || '[]') as Array<Record<string, unknown>>
        if (Array.isArray(items)) {
          for (const item of items) {
            await fetch(`/api/v1/books/${bookmark[1]}/bookmarks`, {
              method: 'POST',
              credentials: 'same-origin',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({
                cfi: item.cfi ?? null,
                title: item.title ?? item.chapterTitle ?? 'Bookmark',
                note: item.note ?? null,
              }),
            })
          }
        }
      } catch {
        // leave key for a later session
        continue
      }
      localStorage.removeItem(key)
      continue
    }
    const annotation = key.match(/^shelf:orbit-reader:annotations:(\d+)$/)
    if (annotation) {
      try {
        const items = JSON.parse(localStorage.getItem(key) || '[]') as Array<Record<string, unknown>>
        if (Array.isArray(items)) {
          for (const item of items) {
            await fetch(`/api/v1/books/${annotation[1]}/annotations`, {
              method: 'POST',
              credentials: 'same-origin',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({
                cfi: item.cfi ?? null,
                text: item.text ?? '',
                color: item.color ?? '#FACC15',
                style: item.style ?? 'highlight',
                note: item.note ?? null,
                chapterTitle: item.chapterTitle ?? null,
              }),
            })
          }
        }
      } catch {
        continue
      }
      localStorage.removeItem(key)
      continue
    }
    const prefs = key.match(/^shelf:orbit-reader:preferences:(\d+)$/)
    if (prefs) {
      try {
        const body = JSON.parse(localStorage.getItem(key) || '{}')
        if (body && typeof body === 'object') {
          await fetch(`/api/v1/reader/preferences/${prefs[1]}`, {
            method: 'PATCH',
            credentials: 'same-origin',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(body),
          })
        }
      } catch {
        continue
      }
      localStorage.removeItem(key)
      continue
    }
    if (key === 'shelf:orbit-reader:defaults:0') {
      try {
        const body = JSON.parse(localStorage.getItem(key) || '{}')
        if (body && typeof body === 'object') {
          await fetch('/api/v1/reader/defaults', {
            method: 'PATCH',
            credentials: 'same-origin',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(body),
          })
        }
      } catch {
        continue
      }
      localStorage.removeItem(key)
    }
  }
  localStorage.setItem(LOCAL_MIGRATE_FLAG, '1')
}

let _migrateStarted = false
function ensureLocalMigration(): void {
  if (_migrateStarted) return
  _migrateStarted = true
  void migrateLocalReaderData()
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
    format?: string
    percentage?: number | null
    position?: string | null
    has_cover?: boolean
  } | null
}

/** Resolve imported-file progress/cover from a Shelf book row (flat or nested). */
export function shelfEpubFields(book: ShelfBookRow): {
  format: 'epub' | 'pdf' | 'cbz' | 'cbr' | 'djvu'
  percentage: number
  position: string | null
  has_cover: boolean
} | null {
  const nested = book.epub
  const raw = book.format ?? nested?.format
  const format =
    raw === 'pdf' || raw === 'cbz' || raw === 'cbr' || raw === 'djvu' ? raw : 'epub'
  const isImported =
    book.format === 'epub' ||
    book.format === 'pdf' ||
    book.format === 'cbz' ||
    book.format === 'cbr' ||
    book.format === 'djvu' ||
    nested != null ||
    typeof book.percentage === 'number' ||
    typeof book.position === 'string' ||
    book.has_cover === true
  if (!isImported) return null
  return {
    format,
    percentage: nested?.percentage ?? book.percentage ?? 0,
    position: nested?.position ?? book.position ?? null,
    has_cover: nested?.has_cover ?? book.has_cover === true,
  }
}

function parseFixedPage(position: string | null): number | null {
  if (!position?.endsWith(')')) return null
  for (const prefix of ['pdfpage(', 'cbzpage('] as const) {
    if (!position.startsWith(prefix)) continue
    const n = Number.parseInt(position.slice(prefix.length, -1), 10)
    if (Number.isFinite(n) && n >= 1) return n
  }
  return null
}

/** Map Shelf catalogue rows into the Book Orbit BookDetail fields the reader reads. */
function shelfBookToDetail(book: ShelfBookRow & { title: string; author: string }): Json {
  const imported = shelfEpubFields(book)
  const format = imported?.format ?? 'epub'
  return {
    id: book.id,
    title: book.title,
    authors: book.author ? [{ name: book.author }] : [],
    files: [
      {
        id: book.id,
        format,
        size: 0,
      },
    ],
    coverVersion: imported?.has_cover ? 1 : null,
    coverSource: imported?.has_cover ? 'embedded' : null,
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
    const imported = book ? shelfEpubFields(book) : null
    if (!imported) return jsonResponse({ items: [] })
    // Book Orbit UI uses 0–100; Shelf stores 0–1.
    const shelfPct = imported.percentage ?? 0
    const fixedPage = parseFixedPage(imported.position)
    return jsonResponse({
      items: [
        {
          fileId,
          percentage: shelfPct <= 1 ? shelfPct * 100 : shelfPct,
          updatedAt: null,
          text: {
            cfi: fixedPage == null ? imported.position : null,
            page: fixedPage,
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
      writes?: Array<{
        fileId: number
        percentage?: number
        text?: { cfi?: string | null; page?: number | null }
      }>
    }) : {}
    const write = body.writes?.[0]
    if (!write?.fileId) return jsonResponse({ error: 'Missing write' }, 400)
    const cfi = write.text?.cfi
    const page = typeof write.text?.page === 'number' && write.text.page >= 1 ? write.text.page : null
    let pageToken = 'pdfpage'
    if (page != null) {
      const list = await fetch('/api/v1/books', { credentials: 'same-origin' })
      if (list.ok) {
        const books = (await list.json()) as ShelfBookRow[]
        const book = books.find((item) => item.id === write.fileId)
        const comic = shelfEpubFields(book ?? { id: 0 })?.format
        if (comic === 'cbz' || comic === 'cbr' || comic === 'djvu') pageToken = 'cbzpage'
      }
    }
    const position =
      typeof cfi === 'string' && cfi.startsWith('epubcfi(')
        ? cfi
        : page != null
          ? `${pageToken}(${Math.floor(page)})`
          : null
    const rawPct = typeof write.percentage === 'number' ? write.percentage : 0
    // Book Orbit posts 0–100; Shelf POST /location requires 0..=1.
    const percentage = Math.min(1, Math.max(0, rawPct > 1 ? rawPct / 100 : rawPct))
    if (!position) return emptyResponse()
    const locationBody = { position, percentage }
    try {
      const res = await fetch(`/api/v1/books/${write.fileId}/location`, {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(locationBody),
      })
      if (!res.ok && res.status >= 500) {
        shelfHostOutbox('POST', `/api/v1/books/${write.fileId}/location`, locationBody)
      }
      return res.ok ? emptyResponse() : res
    } catch (error) {
      if (error instanceof TypeError) {
        shelfHostOutbox('POST', `/api/v1/books/${write.fileId}/location`, locationBody)
        throw new NetworkError()
      }
      throw error
    }
  }

  // Bookmarks, annotations, and reader preferences/defaults hit Shelf SQLite APIs (milestone 4).

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
  ensureLocalMigration()
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

  const method = (init.method || 'GET').toUpperCase()
  try {
    const response = await fetch(url, {
      ...init,
      headers,
      credentials: 'same-origin',
    })
    if (response.status === 401) _onAuthFailure?.()
    if (!response.ok) queuePersistFailure(method, url.pathname, init, response.status)
    return response
  } catch {
    queuePersistFailure(method, url.pathname, init)
    throw new NetworkError()
  }
}
