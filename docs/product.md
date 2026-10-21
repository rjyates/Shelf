# oseink product decisions

The emulator (GitHub issues 1 through 8 on [rjyates/oseink](https://github.com/rjyates/oseink/issues)) is done: it boots a Linux image, hot-reloads the shell, and simulates the panel. This document is the next body of work.

## Two products

oseink is a device OS plus a self-hosted service. They share a protocol and stay in this repo.

| Piece | Path | Runs on | Job |
| --- | --- | --- | --- |
| Emulator | `emulator/` | Dev machine | Boot the image, hot-reload the shell, simulate the panel |
| Distro | `distro/` | Device and emulator | Kernel, rootfs, panel driver, capability probe, session launch |
| Shell | `shell/` | Device and emulator | Reader/notes session: library, reader, notes, audio, cache, sync |
| Desktop | distro session | Device and emulator | Full Linux desktop on the same panel path |
| Shelf | `shelf/` | A server the user runs | Catalog, files, progress, alignment jobs, acquisition |
| Protocol | `protocol/` | Both | Request and response types for catalog, files, and sync |

Every pixel that reaches the panel goes through the damage protocol from issue 7 (`quality` / `fast` / `full` plus a damage rect and gray pixels; see `emulator/viewer/damage.js`), including desktop windows. Shelf never learns waveform timings, refresh modes, or panel product names.

## Session model

The device is Linux. People who want a simple reader get one. People who want a normal Linux machine get one. Same image, two sessions, patterned after SteamOS (Game Mode vs Desktop Mode):

| Session | What it is | Who it is for |
| --- | --- | --- |
| Reader/notes | Minimal environment: library, reading, notes, settings. No general window desktop. | Default. Controls behave like a dedicated e-ink device. |
| Desktop | A real Linux desktop (apps, terminal, browser, whatever the user installs). | Hackers and anyone who needs the rest of the machine. |

Rules:

- Default boot enters the reader/notes session.
- From reader/notes, the user can switch to desktop. From desktop, they can return to reader/notes.
- Settings can set boot to desktop so the device skips reader/notes on power-on.
- Desktop does not bypass e-ink safeguards. The compositor and panel driver still throttle refresh, map updates to damage modes, and honor the capability profile so ordinary Linux apps cannot thrash or burn the panel the way they would on LCD.
- The reader/notes session branches on the capability profile. It does not ship per-device UI forks (no Kindle/Kobo/reMarkable skins).
- Desktop may run third-party software the user installs. That does not change the built-in reader, and we do not treat third-party readers as a substitute for `shell/`.

## What we take from the references

[Storyteller](https://github.com/smoores-dev/storyteller) is the read-along model and the source to read when implementing it. An ebook and a narration are aligned so text and audio share one position. Alignment is a server job. The device only plays the result. Interchange format is EPUB 3 Media Overlays. Import Storyteller output. Do not embed Storyteller's web or mobile apps.

When phase 8 (playback) and phase 9 (alignment) start, read that repo for the EPUB 3 behavior:

- [`@smoores/epub`](https://www.npmjs.com/package/@smoores/epub) is how they edit the package: metadata, spine, and writing Media Overlays back into the EPUB. Shelf's alignment job follows that shape.
- Their mobile reader is the reference for highlighting the current sentence from those overlays and driving the audio clock from one position. Readium's own Media Overlay support is still planned, so this playback code is the reference for the gap.
- `tools/crengine-oracle` is only their port of crengine's xpointer, for positions. Page rendering stays Readium.

[BookOrbit](https://github.com/bookorbit/bookorbit) is the library model: a Docker service the user hosts, progress and highlights that follow the reader, and books that can stream or download. One book has one position across reading and listening. Ship OPDS so other readers can browse the catalog. Skip, until someone asks, the achievement system, the phone apps, and the long list of metadata providers.

[Cinephage](https://github.com/MoldyTaint/Cinephage) is the shape of the acquisition service: one process, one database, one web UI, from a request through import into the library. Books only. Shelf does not manage video, subtitles, or live TV, and it does not ship indexer definitions or books. The operator points it at files they already have, a drop folder, an OPDS server, or a download client they already run.

SteamOS is the session pattern only: a constrained default environment, a full desktop on the same OS, and a setting for which one boots. The session switch is a boot-target file and a BusyBox restart, not gamescope, SDDM, or systemd.

[Crosspoint Reader](https://github.com/crosspoint-reader/crosspoint-reader) is the reader-policy reference: stored position, layout cache, clearing refresh, button-first input, highlights, and dither. Paths and what to leave behind are in [references/crosspoint-reader.md](references/crosspoint-reader.md). The work is in phases 4, 6, 7, and 11. That file does not add a phase.

## Locked technical choices

- Device image: Buildroot. The emulator guest is x86_64 under KVM. Hardware images are a later board port of the same userspace.
- Sessions: reader/notes (default) and desktop, switchable at runtime, boot target configurable in settings.
- Panel path: one damage protocol and capability profile for both sessions. The panel device is that protocol. Apps never open it.
- Desktop presenter: Weston 14.0.1 from Buildroot 2025.02.6, pixman renderer, desktop-shell, headless backend. No GL, no DRM backend. Animations off (`animation` none, `idle-time=0`).
- X11: the image has no XWayland and no X server. Desktop apps are Wayland clients of Weston only.
- Desktop pixels: a repaint hook in the Weston headless backend is the only desktop writer of the damage protocol. It grays the pixman damage region, picks `quality`, `fast`, or `full` from rect size, time since the last frame, and a partial-area budget, and stalls `finish_frame` until the panel delay has elapsed. There is no app-facing waveform API. Busy times are in Panel timing.
- Reader/notes presenter: a damage-protocol client that sets the mode itself. It is not a Weston client. Session switch stops one presenter and starts the other.
- Input: seatd and libinput for both sessions. One session holds the seat. Buttons are evdev keys. Touch and stylus are libinput devices.
- Session manager: a BusyBox init script and `/oseink/run/boot-target`, contents `reader` (default) or `desktop`. Switching rewrites the file and restarts the presenter without rebooting the guest.
- Device storage: SQLite for the library cache, the outbox, and positions. Book files live on the local filesystem.
- Shelf deploy: one Docker image, one container, SQLite, book files on a local disk. Compose is the install path, same expectation as BookOrbit and Cinephage.
- Shelf stack: TypeScript on Node 22. The admin UI is served by that same process. Alignment tools run as subprocesses (ffmpeg and a forced-alignment binary), not as a second service.
- Shelf storage warning: library paths, the SQLite file, and import folders stay on a local filesystem. Network mounts are unsupported for the same reason BookOrbit refuses them.
- Accounts: local users first. OIDC is a later shelf setting. Progress is per user.
- Identity of a book: a library id plus a content hash of the canonical file, so a rename does not fork progress.
- Position: EPUB CFI for EPUB, page index for PDF, chapter plus milliseconds for audio. For a read-along book the CFI is the position and the audio clock is derived from the alignment map.
- Conflict rule: each field has its own `updated_at`. A highlight from one device and a position from another both stick. The device keeps an outbox and is authoritative while offline.
- Radio: Wi-Fi comes up to sync or to fetch a file, then goes down. The reader/notes session stays usable with no server configured.
- Reader UI: our own toolkit. Home, library, settings, and chrome draw into the damage buffer. No GTK and no Qt. The shell stays a damage client.
- Engines: MuPDF for PDF. Readium for EPUB 2 and EPUB 3. The shell does not include a document parser. Readium's navigator shows reflowable XHTML in its web view; the shell copies that view into the damage buffer.

## Left unlocked

- Moving the reader into Weston kiosk-shell later, if a side channel for the damage mode is worth a single presenter.
- Extra desktop apps beyond `weston-terminal` (file manager, browser).
- The numeric partial-to-full budget, waveform files, and temperature bands. The budget constant and the Panel timing busy times can move. Do not vendor LUT blobs.
- musl, and systemd. This image is glibc and BusyBox.
- Which embedded web renderer hosts the Readium navigator (WPE WebKit or an equivalent that can run without X11).

## Display capabilities

The distro probes the hardware once at boot and publishes a capability profile. Both sessions honor that profile. The reader/notes UI branches on it. It does not contain a table of Kindle, Kobo, or reMarkable models.

```text
color: gray | kaleido | full
gray_levels: 2 | 16
input: buttons | touch | stylus | touch_and_stylus | buttons_and_touch | buttons_and_stylus | buttons_and_touch_and_stylus
frontlight: yes | no
width, height, dpi
```

Rules:

- Every reader/notes screen works with buttons alone. Touch and stylus add gestures. They are never required to turn a page, open a book, or change a setting.
- Body text is rendered in gray and dithered to `gray_levels`, including on color panels. Color e-ink is reserved for covers, highlight marks, and pen ink.
- `kaleido` and `full` use different color paths. Kaleido is a filter overlay. Full color is a color particle panel. The shell treats them as different profile values.
- Stylus present: store ink as vector strokes on that book and sync them as annotations. Stylus absent: hide ink tools. Touch can still create a highlight from a selection. Buttons can set a bookmark.
- Frontlight absent: hide the light control.
- The emulator's panel simulator must accept this same profile so a gray button-only device and a color touch-and-stylus device can both be tested before hardware exists. The viewer at `:8006` sets color, input, and size separately. Color is black and white (`gray`) or color (`kaleido`, or `full` when the file already says so). Input is any combination of buttons, touch, and stylus. Size is width and height in pixels, from 1 to 4096. The published file is `/oseink/run/profile.json` (on the emulator, `distro/overlay/run/profile.json`). The guest reads that file at session start, the same way a device reads the boot probe. Changing it in the viewer restarts the session. Compose does not set the profile.

## Panel timing

The three damage modes are refresh classes from a Rockchip EBC controller. The reference tree is [Supernote-Ratta/kernel_Nomad_Manta](https://github.com/Supernote-Ratta/kernel_Nomad_Manta), `drivers/gpu/drm/rockchip/ebc-dev`. Userspace submits a buffer and a mode (`EBC_SEND_BUFFER`, `enum panel_refresh_mode`). `epd_lut_get` returns a frame count for that mode and the panel temperature. The TCON plays that many frames (`DSP_FRM_TOTAL`). The driver blocks until the frame-end interrupt (`DSP_END_INT`). One update is in flight for that whole interval. A partial update leaves residue. `EPD_RESET` and `EPD_FORCE_FULL` clear it with a flash.

`EBC_SET_FULL_MODE_NUM` forces a full refresh after a count of partials. The hook's rule is area: rect size, time since the last frame, and a partial-area budget. Copy this wait. Leave the Ratta waveform blobs out of the tree (`pvi_waveform_v8.S`, `rkf_waveform_v8.S`, `ht_ebc.px`).

The desktop hook publishes the gray8 frame, then stalls `finish_frame` for that mode's busy time. `:8006` plays the ghost or the flash across that same interval. The picture settling is the end of the busy time.

Room-temperature stand-ins come from the E Ink mode declaration (A2, GC16, INIT). The hook and `emulator/viewer/damage.js` share this table. The numbers can move.

| mode | class | gray levels | residue | busy |
| --- | --- | --- | --- | --- |
| `fast` | A2 | 2 | harsh ghost | 120ms |
| `quality` | GC16 | 16 | soft ghost | 450ms |
| `full` | INIT / RESET / `EPD_FORCE_FULL` | 16 | flash, then clear | 2000ms |

DU is the slower black-and-white sibling of A2, about 260ms. `fast` uses the A2 time, which is the typing and menu path.

The hook and `emulator/viewer/damage.js` use the busy times above. The first rect that covers the panel is `full`. A later one is `quality`, so a desktop redraw does not flash again. A rect under one eighth of the panel is `fast` when the previous frame was under 450ms ago, and `quality` otherwise. Partials also become `full` after they add up to four panel-areas. That budget can move.

## Phases

Work these in order. A phase can start when the previous one's "done when" is true. Shelf phases can be developed on the host against the protocol. Device UI phases use the emulator.

### 1. Session contract

Foundation. No shelf yet. No book library. Presenters and the damage hook are the locked desktop stack above. Issues: [#16](https://github.com/rjyates/oseink/issues/16) profile file, [#17](https://github.com/rjyates/oseink/issues/17) viewer picker, [#18](https://github.com/rjyates/oseink/issues/18) damage frames, [#19](https://github.com/rjyates/oseink/issues/19) boot target and switch. Milestone: Session contract.

- Publish the capability profile from the distro and from the emulator. On the emulator, the `:8006` viewer selects it.
- Panel output for both sessions uses the damage protocol (retire the VGA/VNC bridge as the real paint path for session UIs).
- Define how sessions start, how the user switches, and the persisted boot target: reader/notes (default) or desktop.

Done when the emulator boots to the saved session preference, can switch sessions without rebuilding the image, and a test frame from either session shows e-ink delay, ghosting, and mode behavior on `:8006`.

### 2. Minimal desktop

A real Linux desktop session on the phase 1 panel path. E-ink safeguards sit under the apps and keep refresh throttled. Uses the stack recorded in Locked technical choices. Issues: [#24](https://github.com/rjyates/oseink/issues/24) Weston session, [#25](https://github.com/rjyates/oseink/issues/25) repaint hook, [#26](https://github.com/rjyates/oseink/issues/26) panel input and return. Milestone: Minimal desktop.

- Ordinary window updates go through damage modes.
- The user can return to reader/notes.

Done when the desktop session runs in the emulator, ordinary window updates still go through damage modes, and the user can return to reader/notes.

### 3. Reader/notes shell

Minimal full-screen reader/notes environment: home, and a settings stub for the boot target and switch to desktop. EPUB and PDF engines are phase 4.

- The same binary and UI work on both capability profiles at chrome level (navigation and settings).
- Buttons alone can switch session and change the boot target.

Done when that chrome is the default session on button-only gray and on touch-and-stylus color, and buttons alone can switch session and change the boot target.

### 4. Local reader

Builds on the reader/notes session. The policies below come from [references/crosspoint-reader.md](references/crosspoint-reader.md).

- Shell opens a local folder of EPUBs and PDFs. EPUB pages come from Readium. PDF pages come from MuPDF. Readium's page output covers the typography checks in that reference: the CSS subset, tables, hyphenation, CJK, RTL, widows and orphans, and footnote hit targets.
- The stored EPUB position is a CFI. The page number is derived from the current layout. A relayout may change the page number and keeps the CFI. PDF keeps a page index. The local cache uses the content hash from the identity rule, so a rename does not fork the position.
- The page cache key is the full typography and viewport spec. The first page is shown before the rest of the chapter is laid out. Idle time continues the cache. A spec change drops it.
- Page turns, library, settings, and the reading chrome work under every `input` value. The reading chrome shows the chapter page, the position in the book, the title, and the battery. Buttons perform every one of those actions. Touch adds zones: one page-turn direction uses the whole page; both directions use the left third versus the rest, swapped for RTL. The center third opens the menu. Edge swipes are back, home, and menu. Hit targets grow sideways so they stay out of the page-turn rows.
- The reader presenter sets the damage mode. Page turns use `fast`, then one clearing refresh on a cadence the user can set (1, 5, 10, 15, 30, or never; default 15). Images and anti-aliased text use `quality`. `full` is the flash that clears residue. One update paints the page, including gray. The phase 2 desktop hook keeps its area budget.
- Pages, covers, and highlights render as gray8, then dither to `gray_levels` in the damage buffer. Ordered dither for flat color. Error diffusion for images and covers. A cover thumbnail is made at the size it will be drawn. A highlight is a CFI range. A broken file shows a failure a button can leave.
- Color is the color-plane phase.

Done when the emulator can boot two profiles, button-only gray and touch-and-stylus color, and the same shell binary opens a local EPUB and a local PDF on both: the first page appears before the chapter cache finishes, a button turns the page and leaves a broken file, and a font change keeps the CFI and the highlight range.

### 5. GPU offload

The local reader is already drawing pages on the CPU. This phase moves that raster off the CPU when a GPU is present. The Weston desktop stays on pixman. Window compositing is cheap next to page rendering, and the phase 2 damage hook stays the desktop writer.

- With no GPU, pixman and CPU raster stay the path from phase 4.
- When a GPU is probed, Mesa's driver for that GPU rasterizes page images, dither, and ink into the same damage buffer. Panfrost is the Mali driver. Other GPUs use their own Mesa driver. llvmpipe is not required for the no-GPU path.
- The damage protocol stays gray8. Apps still do not open the panel. A missing GPU does not change the session or the reader.

Done when a GPU-less emulator still matches phase 4, and a guest whose probe reports a GPU renders a page through that GPU while the panel still updates through damage modes.

### 6. Shelf library

- `docker compose up` starts shelf.
- A user, a library folder, and a scan.
- Metadata from Open Library only. A file whose metadata lookup fails still appears, and the scan records that the result is degraded.
- OPDS catalog and an authenticated file download.
- Admin UI to see the library.

Done when a book dropped in the folder appears in OPDS and downloads as the original file, including a file whose metadata lookup failed.

### 7. Progress sync

- Protocol in `protocol/` for position, bookmark, and highlight. The EPUB position is the CFI. The highlight is a CFI range plus a stable id.
- The shell writes a delete to the outbox before it removes the local row. The outbox write is crash-safe. Wi-Fi comes up to drain the outbox, then goes down.
- Field-level `updated_at` merge on the server.

Done when two emulator instances, or one emulator and the admin UI, share a position and a highlight for one user after an offline edit on each side, and an offline delete still arrives.

### 8. Stream, download, and read-along playback

- The shell pins a book for offline reading by default.
- Audio streams with HTTP range requests or downloads when pinned.
- The reader consumes EPUB 3 Media Overlays, including files produced by Storyteller. Sentence highlight and the audio clock follow Storyteller's reader, as listed under references.
- One CFI drives the page and the audio clock.

Done when a pre-aligned book highlights the current sentence during playback in the emulator, and a pinned book still opens with the radio off.

### 9. Alignment job

- Shelf accepts an ebook plus an audio file and queues a forced-alignment job.
- The job writes EPUB 3 Media Overlays back into the library, following `@smoores/epub` as listed under references.
- The device never runs alignment.

Done when a book and a matching audio file become a read-along file that phase 8 already knows how to play.

### 10. Acquisition

One UI for "I want this book" through "it is in the library".

- Wanted list.
- Sources the operator configures: drop folder, remote OPDS, or an external download client they already run.
- Import lands in the phase 6 library and uses the phase 6 metadata path.
- Format preference is EPUB, then PDF, plus an optional audio file. That is the whole quality model.

Done when a wanted book from a configured source imports once, gets metadata, and shows up on the device through the existing catalog sync.

### 11. Color plane

Covers, highlights, and pen ink exist as gray8 by this point. This phase is when they gain color.

- A damage message may carry a color plane beside gray8.
- Covers, highlight marks, and pen ink use `kaleido` or `full` from the profile. The highlight's stored anchor stays the CFI range. Body text stays gray and dithered to `gray_levels`.
- A gray profile writes gray8 only.

Done when a color profile shows a cover or a highlight in color on `:8006`, and a gray profile stays gray8.

### 12. Notes and frontlight

- Pen notebooks sync as annotations when `input` includes stylus. Color ink uses the phase 11 plane.
- Frontlight control appears only when `frontlight` is yes.

Done when a stylus profile round-trips a stroke through shelf, and a gray profile never offers the ink control.

## Deferred

Leave these out until a later decision changes this file: comics, text-to-speech, achievements, social reading, Hardcover and Readwise export, OIDC, a hosted multi-tenant service, shipping KOReader (or any third-party reader) as the built-in reader/notes session, and any panel-specific fork of the reader UI.

Sleep that keeps the last finished frame on the panel, and a wake that does not flash it, wait for a board port. They are not a phase in this list. Frontlight stays in phase 12, and only when the profile says the light is present.
