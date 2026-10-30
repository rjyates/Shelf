# 0003. Shared EPUB reader and transactional imports

- **Status:** Accepted
- **Date:** 2026-10-30

## Context

Shelf needs to import EPUBs and reopen their saved reading positions on web, Android, and iPhone. The approved design and one-container setup remain in place. EPUBs contain untrusted HTML, CSS, images, and archives; uploaded books must never become executable application assets.

## Options considered

1. Separate web and native readers. Each could use platform controls, but location formats and rendering would need separate integration work.
2. A shared epub.js reader served by Rust and embedded with Flutter's WebView plugin. This reuses navigation and EPUB CFI locations across clients, while requiring careful content isolation.

## Decision

Use the Rust `epub` crate for metadata and covers, with ZIP preflight bounds, and self-host epub.js, JSZip, and DOMPurify. Store imported bytes, optional raster cover, digest, CFI, and percentage in an additive SQLite table. This keeps imports atomic and uses the existing persisted data volume. Limit uploads to 50 MiB, each decompressed resource to 16 MiB, total declared decompressed size to 128 MiB, and archives to 4,096 entries. Reject encrypted resources, symlinks, and unsafe archive paths; never extract archive paths to disk.

Deduplicate exact bytes within an account. Manual entries keep page progress. EPUB entries expose a null page count and a separate percentage and CFI; no estimated page count contributes to page totals.

Sanitize serialized chapters before rendering them in script-disabled sandboxed iframes. Reader CSP blocks external resources, forms, and plugins. Book scripts and popups remain disabled. Vendored dependencies include licenses and verified provenance. Flutter uses a system document picker and the shared reader. Its initial authenticated reader request exchanges the existing bearer session for an HttpOnly cookie, avoiding tokens in URLs and JavaScript bridges. Native navigation stays within Shelf and the reader's sandbox frames.

## Consequences

SQLite backups include imported files and reading positions. Large libraries will grow the database; revisit blob storage when library size and backup performance justify a separate managed content store. Opening a book downloads it into reader memory and generates locations, so offline reading and large-book optimization remain separate work. Concurrent readers use the last position successfully saved to the server. Synchronization is online and has no offline conflict resolution.

Older mobile clients that require numeric page counts must be updated before using an EPUB library. Encrypted/obfuscated resources, advanced scripted interactivity, and other file formats remain unsupported.
