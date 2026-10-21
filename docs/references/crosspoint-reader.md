# Crosspoint Reader

Source map for reader policy. The work is scheduled in [product.md](../product.md) phases 4, 6, 7, and 11. This file does not open a phase and it does not change that order. The earliest phase there whose "done when" is still open stays the current work. Phase 3 already requires buttons for session switch and the boot target, so that part of the button rule is not repeated there.

Source: [crosspoint-reader](https://github.com/crosspoint-reader/crosspoint-reader), default branch `develop` at `f8936ba` (21 Oct 2026), release 1.6.5, MIT. Paths below are from that tree. Open the file when the named phase starts. Adopt the policy. Leave the mechanism they used to get there.

Our locked choices still win: Readium for EPUB, MuPDF for PDF, damage modes `fast` / `quality` / `full`, gray8 until the color-plane phase, SQLite on the device, shelf for catalog and sync, CFI as the EPUB position, content hash plus library id as book identity.

## What to take

### 1. Position is a text offset. The page number is a rendering

| | |
| --- | --- |
| Take | Store an EPUB position as a CFI. Derive the page number from the visible text of the current layout. A relayout may change the page number and must not move the position. |
| When | Phase 4 (Local reader) draws the page. Phase 7 (Progress sync) stores and merges the CFI. |
| From | `lib/Epub/Epub/Section.h` `getVisibleTextOffsetForPage`: visible Unicode codepoint offset per page. `lib/KOReaderSync/ProgressMapper.h` builds an XPath only when talking to KOReader. `lib/KOReaderSync/KOReaderDocumentId.h` is their KOReader document id. |
| Leave | XPath as our position. Their partial MD5 as a shelf identity. |

### 2. Layout cache key is the full render spec

| | |
| --- | --- |
| Take | Key a page cache by the whole typography and viewport spec. Write the cache in pieces. Keep building while the reader is idle. Drop the cache when the spec changes. |
| When | Phase 4 (Local reader), once Readium is producing pages. |
| From | `lib/Epub/Epub/Section.cpp`: `Section::loadSectionFile`, `buildSomeMore` (yields every 1 KB), `suspendBuild`, `SECTION_FILE_VERSION` 64. `lib/Epub/Epub/ReaderRenderSpec.h` is the key. `EpubReaderActivity::backgroundBuildWanted` is the idle continuation. |
| Leave | `ChapterHtmlSlimParser`. Their `.bin` section format. The version number. |

### 3. A clearing refresh after a run of fast updates

| | |
| --- | --- |
| Take | After N fast page updates, one clearing refresh. A gray intermediate must not restroke the whole page. Map the cycle onto `fast`, `quality`, and `full`. Their user setting defaults to 15, with choices 1, 5, 10, 15, 30, and never. On a grayscale panel they overlap the CPU gray conversion with the panel refresh and leave the framebuffer alone until that refresh finishes. |
| When | Phase 4 (Local reader), where the reader presenter chooses the mode. The phase 2 desktop hook keeps its own rule: rect size, time since the last frame, and a partial-area budget. |
| From | `src/activities/reader/ReaderUtils.h` `displayWithRefreshCycle` and `displayBaseWithRefreshCycle`. The counter is `pagesUntilFullRefresh`. It selects `HALF_REFRESH` (about 1720 ms), not `FULL` (black flash). `lib/hal/HalDisplay.h` names the modes. `EpubReaderActivity::renderContents` branches on `grayscaleCapabilities()` (`stripUploads`, `asyncBase`, `GrayscaleBase::Combined`) and waits in `waitRefreshComplete()`. |
| Leave | Their mode names. Calling that periodic clear `full` while the panel command is `HALF`. Any change to the desktop area budget. |

### 4. Buttons do every action. Touch adds zones

| | |
| --- | --- |
| Take | Every reader action has a button. Touch only adds hit zones. One page-turn direction: the whole page is that direction. Both directions: the left third versus the rest, swapped for RTL and for inverted tap. The center third opens the menu and is not a page turn. Edge swipes: left edge, swipe right, Back; bottom edge, swipe up, Home; top edge, swipe down, Menu. Remap buttons from the renderer's current orientation. Keys arrive as evdev through seatd and libinput. |
| When | Phase 3 (Reader/notes shell) already requires buttons for session switch and the boot target. The zone map, RTL swap, and edge swipes are phase 4 (Local reader), with page turns. |
| From | `src/activities/reader/ReaderUtils.h` `detectPageTurn` and `detectTouchPageTurn`. `src/MappedInputManager.h`. `docs/contributing/touch-and-ui.md`. |
| Leave | Tilt. BLE. A touch-only path for any action. |

### 5. Highlight anchors survive relayout. Deletes are journaled first

| | |
| --- | --- |
| Take | Store a highlight as a CFI range plus a stable id. Chapter text offsets outrank a page index or a word index. A layout signature and a page count decide when those indexes are still valid. Write a delete to the outbox before removing the local row. Carry pending and revision fields. Shelf owns the wire protocol. |
| When | Phase 4 (Local reader) draws the mark in gray8. Phase 7 (Progress sync) is the outbox and the merge. Phase 11 (Color plane) is when the mark gains color. |
| From | `src/ClippingStore.h`. `src/clippings/ClippingSync.h`. `docs/file-formats.md`, clipping store version 4: chapter codepoint offsets, `layoutSignature`, `pageCount`, `pendingUpload`, `syncRevision`, stable id. A delete is written to a sibling `.deleted` file before the local row is removed. |
| Leave | `My Clippings.txt`. Screen rectangles as the stored anchor. Their sync protocol. |

### 6. Dither for the artifact. Never rescale a dithered bitmap

| | |
| --- | --- |
| Take | Dither in the damage buffer, from gray8 down to the profile `gray_levels`. Pick the dither for the artifact: ordered dither for flat color, error diffusion for images and covers. Generate a cover thumbnail at the size it will be drawn. Text anti-aliasing is a second pass on the same layout, not a second parse. |
| When | Phase 4 (Local reader). |
| From | `lib/Epub/Epub/converters/DitherUtils.h` `applyBayerDither4Level` (ordered 4×4 Bayer, used for flat color). `lib/GfxRenderer/BitmapHelpers.h` Atkinson and Floyd–Steinberg for images and covers. `BaseTheme` and `CoverGridHomeUi` refuse to rescale a dithered bitmap. `SETTINGS.textAntiAliasing` is the second pass. `test/absolute_grayscale/` holds the image checks. |
| Leave | Their 4-level overlay as a stand-in for `quality` (16 levels). A color plane before phase 11. |

### 7. Small writes are atomic. A broken index stays visible

| | |
| --- | --- |
| Take | Write a small durable file by temp file and rename. If a scan or an index cannot finish, keep showing the books that are still known and record that the result is degraded. On this project the durable store is SQLite, so the crash-safe write is a transaction, not a sidecar progress file. |
| When | Phase 6 (Shelf library) for a scan that cannot finish. Phase 7 (Progress sync) for the outbox. |
| From | `src/activities/reader/ProgressFile.h` `writeAtomic` (temp file plus rename; issue #2275, a torn FAT write). `lib/LibraryIndex/LibraryFormat.h`: `CLX1` fixed 128-byte records, max 4096, flags `RANKS_DEGRADED` and `DEDUP_DEGRADED`. `lib/LibraryIndex/LibraryBuilder.cpp` skipped a content hash because it would read about 12 KB per book; a rename match there is file size. |
| Leave | The binary index. File size as identity. Our identity stays a content hash plus a library id, in SQLite. |

### 8. Sleep leaves the frame that finished. Wake does not flash it

| | |
| --- | --- |
| Take | Sleep keeps the last finished frame on the panel. Wake must not clear it with a flash. Restore the frontlight only when the profile says `frontlight` is yes. |
| When | A later board port, for the sleep and wake behavior. Phase 12 (Notes and frontlight) for the light. Listed under Deferred in product.md. |
| From | `src/main.cpp` `enterDeepSleep`. `lib/hal/HalDisplay.h` `begin(seamless)`. Frontlight restore runs only on boards that have a light. |
| Leave | ESP32 deep sleep. The `FREEINK_DEVICE_PICCO` USB charge strip. |

### 9. Typography is a Readium acceptance list

| | |
| --- | --- |
| Take | Treat their CSS and layout notes as checks on Readium output. CSS they honor: alignment, bold, italic, underline, line-through, indent, margin, padding, a few border styles, `display: none`, superscript and subscript, list disc or none, white-space, font-size inheritance. Tables: measure column widths, then a grid or a stacked full-width flow. Hyphenation: Liang patterns, language from the OPF. CJK uses the SD fonts. Korean keeps `word-break: keep-all`. RTL uses a bidi pass. Footnote hit targets: at most 16 per page, at least 28 px wide, 6 px of horizontal slop. |
| When | Phase 4 (Local reader). |
| From | `lib/Epub/Epub/css/CssStyle.h`. `lib/Epub/Epub/hyphenation/Hyphenator.h`. `docs/file-formats.md` (widows and orphans, noted around section format version 56). `EpubReaderUtils::linkAtPoint` for the footnote cap and the hit slop. RTL is MiniBidi. |
| Leave | Their EPUB parser. Copying any of this into a second layout engine. |

### 10. Host-test the pure decisions. The panel stays on the emulator

| | |
| --- | --- |
| Take | Pytest covers a pure decision: which damage mode, which profile branch, how an outbox merges. Behavior on the panel is checked at `:8006`. |
| When | Damage mode choice and profile branching are already tested with the desktop hook. Outbox merge tests arrive with phase 7 (Progress sync). This is the existing test split, not a new phase bullet. |
| From | `test/CMakeLists.txt`. `docs/contributing/testing-debugging.md`. Their host tests never drive the panel; the device does. |
| Leave | Their firmware test harness. |

## Do not copy

These are not features to research later.

| What | Where it shows up | Why it stays out |
| --- | --- | --- |
| Path as a cache id | `Epub::Epub` hashes the path with `std::hash`. The README calls that a content hash. It is not. The library index uses an FNV-1a of the path. KOReader sync uses a partial MD5. Three ids, none of them the file bytes. | Book identity is a content hash plus a library id. |
| Per-device branches in the reader | `isEegoA4`, `deviceIsX3`, `Controller::UC8279`, `FREEINK_DEVICE_*` in `src/main.cpp`. | The capability profile is the part to keep. Device checks inside the reader are the failure. |
| A clear named `full` that sends `HALF` | `pagesUntilFullRefresh` selects `HALF_REFRESH`. | Our `full` is the flash that clears residue. |
| Section-format churn | `SECTION_FILE_VERSION` moves often. The layout signature omits `paragraphIndentSpaces`. | We do not take the `.bin` format, so we do not take its version or its incomplete key. |
| Their EPUB parser and the in-browser optimizer | `USER_GUIDE.md`, PR #1224. | EPUB goes through Readium. Rewriting the file fights the content hash. |
| Heap tricks | `lendFrameBufferStorage`. Refusal to use `std::function`. | Firmware memory tactics. The guest is Linux. |
| The device web server, obfuscated OPDS passwords, on-device DRM, SD plugins as a second catalog | `docs/webserver.md` (no authentication). `OpdsServerStore.h` (XOR-obfuscated OPDS passwords). `BookKey`, `LcpDevice`. `docs/sd-plugins.md`. | Shelf is the catalog. Accounts live there. DRM is out. |

## Gaps, now scheduled

Each row is a hole their mechanism makes visible. The matching bullet in [product.md](../product.md) is the work.

| Gap | Phase | What exposed it |
| --- | --- | --- |
| Reader refresh policy beside the desktop area budget. The reader counts fast updates. The desktop hook sums rect area. They stay separate policies. | 4 Local reader | `displayWithRefreshCycle` versus the phase 2 hook in `distro/panel/oseink-refresh.h` |
| First open of a large EPUB has no page cache. Build in pieces and continue while idle, or the first open blocks on the whole book. | 4 Local reader | `buildSomeMore`, `backgroundBuildWanted` |
| A highlight is a CFI range. A screen rectangle does not survive relayout. | 4 Local reader, sync in 7 Progress sync | Clipping store: chapter codepoint offsets win over page and word indexes |
| Grow hit targets sideways. A target that grows down the line steals the page-turn zone. | 4 Local reader | `linkAtPoint`: 28 px minimum width, 6 px horizontal slop. Center third is the menu (`detectTouchPageTurn`) |
| Reading chrome has to list its fields, and every field has to work with buttons only. | 3 Reader/notes shell for home and settings. 4 Local reader for the reading chrome | `docs/contributing/touch-and-ui.md`: buttons perform every action, touch only adds zones |
| A broken EPUB shows a failure, and a button leaves it. | 4 Local reader | Same button rule. A failed section load still has to be dismissable with no touch |
| The device cache uses the same content hash as shelf. A rename must not fork progress. | 4 Local reader for a local cache. 7 Progress sync for the copy that syncs | `LibraryBuilder.cpp` matches a rename by file size and skipped the content hash |
| Wi-Fi comes up only to drain the outbox. A delete is journaled, then the local row is removed, then the radio comes up. | 7 Progress sync | `ClippingSync`: sibling `.deleted` file before the local row is removed. `pendingUpload` |
