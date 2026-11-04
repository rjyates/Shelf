# SuperNote Nomad stylus & live ink (Shelf)

Device used for all physical smoke: `SN078F10019410` · Supernote Nomad · Android 11 /
SDK 30 · build `eng.supern.20261019.184438`. Evidence: REA + `adb` / `getevent`.

**Scope today:** accelerated live ink is **SuperNote Nomad only**. Physical
smoke and OEM refresh knobs were proven on one device
(`SN078F10019410`). Other Android tablets (LCD or e-ink) get Flutter Canvas
via `noop` / `InkOverlay` — usable strokes, not Nomad tip feel.

**Shipped Nomad path (2026-11-04, ~0.15–0.2s tip lag):** translucent
`InkHostView` + `ShelfEink` SPEED screen mode + A2 host update mode + tip lead.
Code: `EinkRefreshPlugin.kt`, `InkHostView.kt`, `ShelfEink.kt`, `eink_refresh.dart`.
Commit family: `feat/nomad-ink-latency` / “lock Shelf Nomad ink overlay…”.

Read **Do not retread** before changing ink. Older sections are historical.

### Roadmap (drawing)

1. **Generic Android tablet drawing** — one portable mid-stroke path (Surface /
   Flutter) that works on any stylus Android tablet without OEM refresh APIs.
   Nomad keeps its SPEED+A2 backend behind the same `EinkRefresh` contract.
2. **Golden tablets** — after the generic path is solid, deepen a few devices
   that are open enough to hack (framework / EPD / binder access). Nomad is the
   first golden; candidates later (e.g. Onyx `EpdController`) only when we can
   smoke them the way we did Nomad. Do not invent backends from datasheets alone.

---

## Current product behavior

1. Probe Android for `getSystemService("eink")` or features `android.software.eink` /
   `hteink` → backend id `ratta_pw` (means “Shelf overlay on Ratta device”, not
   Notes PWCore).
2. Ink mode on: attach translucent `InkHostView` over the reader content rect;
   `EinkManager.setScreenMode(SPEED=2)`; `View.setEinkUpdateMode(A2)` on the host;
   harden WebView `user-select`; consume stylus in `MainActivity` (fingers still
   page-turn).
3. Mid-stroke: draw into a **transparent** host bitmap → one coalesced
   `lockCanvas`/`present` per MotionEvent; tip lead (~28 ms velocity, max 14 px).
4. Pen-up: emit stroke on `shelf.eink/strokes` for persistence; **no** soft refresh.
5. Ink mode off: restore screen mode, soft DU once, remove host; Flutter paints
   committed strokes.

LCD / `noop`: Flutter `InkOverlay` global pointer route + Canvas (no native host).

### Portability (honest)

| Piece | Scope today |
| --- | --- |
| Flutter `InkOverlay` + stroke math (`ink_pipeline`) | Any Android / LCD `noop` |
| Probe → `ratta_pw` + SPEED/A2 + tip lead | **Nomad / Ratta only** (proven) |
| `InkHostView` Surface overlay without SPEED/A2 | Built, not productized as “generic tablet” yet |

Probe matching `eink` / `hteink` on a non-Nomad device may attach the overlay
with no-op refresh → lag like plain Surface; that is **not** a supported
backend. Treat non-Nomad as `noop` until the generic tablet path lands, then
add golden backends one device at a time.

### What Notes does (why we do not clone it)

Notes tip feel is `librecgnition.so` `ThreadUpdateEpdc` → `HTEINK_IOC_DISPAREA` on
`/dev/ebc` (direct overlay FB), plus HandWrite / SF STARTPW / PWCore. That is
privileged OEM stack, not a public SDK. Shelf approximates with framework SPEED+A2
on **our** Surface. Raw DISPAREA / shipping Notes `.so` is out of product scope.

---

## Do not retread (hard-won)

| Do not | Why | Symptom we saw |
| --- | --- | --- |
| Mid-stroke `EinkManager.screenRefresh` | Queues panel GC | ~0.5s+ tip lag |
| Soft refresh on every pen-up | Fights SPEED/A2 | Sticky tip, worse feel |
| `enableFullUiAuto(false)` without working EMR | Freezes UI partials | Ink invisible until mode exit |
| Notes HandWrite binder as live paint | Reply `realTimeHandWriting` ≠ our pixels | No mid-stroke ink |
| `PWCoreCtrl` / SF Communicate from sideload | SecurityException / no paint | Dead end |
| FastPW `postRectForPw` + host bmp chase | Plateau ~0.5s; easy to blank page | White page / flash |
| `SYNC_PW` mid-stroke | Bad waveform for our host | Flash + white page |
| Seed WebView / PixelCopy into host bmp | Opaque page cover | White page when drawing |
| PixelCopy Flutter SurfaceView | Unsafe | SIGBUS |
| `deinitForPw` on exit | Firmware bug | Scudo abort |
| Present every history sample | UI thread flood | ANR, 8–21s/event |
| Live `initForPw`+refresh from `diagnose`/`ensureReady` | Hot path | SIGSEGV with reader open |
| Mid-stroke Flutter Canvas on Nomad accelerated | Panel ignores Flutter damage | Ink only after mode exit |
| `setState` on every native stroke while ink mode on | Flutter invalidate | Full GC flash |
| Reload stroke geometry from server after save | EMA/finalize morph | Path changes after lift |
| Cover EMR (if ever) with opaque Surface | Z-order | Overlay invisible |

### Safe patterns (keep)

- One `present` per MotionEvent after coalescing dirty rects.
- Transparent host bitmap; ink only.
- Diagnose passive (probe/flags only); live refresh only in begin/end ink.
- Consume stylus only; never consume finger.
- Persist via `nativeStrokes` without rebuilding the Flutter layer mid-session.
- Clear native ink layer on page turn (`clearInkLayer`).
- Temurin/Java 21 for Flutter Android builds when host JDK is 25+.
- Device login: wait for ≥3 EditTexts (Nomad first frame is slow).

---

## Input device (portable baseline)

EMR appears as **`Wacom-pen`** (`/dev/input/event7`, vendor `0x2d1f`). Cooked
`InputDevice` sources stylus+touchscreen; Flutter `PointerDeviceKind.stylus` /
`invertedStylus` with pressure/tilt. No Ratta SDK required for sampling.

Features: `android.software.eink`, `android.software.hteink` (not
`android.hardware.faketouch.stylus`).

## Proprietary (do not ship / do not depend)

- `com.ratta.drawpath` + `librecgnition.so` / `librPainter.so` / `DrawService`
- Notes HandWrite `service_myservice` / `superNoteNote` protocol
- `ThreadSampleEmr`, OpenCV `drawLineLive`, Wacom/Lamy LUT catalogs
- Raw `/dev/ebc` as **primary** product path (experimental O5 only; still OEM-specific)

Binder `eink` / `hteink` / `opt_service` = **refresh** control, not stroke APIs.
`sn-plugin-lib` handwriting APIs are for Notes plugin host only.

## Portable stroke math (from REA ideas, reimplemented)

In `mobile/lib/ink_pipeline.dart`: smoothstep, pressure→width, thin/EMA,
`tiltWidthFactor` (procreate-style). Do not link Notes binaries.

## Deferred

| ID | Item | Notes |
| --- | --- | --- |
| G1 | Generic Android tablet mid-stroke drawing | Portable path before more OEM backends |
| G2 | Golden-tablet backends | Per device that we can physically hack; Nomad first |
| O5 | Shelf `HTEINK_IOC_DISPAREA` wrapper | **Experimental** behind `debug.shelf.ink.ebc=1` (see below) |
| O10 | Azimuth×brush chisel | Optional feel |
| O11 | Ink marks UI / clear-all | Product |
| O12 | Stable PDF/fixed-layout anchors | After PDF reader |
| O13 | Per-vendor refresh settings | Power users, after G2 |
| Onyx (example golden) | `EpdController` DW | Only with device in hand + smoke; same `EinkRefresh` contract |

---

## Historical experiments (archive)

**2026-11-03 O1:** Framework PW callable; `postRectForPw` no-op without host bmp;
diagnose+init SIGSEGV; Flutter invalidate mid-stroke ~0.5s + select issues.

**2026-11-03–04 host-bmp + HandWrite + FastPW:** Live Surface present helped;
FastPW/SYNC_PW/`enableFullUiAuto` caused white page or no gain past ~0.5s.

**2026-11-04 EMR-only (no Surface):** Ink only after mode exit; abandoned.

**2026-11-04 locked path:** Surface + SPEED + A2 + tip lead → ~0.15–0.2s tip lag.

**2026-11-04 O5 ebc DISPAREA experiment:** Shelf-owned `/dev/ebc` path (not Notes
`.so`). Recovered from `libeinkutils.so` + live Nomad probe + app smoke:

| Item | Value |
| --- | --- |
| Node | `/dev/ebc` world-`rw`, SELinux `rga_device` — shell + app open OK on this build |
| `HTEINK_IOC_GETINFO` | `0x48545201` — buffer must be **≥256 B** (16 B stack → `stack corruption` abort) |
| Geometry | Panel FB **1872×1404 landscape**; app UI **1404×1872 portrait** → map 90° CW |
| `HTEINK_IOC_DISPAREA` | `0x48545701` → `{l,t,r,b,offset,mode}`; mode `4`≈A2-ish, `7` default |
| Code | `shelf_ebc.cpp` / `ShelfEbc.kt`; default **off** |

Enable (Nomad USB):

```bash
adb shell setprop debug.shelf.ink.ebc 1
adb shell setprop debug.shelf.ink.ebc_only 1   # optional: no Surface present
adb shell setprop debug.shelf.ink.ebc_smoke 1  # optional: synthetic stroke on ink-enter
# toggle ink;: adb logcat --pid=$(adb shell pidof app.shelf.shelf_mobile) | grep ShelfEbc
```

Disable: `setprop … 0`. Locked SPEED+A2 path stays default.

**Lessons this session**

| Do not | Why |
| --- | --- |
| Undersized GETINFO stack buffer | SIGABRT `stack corruption detected` on launch |
| mmap `/dev/ebc` in plugin `diagnose`/probe on UI thread | ~0.5s+ stall / skipped frames; app loses focus |
| Assume screen coords == panel FB coords | Portrait UI vs landscape EPD; need rotation |
| `adb shell sendevent` EMR for smoke | SELinux denies untrusted/shell write to input |
| Trust ioctl out-params for logging DISPAREA rect | Driver may clobber the arg struct; log before ioctl |

**Risks still open:** shared panel FB vs SF; UI-thread `getPixels`+ioctl on large dirty
rects; tip-lag vs locked path not yet A/B’d with a real stylus. Present worker
still deferred.
