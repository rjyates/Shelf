# Shelf e-ink Android — reference

## Architecture under test

```
Nomad app (Flutter)
  ├─ Dart HttpClient → http://127.0.0.1:8080/api/...   (adb reverse)
  └─ WebView → /reader?id=… (+ Bearer, seeded shelf_session cookie)
       └─ Foliate XHR → /file, /location (same origin, cookie)
Host
  └─ shelf-server SHELF_LISTEN=127.0.0.1:8080  SHELF_DATA=./data
```

Cookie seed lives in `mobile/lib/reader.dart` (`WebViewCookieManager` + `shelf_session`). Clear cookies only immediately before seed; dispose still clears on leave.

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

WebView sandboxed process logs `NetworkSecurityConfig: … platform default` either way; the application flag still controls cleartext allowance.

## UI automation tips

- Dump hierarchy: `adb shell uiautomator dump /sdcard/shelf-ui.xml` then `adb pull`.
- EditTexts and Buttons expose `bounds="[x1,y1][x2,y2]"`.
- `adb shell input text` rejects some shell-splitting URLs if the command is built via naive word-splitting — use Python `subprocess` list argv.
- Colons in URLs are fine when passed as a single argv element.
- After install, session is memory-only — every cold start needs login again.

## Screenshots

```bash
adb exec-out screencap -p > /tmp/shelf-nomad/shot.png
```

WebView content often has **empty** accessibility text dumps; screenshots are the ground truth for reader chrome.

## Related docs

- `mobile/README.md` — product shape, HTTPS phone path
- `AGENTS.md` — fmt/analyze/test gates
- `docs/parity.md` — milestone status (mobile online reader vs display-profile onboarding)
