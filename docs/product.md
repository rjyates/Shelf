# oseink product decisions

Read this after the emulator exists. Do not implement it before then.

The emulator is GitHub issues 1 through 8 on [rjyates/oseink](https://github.com/rjyates/oseink/issues). That work boots a Linux image, hot-reloads the shell, and simulates the panel. This document is the next body of work.

## Two products

oseink is a device OS plus a self-hosted service. They share a protocol and stay in this repo.

| Piece | Path | Runs on | Job |
| --- | --- | --- | --- |
| Emulator | `emulator/` | Dev machine | Boot the image, hot-reload the shell, simulate the panel |
| Distro | `distro/` | Device and emulator | Kernel, rootfs, panel driver, capability probe |
| Shell | `shell/` | Device and emulator | Library, reader, audio, local cache, sync client |
| Shelf | `shelf/` | A server the user runs | Catalog, files, progress, alignment jobs, acquisition |
| Protocol | `protocol/` | Both | Request and response types for catalog, files, and sync |

The shell speaks the damage protocol from issue 7. Shelf never learns waveform timings, refresh modes, or panel product names.

## What we take from the references

[Storyteller](https://github.com/smoores-dev/storyteller) is the read-along model: an ebook and a narration aligned so text and audio share one position. Alignment is a server job. The device only plays the result. Interchange format is EPUB 3 Media Overlays. Import Storyteller output. Do not embed Storyteller's web or mobile apps.

[BookOrbit](https://github.com/bookorbit/bookorbit) is the library model: a Docker service the user hosts, progress and highlights that follow the reader, and books that can stream or download. One book has one position across reading and listening. Ship OPDS so other readers can browse the catalog. Skip, until someone asks, the achievement system, the phone apps, and the long list of metadata providers.

[Cinephage](https://github.com/MoldyTaint/Cinephage) is the shape of the acquisition service: one process, one database, one web UI, from a request through import into the library. Books only. Shelf does not manage video, subtitles, or live TV, and it does not ship indexer definitions or books. The operator points it at files they already have, a drop folder, an OPDS server, or a download client they already run.

## Locked technical choices

- Device image: Buildroot. The emulator guest is x86_64 under KVM. Hardware images are a later board port of the same userspace.
- Device storage: SQLite for the library cache, the outbox, and positions. Book files live on the local filesystem.
- Shelf deploy: one Docker image, one container, SQLite, book files on a local disk. Compose is the install path, same expectation as BookOrbit and Cinephage.
- Shelf stack: TypeScript on Node 22. The admin UI is served by that same process. Alignment tools run as subprocesses (ffmpeg and a forced-alignment binary), not as a second service.
- Shelf storage warning: library paths, the SQLite file, and import folders stay on a local filesystem. Network mounts are unsupported for the same reason BookOrbit refuses them.
- Accounts: local users first. OIDC is a later shelf setting. Progress is per user.
- Identity of a book: a library id plus a content hash of the canonical file, so a rename does not fork progress.
- Position: EPUB CFI for EPUB, page index for PDF, chapter plus milliseconds for audio. For a read-along book the CFI is the position and the audio clock is derived from the alignment map.
- Conflict rule: each field has its own `updated_at`. A highlight from one device and a position from another both stick. The device keeps an outbox and is authoritative while offline.
- Radio: Wi-Fi comes up to sync or to fetch a file, then goes down. The shell stays usable with no server configured.
- Engines: MuPDF for PDF. An existing EPUB engine (crengine or the equivalent already proven on e-ink) for EPUB. Do not write a document parser.

## Display capabilities

The distro probes the hardware once at boot and publishes a capability profile. The shell branches on that profile. It does not contain a table of Kindle, Kobo, or reMarkable models.

```text
color: gray | kaleido | full
gray_levels: 2 | 16
input: buttons | touch | stylus | touch_and_stylus
frontlight: yes | no
width, height, dpi
```

Rules:

- Every screen works with buttons alone. Touch and stylus add gestures. They are never required to turn a page, open a book, or change a setting.
- Body text is rendered in gray and dithered to `gray_levels`, including on color panels. Color e-ink is reserved for covers, highlight marks, and pen ink.
- `kaleido` and `full` use different color paths. Kaleido is a filter overlay. Full color is a color particle panel. The shell treats them as different profile values.
- Stylus present: store ink as vector strokes on that book and sync them as annotations. Stylus absent: hide ink tools. Touch can still create a highlight from a selection. Buttons can set a bookmark.
- Frontlight absent: hide the light control.
- The emulator's panel simulator must accept this same profile so a gray button-only device and a color touch-and-stylus device can both be tested before hardware exists.

## Phases

Work these in order. A phase can start when the previous one's "done when" is true. Shelf phases can be developed on the host against the protocol. Shell phases use the emulator.

### 1. Local reader

No shelf yet.

- Publish the capability profile from the distro and from the emulator.
- Shell opens a local folder of EPUBs and PDFs.
- Page turns, library, and settings work under every `input` value.
- Color and gray profiles change cover and highlight rendering and leave body text gray.

Done when the emulator can boot two profiles, button-only gray and touch-and-stylus color, and the same shell binary is usable on both.

### 2. Shelf library

- `docker compose up` starts shelf.
- A user, a library folder, and a scan.
- Metadata from Open Library only.
- OPDS catalog and an authenticated file download.
- Admin UI to see the library.

Done when a book dropped in the folder appears in OPDS and downloads as the original file.

### 3. Progress sync

- Protocol in `protocol/` for position, bookmark, and highlight.
- Shell outbox drains when the radio is up.
- Field-level `updated_at` merge on the server.

Done when two emulator instances, or one emulator and the admin UI, share a position and a highlight for one user after an offline edit on each side.

### 4. Stream, download, and read-along playback

- The shell pins a book for offline reading by default.
- Audio streams with HTTP range requests or downloads when pinned.
- The reader consumes EPUB 3 Media Overlays, including files produced by Storyteller.
- One CFI drives the page and the audio clock.

Done when a pre-aligned book highlights the current sentence during playback in the emulator, and a pinned book still opens with the radio off.

### 5. Alignment job

- Shelf accepts an ebook plus an audio file and queues a forced-alignment job.
- The job writes EPUB 3 Media Overlays back into the library.
- The device never runs alignment.

Done when a book and a matching audio file become a read-along file that phase 4 already knows how to play.

### 6. Acquisition

One UI for "I want this book" through "it is in the library".

- Wanted list.
- Sources the operator configures: drop folder, remote OPDS, or an external download client they already run.
- Import lands in the phase 2 library and uses the phase 2 metadata path.
- Format preference is EPUB, then PDF, plus an optional audio file. That is the whole quality model.

Done when a wanted book from a configured source imports once, gets metadata, and shows up on the device through the existing catalog sync.

### 7. Notes and color polish

- Pen notebooks sync as annotations when `input` includes stylus.
- Covers and highlight colors honor `kaleido` and `full`.
- Frontlight control appears only when `frontlight` is yes.

Done when a stylus profile round-trips a stroke through shelf, and a gray profile never offers that control.

## Deferred

Leave these out until a later decision changes this file: comics, text-to-speech, achievements, social reading, Hardcover and Readwise export, OIDC, a hosted multi-tenant service, KOReader compatibility, and any panel-specific fork of the shell.
