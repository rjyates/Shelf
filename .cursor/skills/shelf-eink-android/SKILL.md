---
name: shelf-eink-android
description: >-
  Sideload and exercise Shelf's Flutter Android app on a local e-ink tablet
  (SuperNote Nomad) against a loopback Shelf server via adb reverse. Use when
  installing or testing the mobile app on physical Android/e-ink hardware,
  debugging Readium open / EPUB download over adb reverse, automating login on
  device, or continuing Nomad/device QA sessions.
---

# Shelf e-ink Android device testing

## When this applies

Local debug installs of `mobile/` on a USB Android e-ink device (typically Supernote Nomad) talking to Shelf on the host. Production HTTPS phone testing is different — see root README “Access from a phone”.

Read this skill before inventing a new install/login/reader path.

## Hard-won facts (do not rediscover)

1. **One mobile reader: Readium.** Read book downloads/caches the EPUB via Dart `HttpClient`, then opens `flutter_readium`. There is no Foliate WebView on mobile (Foliate is web-only).
2. **Debug/profile already allow cleartext** in `mobile/android/app/src/{debug,profile}/AndroidManifest.xml` (`usesCleartextTraffic=true`) so HTTP loopback downloads work. Release must stay HTTPS-only.
3. **Loopback to the host is `adb reverse`, not the LAN IP.** App accepts plain HTTP only for `localhost` / `127.0.0.1`. On device use `http://127.0.0.1:8080` after `adb reverse tcp:8080 tcp:8080`.
4. **USB often fails through docks.** Prefer a **data** cable into a **direct** Framework/laptop port. Nomad enumerates as Rockchip `Supernote Nomad` (`idVendor=2207`). Empty `adb devices` + no new `lsusb` row ⇒ cable/port/sideloading, not the app.
5. **Nomad sideloading:** Settings → Security & Privacy → **Sideloading** ON (accept disclaimer). First connect may prompt USB debugging — accept.
6. **Keyboard covers fields.** UIAutomator taps by absolute Y while the soft keyboard is open hit the keyboard, not the next field. **Dismiss with `KEYCODE_BACK` between fields.** Fill **username before server** if automation is flaky; prefer `input text` for ASCII after focus.
7. **Library AppBar** must keep `automaticallyImplyLeading: false` — back arrow is wrong there (sign-out only).
8. **Mobile UI is e-ink-first (light paper / black ink).** Do not restyle the phone app to the web journal dark palette (`.picasso.md` / `docs/design/v3.md` stay for web). Avoid mid-grays, shadows, splash, and motion on this surface.

## Prerequisites checklist

```
- [ ] Flutter version matches repo `.flutter-version` (export that SDK on PATH)
- [ ] ANDROID_HOME points at a local Android SDK
- [ ] Shelf server healthy on host :8080 (`curl -sS http://127.0.0.1:8080/api/v1/health`)
- [ ] Device listed: `adb devices -l` (model Supernote_Nomad)
- [ ] Sideloading enabled on device
- [ ] `adb reverse tcp:8080 tcp:8080` (re-run after every USB reconnect)
```

### Start local server (typical)

```bash
# from repo root; keep existing ./data if present
test -f web/orbit-reader/dist/index.html || npm run build --prefix web/orbit-reader
SHELF_DATA="$PWD/data" SHELF_LISTEN=127.0.0.1:8080 \
  cargo run --manifest-path server/Cargo.toml --release
```

### Credentials

Do **not** commit passwords. Use gitignored `mobile/device-test.env` (see `mobile/device-test.env.example`).

Create first account only on empty DB:

```bash
curl -sS -X POST http://127.0.0.1:8080/api/v1/setup \
  -H 'Content-Type: application/json' \
  -d "{\"username\":\"$SHELF_DEVICE_USER\",\"password\":\"$SHELF_DEVICE_PASSWORD\"}"
```

## Build / install / launch

```bash
cd mobile
export PATH="$HOME/flutter/$(cat ../.flutter-version)/bin:$PATH"   # adjust if SDK lives elsewhere
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/sdk}"
flutter pub get --enforce-lockfile
flutter build apk --debug
SERIAL=$(adb devices | awk '/\tdevice$/{print $1; exit}')
adb -s "$SERIAL" reverse tcp:8080 tcp:8080
adb -s "$SERIAL" install -r build/app/outputs/flutter-apk/app-debug.apk
adb -s "$SERIAL" shell am start -W -n app.shelf.shelf_mobile/.MainActivity
```

Package: `app.shelf.shelf_mobile` · Activity: `.MainActivity`.

Prefer the helper:

```bash
python3 .cursor/skills/shelf-eink-android/scripts/device_login.py
# options: --install  --open-first-book  --serial SN...
```

## Manual login values on device

| Field | Value |
| --- | --- |
| Server | `http://127.0.0.1:8080` |
| Username / password | from `mobile/device-test.env` |

## Diagnose reader failures

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| Read fails / cannot download EPUB | Reverse missing or cleartext blocked | Confirm debug cleartext + `adb reverse --list` shows `tcp:8080` |
| Catalogue works, Readium blank/error | Bad/missing local EPUB bytes or Readium open | Logcat `ShelfEink` / Readium; retry Save offline then Read |
| Login validation “Enter your username” after automation | Keyboard ate taps | Dismiss keyboard between fields; username first |
| `adb devices` empty | Cable/dock/sideloading | Direct port, data cable, Sideloading ON |
| Reverse was fine, then broke | USB re-enumeration | Re-run `adb reverse tcp:8080 tcp:8080` |

Useful probes:

```bash
adb reverse --list
# from device through reverse:
adb shell 'echo -e "GET /api/v1/health HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n" | toybox nc 127.0.0.1 8080'
adb logcat -d -t 200 | rg -i 'cleartext|NetworkSecurityConfig|WebView|ERR_'
```

## Verify after mobile changes

From `mobile/` (also in `AGENTS.md`):

```bash
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --fatal-infos
flutter test
```

Then rebuild/install if exercising on hardware.

## Out of scope

- Store signing / Play / App Store
- LAN HTTP to the host IP (app rejects non-localhost cleartext)
- Restyling Book Orbit Foliate chrome to the journal theme
- Assuming e-ink display-profile onboarding is implemented (still a later milestone; UI is already e-ink-oriented)

## Extra detail

- Automation and env file format: [scripts/device_login.py](scripts/device_login.py), [mobile/device-test.env.example](../../../mobile/device-test.env.example)
- Edge cases: [reference.md](reference.md)
