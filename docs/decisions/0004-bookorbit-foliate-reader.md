# 0004. Book Orbit Foliate reader as Shelf's shared EPUB reader

- **Status:** Accepted
- **Date:** 2026-11-02
- **Supersedes in part:** [0003](0003-shared-epub-reader.md) (rendering stack only; import and CFI storage remain)

## Context

Shelf's first shared reader was a minimal epub.js page. Book Orbit and Storyteller both ship much richer web readers. After comparing them, Foliate as used by Book Orbit is the closer base: CFI positions, iframe chapter rendering, and a feature surface that already includes settings, search, bookmarks, annotations UI, shortcuts, and TTS hooks. Storyteller's web reader is Readium-based, experimental, and tightly bound to aligned audio.

Shelf is AGPL-3.0-only; Book Orbit is AGPL-3.0-only. Taking their reader with in-code attribution is license-compatible.

## Decision

Host Book Orbit's EPUB reader (Vue UI + vendored Foliate) under `web/orbit-reader/`, built with Vite and embedded into the Rust server from `web/orbit-reader/dist`. Keep Book Orbit's reader chrome and theming as shipped; do not restyle it to Shelf's journal palette in this phase.

Shelf adapters rewrite Book Orbit HTTP calls onto Shelf's cookie session, `/api/v1/books/{id}/file`, and `/api/v1/books/{id}/location`. Bookmarks, annotations, and reader preferences that Shelf cannot store yet stay in `localStorage` so the full chrome can mount. Comic, PDF, and audiobook branches in Book Orbit's `ReaderView` remain stubs until those formats exist on Shelf.

Flutter continues to open `/reader?id=` in a WebView; the SPA redirects to `/reader/{id}/{id}?format=epub` and may load `/orbit-reader/*` assets.

## Consequences

Opening an EPUB downloads the whole file into Foliate (no Book Orbit streaming EPUB API yet). Server-backed bookmarks, annotations, synced preferences, fonts, TTS providers, and media overlays need later Shelf APIs. Decision 0003's import bounds, digest dedupe, and CFI/percentage columns remain. The old `web/reader.js` epub.js page is no longer served.
