# M11 — Library scale, organization, and metadata enrichment

Ordered fly path for Shelf milestone 11 (`docs/parity.md`). **Ship as one PR** (server + clients). Former **M17** (providers + multi-cover) is absorbed here.

## Slice order

1. Editable metadata — `PATCH /api/v1/books/{id}` title/author; Manage UI. **Shipped**
2. Series fields — `series` / `series_index` + dual-write to series entities. **Shipped**
3. Authors browse — `GET /api/v1/authors` (+ first-class `authors` / `book_authors`). **Shipped**
4. Collections — `/api/v1/collections`. **Shipped**
5. Smart scopes — `/api/v1/scopes`. **Shipped**
6. Folder scanning — roots + scan under `SHELF_LIBRARY_ROOTS`. **Shipped**
7. Catalogue pagination/filters. **Shipped**
8. Android self-update. **Shipped**
9. Persistent mobile login. **Shipped**
10. E-ink disables animations. **Shipped**
11. Bibliographic metadata model — `book_metadata`, entities, covers, locks, score, provider IDs. **Shipped**
12. Providers + field-rules pipeline — BO provider parity: OL/Google/iTunes/AudNexus/Goodreads/Amazon (+ Audible/Hardcover/ComicVine/Kobo/Libro.fm/RanobeDB); cover search iTunes/OL/DDG/AudiobookCovers; external HTTP adapter. **Shipped**
13. Auto-fetch + sync — durable queue, import enqueue, run/pause/resume/cancel, per-book refresh. **Shipped**
14. Enrichment UI — web Settings + Manage search/apply/covers; mobile Manage autofill/search/refresh + library sync. **Shipped**

## Out of M11

- Write metadata into files / rename → later
- Hardcover / Readwise / StoryGraph account sync → **M12**
- Multi-user ACL scopes → **M16**
- Embeddings / Book Dock → later

## Acceptance

- Auth PATCH updates title/author/series and rich metadata fields; locks respected on apply
- Providers search/apply; auto-fetch queue processes eligible books; Sync library enqueues run
- Multi-cover gallery with activate; legacy `epub_files.cover` stays in sync for ebook active cover
