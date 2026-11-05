# Shelf e-ink Android — reference

## Architecture under test

```
Nomad app (Flutter)
  ├─ Dart HttpClient → http://127.0.0.1:8080/api/...   (adb reverse)
  │    downloadEpub → EpubStore → flutter_readium open
  └─ Readium native view (+ optional InkHostView overlay)
Host
  └─ shelf-server SHELF_LISTEN=127.0.0.1:8080  SHELF_DATA=./data
```

Mobile does **not** embed Foliate WebView. Book Orbit Foliate remains the web
`/reader` host only.

## Supernote specifics

- OS: Chauvet (Android 11-class). No Play Store; ADB sideload only.
- Serial looks like `SN…`. `adb devices -l` shows `product:Supernote model:Supernote_Nomad`.
- Some USB hubs never enumerate the device; prefer direct USB-C.
- Soft keyboard is large on Nomad — always dismiss before tapping another field or the primary button.

## Flutter / SDK pins

- Flutter: `.flutter-version` at repo root.
- Typical local layouts: `$HOME/flutter/<version>/bin` or whatever `flutter --version` resolves when PATH is correct.
- `ANDROID_HOME` / `ANDROID_SDK_ROOT`: e.g. `$HOME/Android/sdk`.

## Why merged cleartext matters

`targetSdk` is high enough that platform default **blocks** cleartext. Debug manifest merge must show:

```xml
android:usesCleartextTraffic="true"
```

Inspect: `mobile/build/app/intermediates/merged_manifests/debug/processDebugManifest/AndroidManifest.xml`

Needed so EPUB download over `http://127.0.0.1:8080` works in debug.

## UI automation tips

- Dump hierarchy: `adb shell uiautomator dump /sdcard/shelf-ui.xml` then `adb pull`.
- EditTexts and Buttons expose `bounds="[x1,y1][x2,y2]"`.
- `adb shell input text` rejects some shell-splitting URLs if the command is built via naive word-splitting — use Python `subprocess` list argv.
- Colons in URLs are fine when passed as a single argv element.
- After install, session is memory-only — every cold start needs login again.
- `uiautomator dump` can return null on first frames — retry; dismiss IME with `KEYCODE_BACK` between fields (not while dump is stale); `pm clear` if fields are corrupted from a bad automation pass.

## Screenshots

```bash
adb exec-out screencap -p > /tmp/shelf-nomad/shot.png
```

## Nomad live ink (locked path)

Accelerated drawing is **Nomad-only** until milestone 13 (generic tablet → golden
OEM). Do **not** revive Notes HandWrite / PWCore / mid-stroke `screenRefresh` /
WebView seeding / PixelCopy / `deinitForPw`. Full ledger:
[`docs/research/nomad-stylus.md`](../../../docs/research/nomad-stylus.md).

Shipped Nomad feel (~0.15–0.2s tip lag): translucent `InkHostView` +
`EinkManager.setScreenMode(SPEED)` + `View.setEinkUpdateMode(A2)` + tip lead.
Logcat tag `ShelfEink`. Ink mode AppBar control:
`content-desc="Ink mode (stylus, fast refresh)"`.

Do not chase mid-stroke “fill-in” with DU polish / dropping SPEED — smoke showed
no win or worse tip feel; ledger in `nomad-stylus.md`.

Java for Flutter builds: prefer JDK 21 when the host default is 25+.

### Experimental DISPAREA (`debug.shelf.ink.ebc`)

```bash
adb shell setprop debug.shelf.ink.ebc 1          # Surface + ebc
adb shell setprop debug.shelf.ink.ebc_only 1     # ebc only (no Surface present)
adb shell setprop debug.shelf.ink.ebc_smoke 1    # synthetic stroke on ink-enter
adb logcat --pid="$(adb shell pidof app.shelf.shelf_mobile)" | grep ShelfEbc
```

Panel FB is landscape vs portrait UI — native maps 90° CW. Default remains
Surface+SPEED+A2. Details: `docs/research/nomad-stylus.md`.

## Related docs

- `mobile/README.md` — product shape, HTTPS phone path
- `AGENTS.md` — fmt/analyze/test gates
- `docs/parity.md` — milestone status
- `docs/research/nomad-stylus.md` — ink portability + do-not-retread
