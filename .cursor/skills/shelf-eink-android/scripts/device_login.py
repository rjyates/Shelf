#!/usr/bin/env python3
"""Install (optional), reverse-tunnel, and sign into Shelf on a USB Android device.

Reads mobile/device-test.env (gitignored) or SHELF_DEVICE_* environment variables.
Requires: adb, a running Shelf on host :8080, sideloading enabled on device.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
MOBILE = REPO / "mobile"
ENV_FILE = MOBILE / "device-test.env"
APK = MOBILE / "build/app/outputs/flutter-apk/app-debug.apk"
PACKAGE = "app.shelf.shelf_mobile"
ACTIVITY = f"{PACKAGE}/.MainActivity"


def adb(serial: str, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    cmd = ["adb", "-s", serial, *args]
    proc = subprocess.run(cmd, check=False, capture_output=True, text=True)
    if check and proc.returncode != 0:
        raise SystemExit(
            f"adb failed ({proc.returncode}): {' '.join(cmd)}\n{proc.stderr or proc.stdout}"
        )
    return proc


def load_env() -> tuple[str, str, str]:
    vals = {
        "SHELF_DEVICE_SERVER": "http://127.0.0.1:8080",
        "SHELF_DEVICE_USER": "",
        "SHELF_DEVICE_PASSWORD": "",
    }
    if ENV_FILE.is_file():
        for line in ENV_FILE.read_text().splitlines():
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, _, value = line.partition("=")
            vals[key.strip()] = value.strip().strip('"').strip("'")
    import os

    for key in list(vals):
        if os.environ.get(key):
            vals[key] = os.environ[key]
    if not vals["SHELF_DEVICE_USER"] or not vals["SHELF_DEVICE_PASSWORD"]:
        raise SystemExit(
            f"Missing credentials. Copy {ENV_FILE.name}.example to {ENV_FILE.name} "
            "or export SHELF_DEVICE_USER and SHELF_DEVICE_PASSWORD."
        )
    return vals["SHELF_DEVICE_SERVER"], vals["SHELF_DEVICE_USER"], vals["SHELF_DEVICE_PASSWORD"]


def pick_serial(requested: str | None) -> str:
    out = subprocess.check_output(["adb", "devices", "-l"], text=True)
    rows = []
    for line in out.splitlines()[1:]:
        if "\tdevice" in line or re.search(r"\sdevice\s", line):
            serial = line.split()[0]
            rows.append(serial)
    if requested:
        if requested not in rows:
            raise SystemExit(f"Device {requested} not connected. Seen: {rows}")
        return requested
    if not rows:
        raise SystemExit(
            "No adb device. Enable Nomad Sideloading, use a data cable on a direct port, "
            "then re-plug until `adb devices` lists it."
        )
    return rows[0]


def dump_ui(serial: str) -> str:
    adb(serial, "shell", "uiautomator", "dump", "/sdcard/shelf-ui.xml")
    adb(serial, "pull", "/sdcard/shelf-ui.xml", "/tmp/shelf-device-ui.xml")
    return Path("/tmp/shelf-device-ui.xml").read_text(errors="replace")


def centers(xml: str, cls: str) -> list[tuple[int, int]]:
    found = []
    for m in re.finditer(
        rf'class="{re.escape(cls)}"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"',
        xml,
    ):
        x1, y1, x2, y2 = map(int, m.groups())
        found.append(((x1 + x2) // 2, (y1 + y2) // 2))
    return found


def tap(serial: str, x: int, y: int) -> None:
    adb(serial, "shell", "input", "tap", str(x), str(y))
    time.sleep(0.5)


def dismiss_keyboard(serial: str) -> None:
    adb(serial, "shell", "input", "keyevent", "KEYCODE_BACK")
    time.sleep(0.75)


def type_text(serial: str, text: str) -> None:
    # Single argv — do not shell-split URLs.
    adb(serial, "shell", "input", "text", text.replace(" ", "%s"))
    time.sleep(0.35)


def dismiss_display_profile(serial: str) -> None:
    """First-run e-ink onboarding blocks the login form until answered."""
    xml = dump_ui(serial)
    if "Are you using an e-ink display?" not in xml and "Yes, e-ink" not in xml:
        return
    for m in re.finditer(
        r'content-desc="Yes, e-ink"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"'
        r'|bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"[^>]*content-desc="Yes, e-ink"',
        xml,
    ):
        g = [int(x) for x in m.groups() if x]
        if len(g) == 4:
            tap(serial, (g[0] + g[2]) // 2, (g[1] + g[3]) // 2)
            time.sleep(1.2)
            return
    buttons = centers(xml, "android.widget.Button")
    if buttons:
        tap(serial, *buttons[0])
        time.sleep(1.2)


def fill_login(serial: str, server: str, user: str, password: str) -> None:
    dismiss_display_profile(serial)
    xml = dump_ui(serial)
    edits = centers(xml, "android.widget.EditText")
    if len(edits) < 3:
        raise SystemExit(f"Expected 3 login fields, found {len(edits)}. UI:\n{xml[:800]}")

    # Username first so a later keyboard cannot steal the tap.
    tap(serial, *edits[1])
    type_text(serial, user)
    dismiss_keyboard(serial)

    xml = dump_ui(serial)
    edits = centers(xml, "android.widget.EditText")
    tap(serial, *edits[0])
    type_text(serial, server)
    dismiss_keyboard(serial)

    xml = dump_ui(serial)
    edits = centers(xml, "android.widget.EditText")
    tap(serial, *edits[2])
    type_text(serial, password)
    dismiss_keyboard(serial)

    xml = dump_ui(serial)
    buttons = centers(xml, "android.widget.Button")
    if not buttons:
        raise SystemExit("No login button found after filling the form.")
    tap(serial, *buttons[0])
    time.sleep(4)


def open_first_book(serial: str) -> None:
    xml = dump_ui(serial)
    for m in re.finditer(
        r'(?:text|content-desc)="Read book"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"'
        r'|bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"[^>]*(?:text|content-desc)="Read book"',
        xml,
    ):
        g = [int(x) for x in m.groups() if x]
        if len(g) == 4:
            tap(serial, (g[0] + g[2]) // 2, (g[1] + g[3]) // 2)
            time.sleep(8)
            return
    raise SystemExit("No 'Read book' control — still on login, or library empty.")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", help="adb serial (default: first device)")
    parser.add_argument("--install", action="store_true", help="install debug APK first")
    parser.add_argument("--open-first-book", action="store_true")
    args = parser.parse_args()

    server, user, password = load_env()
    serial = pick_serial(args.serial)
    print(f"device={serial}", flush=True)

    adb(serial, "reverse", "tcp:8080", "tcp:8080")
    print(adb(serial, "reverse", "--list").stdout.strip() or "reverse ok", flush=True)

    # Asleep Nomad + lock screen makes uiautomator return null / empty forms and
    # looks like a library outage. Wake and hold the display for the session.
    adb(serial, "shell", "input", "keyevent", "KEYCODE_WAKEUP", check=False)
    adb(serial, "shell", "svc", "power", "stayon", "true", check=False)
    adb(serial, "shell", "settings", "put", "system", "screen_off_timeout", "1800000", check=False)

    if args.install:
        if not APK.is_file():
            raise SystemExit(f"Missing {APK}. Run: flutter build apk --debug (in mobile/)")
        adb(serial, "install", "-r", str(APK))

    adb(serial, "shell", "am", "force-stop", PACKAGE)
    adb(serial, "shell", "am", "start", "-n", ACTIVITY)
    # Nomad e-ink first frame is slow; 3.5s often dumps 0 EditTexts.
    time.sleep(6.0)
    for _ in range(8):
        xml = dump_ui(serial)
        if len(centers(xml, "android.widget.EditText")) >= 3:
            break
        time.sleep(1.0)

    fill_login(serial, server, user, password)
    labels = re.findall(r'(?:text|content-desc)="([^"]+)"', dump_ui(serial))
    if not any("library" in l.lower() or "Read book" in l for l in labels):
        raise SystemExit(f"Login may have failed. UI labels: {labels[:30]}")
    print("logged in", flush=True)

    if args.open_first_book:
        open_first_book(serial)
        print("opened first Read book", flush=True)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(130)
