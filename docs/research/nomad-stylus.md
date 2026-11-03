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

## Unknowns (callability still open)

- Whether a **sideloaded** APK can invoke `EinkPWCoreController` / `EinkManager` (hiddenapi BLOCKED flags on the interface; binder `getSystemService("eink")` may still work).
- Whether PW mode needs a Ratta View/Surface vs posting rects over Flutter’s surface.
- Exact waveform ints for Nomad (defaults 0/1 until device smoke).
- MotionEvent vs Notes latency on Nomad (on-device probe).
