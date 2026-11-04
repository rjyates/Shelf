# Book Orbit feature roadmap

Reference: [Book Orbit](https://github.com/bookorbit/bookorbit), reviewed 2026-11-02. This is a parity target, not a claim that Shelf already implements every Book Orbit product surface.

Shelf's shared EPUB reader is Book Orbit's Foliate/Vue reader hosted under `web/orbit-reader/` (AGPL-3.0-only, attributed in tree). Shelf does not restyle that reader to the journal theme while the Foliate host is the primary surface. Storyteller remains a reference for later listening/sync behaviors, not the default reader base.

## Mobile product shape (decided)

- **One Flutter app** builds for Android and iPhone. No separate e-ink or stylus codebases.
- **Display profile:** `eink` (default) or `lcd`. First-run onboarding asks whether the device is an e-ink display; choosing no switches to the normal LCD experience (and can be changed later in settings).
- **E-ink default on** means e-ink-friendly defaults (contrast, chrome density, refresh-oriented behavior) and the Readium navigator when a cached EPUB is available. LCD mode keeps the Foliate WebView reader experience aligned with the web app.
- **Platform reality:** almost all e-ink tablets are Android. Online LCD/iPhone stay on Foliate WebView; Android e-ink (and any offline cached open) uses `flutter_readium` inside the same app—not a second app. Stylus ink remains the next native overlay.

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
| EPUB reading (Book Orbit Foliate: chapters, settings, search, shortcuts, CFI/%) | Web host done; bookmarks/annotations/prefs persist on Shelf SQLite |
| Mobile online Foliate WebView + progress sync | Done: WebView opens `/reader`, seeds session cookie, Foliate syncs CFI/%; catalogue refreshes on return |
| Display profile onboarding (`eink` default / `lcd`) | Done on mobile: first-run question, persisted preference, settings retoggle; e-ink disables animations |
| Offline book download + progress/annotation outbox | Done: Save offline cache; outbox (transport + HTTP 5xx); flush on refresh and connectivity; offline/LCD cached opens use Readium; pre-M4 localStorage one-shot migrate |
| Native Android EPUB navigator (e-ink profile) | Done: cached EPUB opens flutter_readium (paginated CSS); Shelf bookmarks/highlights UI; Locator ↔ CFI/% sync |
| Stylus / freehand ink on pages (native overlay) | Done M7–M8 for **Nomad** accelerated path + Flutter `noop` elsewhere. Next: generic Android tablet drawing, then golden-device backends — `docs/research/nomad-stylus.md` |
| Folder scanning and multiple libraries | Planned |
| KEPUB, MOBI, AZW, FB2 reading | Planned (Foliate and/or native navigator once EPUB path is solid) |
| PDF reading | Planned (strong fit for ink; after annotation/offline foundations) |
| Comic archive reading | Planned |
| Audiobook playback and background media controls | Planned |
| EPUB Media Overlay read-along and alignment | Planned; BO media-overlay UI is present, Shelf audio serving is not |
| Annotations, highlights, searchable/exportable notes | Persisted on Shelf; BO + e-ink Readium UI; export/search still Planned |
| Bookmarks | Persisted on Shelf; BO + e-ink Readium UI |
| Reader preference sync across devices | Per-book + account defaults on Shelf; chrome pin stays local |
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

1. **Stabilize the Book Orbit web reader host** — done: file open, CFI restore/save, Foliate assets, navigation, CI build of `web/orbit-reader`, AGPL attribution. No Shelf journal restyle of that chrome.

2. **Mobile online reader (shared app, both platforms)** — done: Flutter WebView → `/reader` with seeded `shelf_session` + Bearer, Foliate file/location sync, catalogue refresh on return. Remains the online LCD/iPhone path.

3. **Display profile onboarding** — done: persist `eink` vs `lcd` from first-run (“Are you using an e-ink display?”), retoggle from connect/library settings. E-ink disables animations and selects the Readium path when a cache exists.

4. **Persist reader data on Shelf** — done: bookmarks, highlights/notes, per-book preferences, and account reader defaults in SQLite; Foliate adapter uses those APIs (no localStorage for those paths). CFI/% remains the position contract. Unblocks web ↔ mobile sync and gives ink/highlights a server home before native drawing.

5. **Offline downloads + sync outbox** — done: Save offline stores EPUB bytes; sync outbox queues failed progress/location/bookmark/annotation/prefs writes (Flutter + Foliate ShelfHost) and flushes on library refresh. Foliate WebView open still needs the network (see follow-ups).

6. **Native Android navigator behind e-ink profile** — done: Android e-ink (and any offline cached open) uses `flutter_readium` for paginated CSS rendering, with Shelf-backed bookmarks/highlights and Locator ↔ CFI/% sync. Online LCD/iPhone keep Foliate WebView.

7. **Stylus ink overlay + stroke sync (+ tilt)** — done: Flutter ink over e-ink Readium (`Listener` stylus/eraser only), `ink_pipeline` (smoothstep + pressure→width + thin/EMA + REA `procreateStyleWidth` tilt factor; Lamy LUT skipped), `InkPoint` stores tilt/orientation, annotations `style: "ink"`, page-scoped redraw via stroke CFI, ink hidden from highlights list. Nomad via `Wacom-pen` MotionEvents. Reflowable EPUB ink is best-effort until PDF.

7.5. **Tilt / calligraphy width** — done in the M7 PR: `tiltWidthFactor` + orientation capture. Azimuth×brush-dir calligraphy blend still unused (Ratta logged it; width return did not need it).

8. **E-ink partial-refresh adapters** — done for **SuperNote Nomad only**: Dart `EinkRefresh` + Shelf `InkHostView` + SPEED/A2/tip lead (~0.15–0.2s tip lag). Not Notes HandWrite/PWCore. Other Android tablets stay Flutter Canvas (`noop`). Planned: generic tablet drawing first, then golden-device OEM backends when we can hack them. Lessons + do-not-retread: `docs/research/nomad-stylus.md`.

9. **PDF and comic readers** — after M7 (+ ideally M8 refresh on e-ink): fixed-layout is the strong ink surface; comics follow.

10. **TTS and media overlays** — Foliate/BO hooks first on the WebView path; native/e-ink listen features after reading stacks are stable. Then Storyteller-like extras (sleep timer, sync offset, PiP) where they fit.

11. **Library scale and organization** — folder scanning, editable metadata, authors/series/scopes/collections, storage/performance.

12. **Rich stats and external integrations** — session heatmaps, streaks, goals; Kobo/KOReader/OPDS; Hardcover/Readwise/StoryGraph; multi-user/OIDC as needed.

**Explicitly out of milestones (REA closed):** clone `ThreadSampleEmr`; ship OpenCV/`librPainter`; port Wacom G12/G14/Lamy LUT catalogs; Notes HandWrite/PWCore EMR. Raw `/dev/ebc` DISPAREA is an **optional Nomad experiment** (`debug.shelf.ink.ebc`), not the default product path. Future ink: generic Android tablet path → golden tablets (Nomad first); see `docs/research/nomad-stylus.md`.

Every milestone needs a branch/PR, meaningful tests, updated API contracts, and an accurate entry in the table above. Do not implement document parsers from scratch or provision paid services to fill a gap. Prefer one Flutter app with feature flags/profiles over forked products. Prefer Book Orbit's AGPL Foliate reader (with attribution) for the shared LCD/web experience; add native modules only where WebView cannot meet e-ink or stylus needs.

## Deferred follow-ups (closed)

Former M4/M5 ultra cuts, now shipped: connectivity outbox flush, HTTP 5xx enqueue, pre-M4 localStorage one-shot migrate, and offline/LCD open via Readium on a cached EPUB (not offline Foliate hosting). M6 polish shipped: `flutter_readium` pagination/CSS, Shelf bookmarks/highlights UI, Locator ↔ CFI/%.

## EPUB / Foliate host handoff, 2026-11-02

Shelf vendors Book Orbit's EPUB reader and Foliate under `web/orbit-reader/` and serves the Vite build from the Rust binary. Opening `/reader?id=` loads that SPA. Progress uses Shelf's location API. Bookmarks, annotations, and reader preferences/defaults hit Shelf SQLite APIs. Fonts, TTS backends, and media overlays remain stubbed locally where Shelf has no API yet.

The previous epub.js page (`web/reader.js`) is no longer routed. Import bounds and digest rules from decision 0003 remain. Decision 0004 records the Book Orbit base. Web and mobile use one **Add book** EPUB import action (manual-entry UI and fixed byte ceilings removed earlier).
