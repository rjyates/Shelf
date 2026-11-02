# Shelf orbit reader

This package hosts [Book Orbit](https://github.com/bookorbit/bookorbit)'s EPUB
reader (Foliate + Vue) inside Shelf. Book Orbit is AGPL-3.0-only; see
`vendor/bookorbit/ATTRIBUTION.md`.

Shelf adapters live in `vendor/bookorbit/lib/api.ts` and a few stubs. They map
progress and file open onto Shelf's cookie session and
`/api/v1/books/{id}/file` / `location`. Bookmarks, annotations, and preferences
use `localStorage` until Shelf gains matching APIs.

Do not restyle this UI to Shelf's journal theme while we are still aligning
behavior with Book Orbit.

```bash
npm ci
npm run build
```

The Rust server embeds `dist/` via `rust-embed` and serves it at `/reader` and
`/orbit-reader/*`.
