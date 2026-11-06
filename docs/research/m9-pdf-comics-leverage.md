# M9 PDF & comics — what to leverage (Book Orbit + Storyteller + Readium)

Goal: implement Shelf PDF then comic reading **without inventing a new reader stack**.
Wire upstream pieces we already vendor or that match mobile Readium.

## Shelf today (starting points)

| Piece | Status |
| --- | --- |
| `web/orbit-reader/…/ReaderView.vue` | Already switches `pdf` → `PdfV4ReaderView`, `cbz|cbr|cb7|djvu` → `CbzReaderView` |
| `PdfV4ReaderView.vue` / `CbzReaderView.vue` in tree | **Shelf stubs** (“not available yet”) — replace with upstream Book Orbit |
| `reader-settings.ts` | PDF + CBX defaults (`PDF_READER_DEFAULTS`, comic fit/spread/RTL) already vendored |
| `useReaderProgress` / `useReaderSettings` / `useReadingSession` | Shared with EPUB; reuse for PDF/comic progress % |
| Server import | EPUB-centric today — M9 needs PDF/CBZ(/CBR) ingest + authenticated file bytes |
| Mobile | Readium-only EPUB (`flutter_readium`); ink overlay wants **fixed page ids** (PDF/FXL) |

Decision 0004 already anticipated comic/PDF branches as stubs until formats exist.

---

## Book Orbit (primary web leverage) — **use these**

Upstream paths under `bookorbit/bookorbit` `client/src/features/reader/`:

### PDF (`pdf-v4/`) — **lift almost as-is**

| Highlight | Why |
| --- | --- |
| **EmbedPDF + Pdfium WASM** (`@embedpdf/*`, `usePdfiumEngine`, `pdfium.wasm`) | Full viewer: scroll/spread/zoom/search/thumbnails/outline/password |
| `PdfV4ReaderView.vue` + `components/*` + `composables/*` | Chrome already matches BO reader UX; tests under `__tests__/` |
| Annotation / selection / highlight plugins | Map onto Shelf bookmark/annotation APIs (same pattern as Foliate adapter) |
| Progress via page number + percentage | Fits Shelf `location` (`percentage` + opaque position string) |

**Shelf work:** restore upstream `pdf-v4` into `web/orbit-reader/vendor/…`, add npm deps, feed document bytes from Shelf `/api/v1/books/{id}/file` (or format-specific file route), persist location/settings through existing ShelfHost/API adapters. Do **not** rewrite a PDF viewer.

### Comics (`cbz/`) — **lift UI; add Shelf page API**

| Highlight | Why |
| --- | --- |
| `CbzReaderView.vue`, `useCbz`, `useCbzSettings`, `spread-layout.ts` | Paginated / infinite / long-strip, single/two-page, LTR/RTL, fit modes |
| `pageUrl(n)` → `/api/v1/cbz/files/{fileId}/pages/{n}` | BO serves **decoded page images** from the server — not a client-side unzip of CBR |
| DjVu as `source: 'djvu'` | Same image reader; optional later |

**Shelf work:** (1) import CBZ (ZIP of images) and optionally CBR; (2) server endpoints for page count + page image (or stream a synthetic CBZ Readium Divina later); (3) drop in BO `cbz/` UI. Prefer **CBZ first**; CBR needs RAR decode (heavier / license caution).

### Also from BO (cheap wins)

| Highlight | Why |
| --- | --- |
| Fixed-layout EPUB single-page / `fixedLayoutSpread` | Manga-as-EPUB without converting to CBZ (BO #487 / 2.1.0) — Foliate path already in host |
| `ReaderView` format dispatch + settings groups (`epub`/`pdf`/`cbx`) | Already in Shelf vendor; only stubs block open |

---

## Storyteller + Readium (primary **mobile** leverage) — **use these**

Storyteller’s mobile apps rebuilt on **Readium v3** for EPUB (and read-along). They are **not** a PDF/comic product, but their stack aligns with Shelf mobile:

| Highlight | Why for M9 |
| --- | --- |
| **Readium PDF adapters** (Kotlin `pdfium` / Swift PDF) | Same toolkit family as `flutter_readium`; enable PDF navigator instead of a second Flutter PDF package |
| **Readium CBZ / Divina** | Native comic archive open on mobile without BO’s server page raster API |
| Locator / progression sync patterns | Reuse Shelf location + % contract; page-index locators for PDF/CBZ |
| Media overlays / TTS | **Defer to M10** — do not block M9 on Storyteller audio |

**Shelf mobile work:** extend `EinkReaderScreen` (or sibling) to open PDF/CBZ via Readium after cache (same download-then-open as EPUB). Ink overlay page scoping becomes stable on PDF page index / FXL href — see `ink_stroke.dart` note on fixed-layout anchors.

---

## Recommended M9 slice order (fly path)

1. **Web PDF** — un-stub `pdf-v4`, deps, file bytes, location save/restore, settings.
2. **Server + web CBZ** — ingest CBZ; page-count + page-image API; un-stub `cbz/` UI.
3. **Mobile PDF** — Readium PDF adapter through Flutter; cache + open; ink page ids.
4. **Mobile CBZ** — Readium CBZ/Divina or shared cache of page images; RTL/fit prefs.
5. **CBR / DjVu** — done: convert to ZIP-of-images at ingest (`rars` + MIT `djvu-rs`); keep `cbr`/`djvu` format labels; reuse CBZ page API + Readium CBZ download.

## Do not reinvent

- A custom PDF.js wrapper when BO already ships EmbedPDF/Pdfium.
- A Foliate WebView on mobile for PDF (mobile is Readium-only).
- Full Storyteller alignment pipeline for M9 (that is M10+).
- Shipping CBR before CBZ.

## References

- Upstream BO: `https://github.com/bookorbit/bookorbit` (`client/src/features/reader/pdf-v4`, `…/cbz`)
- Readium PDF guide: `readium/kotlin-toolkit` `docs/guides/pdf.md` (Pdfium adapter)
- Readium Swift formats table: PDF ✅, CBZ ✅, CBR ❌
- Shelf stubs: `web/orbit-reader/vendor/bookorbit/features/reader/pdf-v4/PdfV4ReaderView.vue`, `…/cbz/CbzReaderView.vue`
- Ink fixed-layout note: `mobile/lib/ink_stroke.dart`
