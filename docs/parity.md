# Book Orbit feature roadmap

Reference: [Book Orbit](https://github.com/bookorbit/bookorbit), reviewed 2026-11-02. This is a parity target, not a claim that Shelf already implements every Book Orbit product surface.

Shelf's shared EPUB reader is Book Orbit's Foliate/Vue reader hosted under `web/orbit-reader/` (AGPL-3.0-only, attributed in tree). Shelf does not restyle that reader to the journal theme while the Foliate host is the primary surface. Storyteller remains a reference for later listening/sync behaviors, not the default reader base.

## Mobile product shape (decided)

- **One Flutter app** builds for Android and iPhone. No separate e-ink or stylus codebases.
- **Display profile:** `eink` (default) or `lcd`. First-run onboarding asks whether the device is an e-ink display; choosing no switches to the normal LCD experience (and can be changed later in settings).
- **E-ink default on** means e-ink-friendly defaults (contrast, chrome density, refresh-oriented behavior). Mobile reading always uses the Readium navigator (`flutter_readium`); Foliate stays the web/desktop host only.
- **Platform reality:** one Flutter app; Read book fetches/caches the EPUB then opens Readium on Android and iPhone. Stylus ink overlays that same surface.

## Capability status

| Capability | Shelf status |
| --- | --- |
| First-account setup, login, session revocation | Implemented; mobile stores the session token in platform secure storage; server uses a 30-day sliding idle window renewed on authenticated requests |
| Manual catalogue entries and persisted page progress | Implemented on web and shared mobile |
| Overview, library search, real catalogue/progress totals | Implemented on web; catalogue/totals on mobile |
| Android and iPhone shared client | Foundation with login, persisted secure session, catalogue, add book, record progress, Readium EPUB/PDF reader |
| Single-container Compose packaging | Implemented; published image begins after a successful main release |
| Independent semantic server/mobile releases | Workflow implemented; publication verified only after main release |
| EPUB upload/import and authenticated downloads | Implemented; exact-byte deduplication and embedded raster covers; single **Add book** action |
| EPUB reading (Book Orbit Foliate: chapters, settings, search, shortcuts, CFI/%) | Web host done; bookmarks/annotations/prefs persist on Shelf SQLite |
| Mobile EPUB reading (Readium) | Done: Read downloads/caches EPUB if needed, opens `flutter_readium`; progress/bookmarks/annotations via Shelf APIs. Foliate WebView removed from mobile. |
| Display profile onboarding (`eink` default / `lcd`) | Done on mobile: first-run question, persisted preference, settings retoggle; e-ink disables animations |
| Offline book download + progress/annotation outbox | Done: Save offline / auto-cache on Read; outbox (transport + HTTP 5xx); flush on refresh and connectivity; pre-M4 localStorage one-shot migrate |
| Native mobile EPUB navigator (Readium) | Done: sole mobile reader; auto-cache on Read; bookmarks/highlights UI; Locator ↔ CFI/% sync |
| Stylus / freehand ink on pages (native overlay) | **M7–M8 complete** (2026-11-04): Nomad SPEED+A2+tip lead (~0.15–0.2s); Flutter `noop` elsewhere. Mid-stroke fill-in experiments closed. Drawing G1/G2 is milestone 13. |
| Folder scanning and multiple libraries | Planned |
| KEPUB, MOBI, AZW, FB2 reading | Planned (Foliate and/or native navigator once EPUB path is solid) |
| PDF reading | **M9 web + mobile PDF** — ingest + BO `pdf-v4`; mobile Readium PDF open/cache with shared `pdfpage(N)`, fit-to-page, tap-to-hide chrome, page/progress footer — `docs/research/m9-pdf-comics-leverage.md` |
| Comic archive reading | **M9 web + mobile CBZ/CBR/CB7/DjVu** — CBZ as ZIP; CBR/CB7/DjVu → ZIP-of-images at ingest (labels preserved; original on `/source`); shared page API + `cbzpage(N)`; DjVu embedded text sheet; web BO comic reader; mobile Readium ZIP path with scroll/RTL/spread prefs — same leverage doc |
| Audiobook playback and background media controls | Planned (later M10 slice — standalone audio, not EPUB narration) |
| EPUB Media Overlay read-along and alignment | **Web usable** — Foliate plays embedded narration; Shelf playlist API for resume; streaming package files and mobile MO still Planned |
| Web OpenAI-compatible / Kokoro TTS | **Usable when configured** — Settings `#settings` providers; reader listen hidden until an enabled provider exists; synthesize proxied by Shelf; see `docs/research/m10-tts-listen-leverage.md` |
| Browser SpeechSynthesis | Dropped for web (not BO’s path); system voices not required |
| Mobile / e-ink listen | **In progress (M10 slice 4)** — EPUB headphones when online + enabled Shelf TTS provider; voice select + server synthesize stream; hidden offline / no providers |
| Annotations, highlights, searchable/exportable notes | Persisted on Shelf; BO + e-ink Readium UI; export/search still Planned |
| Bookmarks | Persisted on Shelf; BO + e-ink Readium UI |
| Reader preference sync across devices | Per-book + account defaults on Shelf; chrome pin stays local |
| Kobo and KOReader integrations | Planned |
| OPDS and Send-to-Kindle | Planned |
| Metadata providers and cover enrichment | Planned |
| Authors, series, smart scopes, collections | Planned |
| Session-based statistics, heatmaps, streaks, Reading DNA | Planned; overview average progress; catalogue `reading_status` (`open`/`finished`/`unfinished`/`rereading`) stored for later reopen metrics |
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

2. **Mobile online reader (shared app, both platforms)** — superseded: mobile no longer embeds Foliate WebView; Read always uses Readium after ensuring a local EPUB cache (download when online). Foliate remains the web host only.

3. **Display profile onboarding** — done: persist `eink` vs `lcd` from first-run (“Are you using an e-ink display?”), retoggle from connect/library settings. E-ink disables animations; reading path is Readium regardless of profile.

4. **Persist reader data on Shelf** — done: bookmarks, highlights/notes, per-book preferences, and account reader defaults in SQLite; Foliate adapter uses those APIs (no localStorage for those paths). CFI/% remains the position contract. Unblocks web ↔ mobile sync and gives ink/highlights a server home before native drawing.

5. **Offline downloads + sync outbox** — done: Save offline stores EPUB bytes; Read auto-downloads when online and uncached; sync outbox queues failed progress/location/bookmark/annotation/prefs writes and flushes on library refresh.

6. **Native mobile navigator** — done: `flutter_readium` is the only mobile EPUB reader (e-ink and LCD, online and offline once cached).

7. **Stylus ink overlay + stroke sync (+ tilt)** — done: Flutter ink over e-ink Readium (`Listener` stylus/eraser only), `ink_pipeline` (smoothstep + pressure→width + thin/EMA + REA `procreateStyleWidth` tilt factor; Lamy LUT skipped), `InkPoint` stores tilt/orientation, annotations `style: "ink"`, page-scoped redraw via stroke CFI, ink hidden from highlights list. Nomad via `Wacom-pen` MotionEvents. Reflowable EPUB ink is best-effort until PDF.

7.5. **Tilt / calligraphy width** — done in the M7 PR: `tiltWidthFactor` + orientation capture. Azimuth×brush-dir calligraphy blend still unused (Ratta logged it; width return did not need it).

8. **E-ink partial-refresh adapters** — **complete for SuperNote Nomad** (2026-11-04): Dart `EinkRefresh` + Shelf `InkHostView` + SPEED/A2/tip lead (~0.15–0.2s tip lag). Not Notes HandWrite/PWCore. Live A2 thinner until ink-off softDu — accepted. Experimental `/dev/ebc` DISPAREA stays opt-in (`debug.shelf.ink.ebc`), not default. Other Android tablets stay Flutter Canvas (`noop`). Lessons + do-not-retread: `docs/research/nomad-stylus.md`.

9. **PDF and comic readers** — web PDF + mobile Readium PDF done; web + mobile CBZ/CBR/CB7/DjVu done (ZIP-normalized ingest, `/source` originals, DjVu text sheet, mobile comic view prefs). **Leverage map:** [`docs/research/m9-pdf-comics-leverage.md`](research/m9-pdf-comics-leverage.md).

10. **TTS and media overlays** — fly path in [`docs/research/m10-tts-listen-leverage.md`](research/m10-tts-listen-leverage.md): (1) dropped browser SpeechSynthesis, (2) OpenAI-compatible/Kokoro, (3) web media overlays, (4) mobile server TTS, (5) mobile media overlays, (6) standalone audiobook player, (7) Storyteller extras (sleep timer, sync offset, PiP, alignment).

11. **Library scale and organization** — folder scanning, editable metadata, authors/series/scopes/collections, storage/performance.

12. **Rich stats and external integrations** — session heatmaps, streaks, goals; Kobo/KOReader/OPDS; Hardcover/Readwise/StoryGraph; multi-user/OIDC as needed.

13. **Post-M8 drawing: generic tablet ink → golden OEM backends (G1 → G2)** — final drawing milestone. G1: portable mid-stroke path (Surface/Flutter) without OEM refresh APIs, same `EinkRefresh` contract. G2: deepen devices we can physically hack (Nomad already golden; e.g. Onyx `EpdController` only with hardware + smoke). Ledger: `docs/research/nomad-stylus.md`.

**Explicitly out of milestones (REA closed):** clone `ThreadSampleEmr`; ship OpenCV/`librPainter`; port Wacom G12/G14/Lamy LUT catalogs; Notes HandWrite/PWCore EMR. Raw `/dev/ebc` DISPAREA is an **optional Nomad experiment** (`debug.shelf.ink.ebc`), not the default product path.

Every milestone needs a branch/PR, meaningful tests, updated API contracts, and an accurate entry in the table above. Do not implement document parsers from scratch or provision paid services to fill a gap. Prefer one Flutter app with feature flags/profiles over forked products. Prefer Book Orbit's AGPL Foliate reader (with attribution) for the **web** experience; mobile EPUB reading is Readium-only.

## Deferred follow-ups (closed)

Former M4/M5 ultra cuts, now shipped: connectivity outbox flush, HTTP 5xx enqueue, pre-M4 localStorage one-shot migrate. M6: `flutter_readium` is the only mobile EPUB reader (auto-cache on Read); Foliate WebView removed from the app.

## EPUB / Foliate host handoff, 2026-11-02

Shelf vendors Book Orbit's EPUB reader and Foliate under `web/orbit-reader/` and serves the Vite build from the Rust binary. Opening `/reader?id=` loads that SPA. Progress uses Shelf's location API. Bookmarks, annotations, and reader preferences/defaults hit Shelf SQLite APIs. Fonts, TTS backends, and media overlays remain stubbed locally where Shelf has no API yet.

The previous epub.js page (`web/reader.js`) is no longer routed. Import bounds and digest rules from decision 0003 remain. Decision 0004 records the Book Orbit base. Web and mobile use one **Add book** EPUB import action (manual-entry UI and fixed byte ceilings removed earlier).
