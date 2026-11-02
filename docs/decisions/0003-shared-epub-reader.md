# 0003. Shared EPUB reader and transactional imports

- **Status:** Accepted
- **Date:** 2026-10-30

## Context

Shelf needs to import EPUBs and reopen their saved reading positions on web, Android, and iPhone. The approved design and one-container setup remain in place. EPUBs contain untrusted HTML, CSS, images, and archives; uploaded books must never become executable application assets.

## Options considered

1. Separate web and native readers. Each could use platform controls, but location formats and rendering would need separate integration work.
2. A shared epub.js reader served by Rust and embedded with Flutter's WebView plugin. This reuses navigation and EPUB CFI locations across clients, while requiring careful content isolation.

## Decision

Use the Rust `epub` crate for metadata and covers, with ZIP preflight bounds. Store imported bytes, optional raster cover, digest, CFI, and percentage in an additive SQLite table. This keeps imports atomic and uses the existing persisted data volume. As originally implemented, uploads were limited to 50 MiB, decompressed resources to 16 MiB, total declared decompressed size to 128 MiB, and archives to 4,096 entries. The approved follow-up below supersedes the fixed byte limits. Reject encrypted resources, symlinks, and unsafe archive paths; never extract archive paths to disk.

Deduplicate exact bytes within an account. Manual entries keep page progress. EPUB entries expose a null page count and a separate percentage and CFI; no estimated page count contributes to page totals.

**Rendering (updated by [0004](0004-bookorbit-foliate-reader.md)):** the shared web/WebView reader is Book Orbit's Foliate/Vue EPUB reader under `web/orbit-reader/`, not the original epub.js page. Import security, CFI storage, cookie exchange for Flutter, and CSP for the reader shell remain required. Foliate loads chapter documents in blob iframes; reader CSP continues to block external connect/frame targets outside `blob:` and `'self'`.

## Consequences

SQLite backups include imported files and reading positions. Large libraries will grow the database; revisit blob storage when library size and backup performance justify a separate managed content store. Opening a book downloads it into reader memory and generates locations, so offline reading and large-book optimization remain separate work. Concurrent readers use the last position successfully saved to the server. Synchronization is online and has no offline conflict resolution.

Older mobile clients that require numeric page counts must be updated before using an EPUB library. Encrypted/obfuscated resources, advanced scripted interactivity, and other file formats remain unsupported.

## Follow-up: unified Add book action and larger EPUBs

On 2026-10-30 the user requested a single **Add book** action that imports EPUBs, and removal of the 50 MiB limit. Both web and mobile now use this action; the manual-entry UI is removed while existing metadata entries and the manual API remain compatible. Remove the fixed upload and decompressed-byte ceilings so they do not impose a different effective size limit. Retain the 4,096-entry check, path/symlink checks, and a per-resource expansion check of compressed size times 200 plus 1 MiB to reject extreme compression. Imports remain authenticated and limited to two concurrent requests. Practical limits are available memory, SQLite's blob capacity, and deployment proxy limits; disk-backed streaming remains a future storage change.
