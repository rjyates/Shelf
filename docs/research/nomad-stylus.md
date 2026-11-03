# SuperNote Nomad stylus exposure (2026-11-02)

Device: `SN078F10019410` · model `Supernote Nomad` · Android 11 / SDK 30 · build `eng.supern.20261019.184438`.

Evidence gathered with REA (`list_adb_*`, `pull_adb_package`, JADX class/method inspect) plus `getevent` / `dumpsys input` on the connected tablet.

## What third-party apps (including Shelf) get

The EMR digitizer is exposed as a normal Android input device named **`Wacom-pen`** (`/dev/input/event7`, vendor `0x2d1f`).

Cooked Android `InputDevice` sources `0x00005002` (stylus + touchscreen class) with:

| Axis | Raw | Cooked |
| --- | --- | --- |
| X / Y | 0…15819 / 0…11864 | mapped to display 1404×1872 |
| Pressure | 0…4095 | 0…1 |
| Tilt X/Y | ±9000 (res 5730) | `TILT` 0…π/2 |
| Hover | `ABS_Z` −255…0 | via distance when reported |
| Buttons | `BTN_TOUCH`, `BTN_STYLUS`, `BTN_STYLUS2`, `BTN_TOOL_RUBBER` | tool type stylus / eraser |

Flutter maps this to `PointerDeviceKind.stylus` / `invertedStylus` with `pressure`, `tilt`, and stylus buttons. **No special Ratta SDK is required for basic ink.**

`pm list features` declares `android.software.eink` and `android.software.hteink`, but **not** `android.hardware.faketouch.stylus` or any other stylus feature flag.

## What is proprietary (do not depend on for Shelf)

`com.ratta.drawpath` (`DrawService`, action `com.ratta.DrawService`) is the system handwriting accelerator:

- Native methods `startDraw(boolean, String)` and JNI `MyNativeBridge.nativeTransactCompat` load `librecgnition.so` (plus `librPainter.so`).
- On Nomad it passes log path `/data/vendor/eink/logd/realTimeHandWriting`.
- Broadcasts include `android.intent.action.drawAppStart`, `android.intent.action.penup`, and pen-button double-click actions — system-user only.
- This path is for Ratta’s own notes/docs stack (C++ / MyScript-class latency), not a supported API for sideloaded apps. Community reports match: Android Ink / Flutter alone is higher latency than Notes.

Binder services `eink` (`android.os.IEinkManager`), `hteink`, and `opt_service` are **display/refresh** control, not a pen stroke API. They are the right place for a **refresh adapter**, not for sampling ink.

`com.ratta.supernote.inkhub` is the InkHub marketplace app, not an input SDK.

SuperNote’s published **plugin** APIs (`canHandwrite`, `getPenInfo` in `sn-plugin-lib`) apply inside the Notes/Document plugin host — not to arbitrary Flutter APKs. No `getPenInfo` class name appears in the pulled PluginHost APK inventory under that string.

## Shelf plan

1. Capture stylus via Flutter `Listener` (`PointerDeviceKind.stylus` / `invertedStylus` only → palm rejection by kind).
2. Overlay strokes on the e-ink Readium reader; sync as Shelf annotations (`style: "ink"`).
3. Shape strokes with a portable pipeline (`mobile/lib/ink_pipeline.dart`) derived from REA math — not Ratta libs.
4. Abstract partial refresh behind one ink-session API; vendor backends (Ratta PW, Onyx EPD, noop) — prefer framework reflection over raw ioctl.

## Portable math derived from `librecgnition.so` (REA)

Treat decompiled formulas like a Stack Overflow answer — reimplement, do not ship their binary.

| Symbol (observed) | Formula we use | Shelf location |
| --- | --- | --- |
| `smoothstep(edge0,edge1,x)` | clamp `t=(x−e0)/(e1−e0)` then `t²(3−2t)` | `inkSmoothstep` |
| `pressureMapping2Width(scale,p,max,min)` | linear map clamped pressure span → width (+floor) | `widthFromPressure` on Android’s already-normalized `0…1` pressure |
| `reduceSamplePoints` / contour smooth | min-distance thin + one-pass EMA (KISS stand-in for Gaussian) | `thinInkPoints` / `smoothInkPoints` / `finalizeInkPoints` |
| `procreateStyleWidth` (tilt/azimuth) | **done in M7 PR** | `tiltWidthFactor` + `InkPoint.orientation`; Lamy LUT skipped |

**Partial refresh is not skipped — it gets its own adapter** (see below). Do not open `/dev/ebc` from Flutter first; prefer the framework PW APIs REA found.

## Deferred-item disposition (REA complete enough to plan)

| Item | Ship in Shelf? | Milestone | Why |
| --- | --- | --- | --- |
| Tilt / `procreateStyleWidth` | **Yes** (portable math) | **M7 PR** | Decompiled: `factor = clamp(1 − 0.98·smoothstep(sin(tilt)), 0.1, 1) * baseWidth` (Lamy nested term omitted). Azimuth stored; brush-dir dot was log-only in their binary. |
| EPDC / PW / partial refresh | **Yes** (adapter, not ioctl clone) | **M8** (before or with PDF ink) | Prefer `EinkPWCoreController` / Onyx `EpdController`; raw `/dev/ebc` last resort. Probe callability first. |
| OEM `ThreadSampleEmr` | **No** | never | Privileged raw EMR loop (`BTN_TOOL_PEN`/`RUBBER`, G12/G14, system pen-button broadcasts). Sideloaded apps already get cooked `Wacom-pen` MotionEvents. |
| OpenCV `drawLineLive` / `librPainter` | **No** (as-is) | never as OpenCV | Tied to Notes framebuffer + EPDC. Keep Flutter `Canvas`; if laggy later, optional Dart isolate / Skia path — not OpenCV. |
| Wacom / Lamy type LUTs | **No catalog** | only if Nomad feel wrong | `normalizePressure` itself logs *“should not use any more”*; `getLamyWidth` is huge `logIndex_X1000` tables per `WalcomEmrType`/`LineWidthType`. Android already normalizes 0…1. At most one empirical curve later — not a pen SKU matrix. |

## What makes Notes feel “great” (observed in `librecgnition.so`)

Native stack (`librecgnition.so` ~1.4 MiB arm64, ~1496 dynamic exports; plus `librPainter.so`) is **not** a thin wrapper over `MotionEvent`. Strings/symbols show:

- `ThreadSampleEmr` — dedicated EMR sampler (`thread_sample_emr`, `getStylusIsSensed`, pen-type paths, hover points).
- `thread_update_epdc` / `EINK_IOC_GETINFO` / `HTEINK_IOC_MISCCTL` — **direct e-ink controller ioctls** while writing (partial refresh), not Flutter/Canvas.
- `thread_draw_path` / `drawLineLive` / OpenCV mats — live stroke rasterization off the UI thread.
- Pressure calibration per Wacom EMR generation (`wacomEmrType`, G12/G14, `ADJUST_SAMPLE_PRESSURE`, “lamy shold use new pressure”).
- Pen models (`FOUNTAIN_PEN`, `CALLIGRAPHY_PEN`, `MARK_PEN_*`, eraser via `BTN_TOOL_RUBBER`).
- JNI only exposes `DrawService.startDraw` and `MyNativeBridge.nativeTransactCompat` — not a public stroke API.

Hardware already separates **pen** (`Wacom-pen`) from **finger** (`pt_mt`, `cyttsp5_mt`). That is most of “palm rejection” for sideloaded apps: ignore touch while drawing with stylus. Notes’ remaining magic is privileged EMR + EPDC access inside a system service.

## Can Shelf reverse-engineer and ship that stack?

**Clone Notes’ native stack: no. Abstract partial refresh with OEM backends: yes — that is the intended path.**

| Approach | Feasible? | Why |
| --- | --- | --- |
| Ship/link `librecgnition` / copy Notes’ EMR+OpenCV stack | No | System service, not a public SDK |
| Raw `/dev/ebc` ioctls from Shelf | Last resort | `ThreadUpdateEpdc` opens `/dev/ebc` + `HTEINK_IOC_MISCCTL` (`CLERA_PW_RECT`); node is world-`rw` here but SELinux `rga_device` + buffer ownership; framework already wraps this |
| **Abstract partial refresh + vendor adapters** | **Yes** | Same product idea as Onyx `EpdController` DW/GU; Ratta exposes pen-write (PW) framework APIs |
| Standard Android stylus + Canvas (current M7) | Yes | Portable baseline until a refresh adapter is wired |

## Abstract e-ink refresh (REA-backed)

REA shows **three layers** on Nomad:

1. **Binder:** `eink` → `android.os.IEinkManager`; `hteink` → `hteink.IDeviceManagerService`; `opt_service` → `hteink.optimization.IOptService`. Features: `android.software.eink`, `android.software.hteink`.
2. **Framework PW API (preferred adapter target):** `libeinkpwcoreapi.jar` / `htfyview.jar` / `htfypw.jar` — `EinkPWCoreController` (`initForPw`, `postRectForPw`, `nativeAddPWRect`, `setEinkUpdateMode`, `screenRefresh`), `android.view.EinkPWInterface`, `htfyun.penwrite.ctrl.PWCoreCtrl`.
3. **Notes-only native:** `ThreadUpdateEpdc` → `/dev/ebc` + framebuffer mats. How Notes gets “ink now”; not the portable entry.

**Shelf contract (KISS):** `beginInk()` / `invalidate(rect)` / `endInk()` / `fullRefresh()` with backends `noop` | `ratta_pw` | `onyx_epd`. Portable part is the contract, not ioctl numbers. Onyx’s documented DW scribble mode is the same product problem with a different private API.

## M8 implementation note

Shelf now probes at runtime (`mobile/lib/eink_refresh.dart` + `EinkRefreshPlugin.kt`):

- `getSystemService("eink")` → `screenRefresh(boolean,int)`
- `EinkPWCoreController` → `initForPw` / `postRectForPw` / `deinitForPw`
- `View.setEinkUpdateMode(int,int)` when present on the Flutter view
- Onyx `EpdController` / `handwritingRepaint` when those classes resolve

All calls are best-effort; failures stay on Canvas-only ink.

## O1 Nomad callability (2026-11-03) — **pass**

Sideloaded debug APK on `SN078F10019410` against host `:8080` via `adb reverse`. Logcat tag `ShelfEink`.

| Check | Result |
| --- | --- |
| `probe` | `ratta_pw` (`mgr=true`, `pwClass=true`, `hteink=true`, `eink=true`) |
| `EinkManager.screenRefresh` | Live invoke OK on cold start; `endInk` → `callScreenRefresh full=true ok` |
| `EinkPWCoreController.initForPw` | Callable (hiddenapi allowed) but leaves `bitmap_width=0` on FlutterView |
| `FlutterView.setEinkUpdateMode(1,0)` | OK |
| `postRectForPw` | JNI `nativeAddPWRect` errors (`bitmap_width=0`) — silent no-op; **not** used for live ink until O3 |
| Live path after O1 follow-up | `setEinkUpdateModeWithView` + regional `View.invalidate` (no mid-stroke `screenRefresh` — that flashed the whole panel). `postRectForPw` still needs host bmp (O3). |
| Stroke feel (REA re-check) | Linear `pressureMapping2Width`; softer tilt; densify sparse MotionEvent gaps (fast circle→box); `calculateSmoothedSpeedFactor` curve for speed thinning |
| UI | Tooltip `Ink mode (stylus, fast refresh)` ⇒ Dart `isAccelerated` |

**Do not** live-invoke PW/refresh from a hot `diagnose`/`ensureReady` path: a second `initForPw`+`screenRefresh` while the Readium reader was open **SIGSEGV**’d the process (uncaught by try/catch). Diagnose is passive (method/class presence only); ink-session APIs remain the sole live callers.

Physical pen smoke (2026-11-03): stroke only appeared after lift because `postRectForPw` never refreshed; after lift, EMA `finalizeInkPoints` + reloading strokes from the server morphing the path. Fixed by screenRefresh fallback, thin-only finalize, and keeping local geometry after save. Annotation **Request failed** was an outdated `shelf-server` binary / missing `annotations` table on the local DB — rebuilt server + applied `schema.sql`.

## Live-ink latency (2026-11-03 follow-up)

Nomad Notes does **not** wait on Flutter frames. REA on `libeinkpwcoreapi` / `libeinkpwcorejni.so`:

1. `EinkPWCoreController.initForPw`
2. `setHostBmpOffsetXY(Bitmap, ox, oy)` — without this, `nativeAddPWRect` logs `bitmap_width=0` and no-ops
3. Draw the stroke into that host bitmap (Notes: OpenCV `drawLineLive`)
4. `postRectForPw(rect, …)` → `postEinkHostBmpRect` / `postEinkPWRectFastHL`

Shelf mirrors that: PixelCopy the Flutter page into a host `Bitmap` on ink enter, then each stylus move calls native `drawSegment` + `postRectForPw`. Mid-stroke Flutter `CustomPaint` is skipped while `hostLive` to cut the ~0.5–0.7s Canvas follow lag. Stroke math now uses a 5-sample speed deque, `getSpeedMapWidth` EMA, azimuth×brush, and `InkPoint` timestamps.

## Unknowns (still open after O1)

- Whether PW over Flutter’s surface matches Notes latency, or a Ratta View/Surface host is required for “live” feel (O3).
- Exact waveform ints for Nomad (defaults 0/1; revisit if partial refresh looks wrong — O2).
- MotionEvent stylus vs Notes latency on Nomad (needs physical pen drawing).

## Deferred optimizations (log — do not implement unless needed)

Ledger of intentional skips after M7/M7.5/M8. Revisit when Nomad (or other e-ink) smoke shows a concrete gap. Prefer the portable `EinkRefresh` / `ink_pipeline` contracts; do not revive OEM clones.

| ID | Item | Why skipped | When to revisit | Suggested approach |
| --- | --- | --- | --- | --- |
| O1 | Nomad callability smoke for M8 | **Done 2026-11-03** — see section above | — | — |
| O2 | Waveform / `setEinkUpdateMode` ints | **Done 2026-11-03** — ink mode=`2` (DU/A2-class; IME uses DU) | Revisit if GC ghosts | Tuned in `EinkRefreshPlugin` |
| O3 | Ratta PW host bitmap | **Done 2026-11-03** — `setHostBmpOffsetXY` + PixelCopy page + `drawSegment`/`postRectForPw` | If PixelCopy fails, falls back to View.invalidate | Nomad Notes path; not Flutter Canvas mid-stroke |
| O4 | Hidden-API / binder hardening | Reflection may be BLOCKED for sideloads | Probe always returns `noop` on Nomad | Prefer binder stubs already returned by `getSystemService("eink")`; avoid `/dev/ebc` |
| O5 | Raw `/dev/ebc` + `EINK_IOC_*` / `HTEINK_IOC_*` | Notes-native, SELinux fragile, non-portable | Only if O3–O4 fail and Nomad-only path is explicitly wanted | Last-resort Nomad adapter behind same `EinkRefresh` API |
| O6 | `ThreadSampleEmr` | Privileged raw EMR; MotionEvents already work | Never for Shelf core | Keep Flutter stylus path |
| O7 | OpenCV `drawLineLive` / `librPainter` | Tied to OEM buffer + refresh | Canvas lag *after* refresh is fixed | Dart isolate / Skia path — not OpenCV |
| O8 | Wacom G12/G14 / Lamy LUT catalogs | SKU matrix; `normalizePressure` obsolete in their binary | Nomad pressure feel wrong after tilt+smoothstep | One empirical curve, not a pen catalog |
| O9 | Nested `getLamyX1000Width` in tilt | Dropped from portable `procreateStyleWidth` | Calligraphy complain | Optional factor only if needed |
| O10 | Azimuth × brush-dir blend | Ratta logged it; width return unused it | Users want chisel tips | Optional; `orientation` already stored on `InkPoint` |
| O11 | Dedicated ink marks list / clear-all | Highlights sheet hides ink JSON | Users can’t manage strokes | Marks sheet “Ink” tab or filter |
| O12 | Stable page anchors for reflow EPUB | CFI page-scope is best-effort | PDF / fixed-layout ink | Page/spine ids on fixed layout |
| O13 | Per-vendor refresh settings UI | Adapter should stay invisible | Power users need mode toggle | Tiny e-ink settings, Shelf tokens, no motion |
| O14 | iOS / LCD refresh backends | No e-ink controller | N/A | Keep `noop` |
