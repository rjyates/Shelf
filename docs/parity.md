# Book Orbit feature roadmap

Reference: [Book Orbit](https://github.com/bookorbit/bookorbit), reviewed 2026-11-02. This is a parity target, not a claim that Shelf already implements every Book Orbit product surface.

Shelf's shared EPUB reader is Book Orbit's Foliate/Vue reader hosted under `web/orbit-reader/` (AGPL-3.0-only, attributed in tree). Shelf does not restyle that reader to the journal theme while the Foliate host is the primary surface. Storyteller remains a reference for later listening/sync behaviors, not the default reader base.

## Mobile product shape (decided)

- **One Flutter app** builds for Android and iPhone. No separate e-ink or stylus codebases.
- **Display profile:** `eink` (default) or `lcd`. First-run onboarding asks whether the device is an e-ink display; choosing no switches to the normal LCD experience (and can be changed later in settings).
- **E-ink default on** means e-ink-friendly defaults (contrast, chrome density, refresh-oriented behavior, and later the native navigator/ink path when present). LCD mode keeps the Foliate WebView reader experience aligned with the web app.
- **Platform reality:** almost all e-ink tablets are Android. iPhone stays on the Foliate WebView path; Android may add a native reader/ink module *inside the same app* behind the e-ink profile, via Flutter platform channels—not a second app.

## Capability status

| Capability | Shelf status |
| --- | --- |
| First-account setup, login, session revocation | Implemented in foundation |
| Manual catalogue entries and persisted page progress | Implemented on web and shared mobile |
| Overview, library search, real catalogue/progress totals | Implemented on web; catalogue/totals on mobile |
| Android and iPhone shared client | Foundation with login, catalogue, add book, record progress, WebView reader entry |
| Single-container Compose packaging | Implemented; published image begins after a successful main release |
| Independent semantic server/mobile releases | Workflow implemented; publication verified only after main release |
| EPUB upload/import and authenticated downloads | Implemented; exact-byte deduplication and embedded raster covers; single **Add book** action |
| EPUB reading (Book Orbit Foliate: chapters, settings, search, shortcuts, CFI/%) | Web host usable; bookmarks/annotations/prefs still localStorage until Shelf APIs exist |
| Mobile online Foliate WebView + progress sync | Next milestone |
| Display profile onboarding (`eink` default / `lcd`) | Planned; lands early so later reader work can branch on one preference |
| Offline book download + progress/annotation outbox | Planned; required before serious e-ink use |
| Native Android EPUB navigator (e-ink profile) | Planned; same app, platform module; maps Locator ↔ Shelf CFI/% |
| Stylus / freehand ink on pages (native overlay) | Planned after offline + annotation persistence; vendor adapters later |
| Folder scanning and multiple libraries | Planned |
| KEPUB, MOBI, AZW, FB2 reading | Planned (Foliate and/or native navigator once EPUB path is solid) |
| PDF reading | Planned (strong fit for ink; after annotation/offline foundations) |
| Comic archive reading | Planned |
| Audiobook playback and background media controls | Planned |
| EPUB Media Overlay read-along and alignment | Planned; BO media-overlay UI is present, Shelf audio serving is not |
| Annotations, highlights, searchable/exportable notes | UI in BO reader; server persistence Planned |
| Bookmarks | UI in BO reader; server persistence Planned |
| Reader preference sync across devices | Local in browser for now; server sync Planned |
| Kobo and KOReader integrations | Planned |
| OPDS and Send-to-Kindle | Planned |
| Metadata providers and cover enrichment | Planned |
| Authors, series, smart scopes, collections | Planned |
| Session-based statistics, heatmaps, streaks, Reading DNA | Planned; overview already exposes average progress across started books |
| Annual goals, monthly challenges, achievements | Planned |
| Hardcover, Readwise, StoryGraph | Planned; external accounts remain unconfigured |
| Multiple users, roles, OIDC/SSO | Planned; initial foundation is one account |
| Automated ingestion and book requests/download clients | Planned |
| Reading journal | Planned; approved design includes a journal panel |
| Localization | Planned; vendored reader ships English from Book Orbit |
| Storyteller-style synced narration, PiP, sleep timer | Planned after audio; not the reader base |
| Signed Android/iOS distribution and stores | Deferred by user; artifacts first |

## Implementation milestones (ordered for mutual benefit)

1. **Stabilize the Book Orbit web reader host** — mostly done: file open, CFI restore/save, Foliate assets, navigation, CI build of `web/orbit-reader`, AGPL attribution. No Shelf journal restyle of that chrome.

2. **Mobile online reader (shared app, both platforms)** — harden Flutter WebView → `/reader`: auth cookie/bearer, EPUB file open, two-way CFI/% via `/location`, catalogue refresh after reading. This is the LCD path forever and the e-ink path until a native module exists.

3. **Display profile onboarding** — persist `eink` (default) vs `lcd` from a short first-run question (“Are you using an e-ink display?”). Wire settings to retoggle. Apply immediate UX defaults (theme density, animation/reduced motion, contrast) without yet swapping engines. Later milestones read this flag.

4. **Persist reader data on Shelf** — bookmarks, highlights/notes, and per-book reader preferences in SQLite (replace localStorage). Keep CFI/% as the position contract. Unblocks web ↔ mobile sync and gives ink/highlights a server home before native drawing.

5. **Offline downloads + sync outbox** — download EPUB bytes to device storage; queue location and annotation writes while offline; reconcile on reconnect. E-ink users need this; LCD travelers benefit too. Do this before investing in native ink so strokes and progress have somewhere to live.

6. **Native Android navigator behind e-ink profile** — same Flutter app; Android platform module (Readium Kotlin or equivalent) for EPUB when `displayProfile == eink`. Map Locator ↔ Shelf CFI/%. iPhone and LCD Android keep Foliate WebView. Goal: e-ink reading quality without a second product.

7. **Stylus ink overlay + stroke sync** — native transparent ink layer over the e-ink navigator (not Foliate canvas). Store strokes with page/spine anchors; sync through Shelf annotation APIs from milestone 4. Prefer PDF/fixed-layout for stable ink; treat reflowable EPUB ink as best-effort. Vendor acceleration (Onyx, etc.) as adapters under one ink API—not per-device apps.

8. **PDF and comic readers** — lift Book Orbit format views and/or native navigators once offline + annotations exist; PDF especially reinforces the ink/study workflow.

9. **TTS and media overlays** — Foliate/BO hooks first on the WebView path; native/e-ink listen features after reading stacks are stable. Then Storyteller-like extras (sleep timer, sync offset, PiP) where they fit.

10. **Library scale and organization** — folder scanning, editable metadata, authors/series/scopes/collections, storage/performance.

11. **Rich stats and external integrations** — session heatmaps, streaks, goals; Kobo/KOReader/OPDS; Hardcover/Readwise/StoryGraph; multi-user/OIDC as needed.

Every milestone needs a branch/PR, meaningful tests, updated API contracts, and an accurate entry in the table above. Do not implement document parsers from scratch or provision paid services to fill a gap. Prefer one Flutter app with feature flags/profiles over forked products. Prefer Book Orbit's AGPL Foliate reader (with attribution) for the shared LCD/web experience; add native modules only where WebView cannot meet e-ink or stylus needs.

## EPUB / Foliate host handoff, 2026-11-02

Shelf vendors Book Orbit's EPUB reader and Foliate under `web/orbit-reader/` and serves the Vite build from the Rust binary. Opening `/reader?id=` loads that SPA. Progress uses Shelf's location API. Bookmarks, annotations, fonts, TTS backends, and preference sync are stubbed locally where Shelf has no API yet.

The previous epub.js page (`web/reader.js`) is no longer routed. Import bounds and digest rules from decision 0003 remain. Decision 0004 records the Book Orbit base. Web and mobile use one **Add book** EPUB import action (manual-entry UI and fixed byte ceilings removed earlier).
