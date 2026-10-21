import json
import os
import select
import subprocess
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
OVERLAY = ROOT / "distro" / "overlay"
BOOT = OVERLAY / "run" / "boot-target"
SESSION = OVERLAY / "etc" / "init.d" / "S60oseink-session"
READER = OVERLAY / "bin" / "oseink-reader"
DESKTOP = OVERLAY / "bin" / "oseink-desktop"
PAGE = OVERLAY / "share" / "demo" / "page.gray"
DAMAGE = OVERLAY / "run" / "damage.json"
FRAME = OVERLAY / "run" / "frame.gray"
IMAGES = ROOT / ".build" / "oseink" / "images"
KERNEL = IMAGES / "bzImage"
ROOTFS = IMAGES / "rootfs.ext2"


def _target():
    return subprocess.run(
        ["sh", str(SESSION), "target"],
        capture_output=True,
        text=True,
        check=False,
    )


def test_boot_target_defaults_to_reader():
    ignored = subprocess.run(
        ["git", "check-ignore", "-q", "--", "distro/overlay/run/boot-target"],
        cwd=ROOT,
        check=False,
    )
    assert ignored.returncode != 0
    assert BOOT.read_text(encoding="utf-8") == "reader\n"
    result = _target()
    assert result.returncode == 0, result.stderr
    assert result.stdout == "reader\n"


def test_session_script_accepts_only_reader_or_desktop():
    original = BOOT.read_text(encoding="utf-8")
    try:
        BOOT.write_text("desktop\n", encoding="utf-8")
        assert _target().stdout == "desktop\n"
        BOOT.write_text("nope\n", encoding="utf-8")
        assert _target().stdout == "reader\n"
        BOOT.unlink()
        assert _target().stdout == "reader\n"
        bad = subprocess.run(
            ["sh", str(SESSION), "restart", "nope"],
            capture_output=True,
            text=True,
            check=False,
        )
        assert bad.returncode != 0
        assert not BOOT.exists()
    finally:
        BOOT.write_text(original, encoding="utf-8")


def test_reader_stays_damage_client_and_desktop_starts_weston():
    session = SESSION.read_text(encoding="utf-8")
    assert "/oseink/run/boot-target" in session or "run/boot-target" in session
    assert "oseink-reader" in session
    assert "oseink-desktop" in session
    assert "/usr/bin/weston" not in session
    reader = READER.read_text(encoding="utf-8")
    desktop = DESKTOP.read_text(encoding="utf-8")
    lowered = reader.lower()
    assert "weston" not in lowered
    assert "readium" not in lowered
    assert "mupdf" not in lowered
    assert "readium" not in session.lower()
    assert "mupdf" not in session.lower()
    assert "/oseink/bin/oseink-damage" in reader
    assert "page.gray" not in reader
    assert "menu.gray" not in desktop
    assert "oseink-damage" not in desktop
    assert "readium" not in desktop.lower()
    assert "mupdf" not in desktop.lower()
    assert "/usr/bin/seatd" in desktop
    assert "/oseink/bin/oseink-input" in desktop
    assert "udevadm settle" in desktop
    assert "atkbd" in desktop
    for name in (
        "buttons",
        "touch",
        "stylus",
        "touch_and_stylus",
        "buttons_and_touch",
        "buttons_and_stylus",
        "buttons_and_touch_and_stylus",
    ):
        assert name in desktop
    assert "oseink-input" not in reader
    assert "/usr/bin/seatd" not in reader
    assert "/usr/bin/weston" in desktop
    assert "--socket=wayland-0" in desktop
    assert "--backend=headless" in desktop
    assert "--renderer=pixman" in desktop
    assert "--shell=desktop" in desktop
    assert "--idle-time=0" in desktop
    assert '--width="$width"' in desktop
    assert '--height="$height"' in desktop
    assert "/oseink/etc/weston.ini" in desktop
    ini = (OVERLAY / "etc" / "weston.ini").read_text(encoding="utf-8")
    assert "backend=headless" in ini
    assert "renderer=pixman" in ini
    assert "shell=desktop" in ini
    assert "idle-time=0" in ini
    assert "animation=none" in ini
    assert "startup-animation=none" in ini
    assert "close-animation=none" in ini
    assert "focus-animation=none" in ini
    assert "require-input=false" in ini
    assert "path=/oseink/bin/oseink-terminal" in ini
    terminal = (OVERLAY / "bin" / "oseink-terminal").read_text(encoding="utf-8")
    assert "weston-terminal --fullscreen" in terminal
    shell = (OVERLAY / "bin" / "oseink-shell").read_text(encoding="utf-8")
    assert "S60oseink-session foreground" in shell
    assert "pidof weston" in SESSION.read_text(encoding="utf-8")


def _have_qemu():
    return subprocess.run(["which", "qemu-system-x86_64"], capture_output=True).returncode == 0


def _drain(proc, buf, seconds):
    deadline = time.time() + seconds
    while time.time() < deadline:
        if proc.poll() is not None:
            break
        ready, _, _ = select.select([proc.stdout], [], [], 0.25)
        if not ready:
            continue
        chunk = os.read(proc.stdout.fileno(), 4096)
        if not chunk:
            break
        buf.append(chunk.decode("utf-8", "replace"))
    return "".join(buf)


def _boot_id(text):
    found = None
    for line in text.splitlines():
        line = line.strip()
        if len(line) == 36 and line.count("-") == 4:
            found = line
    return found


def _fresh(buf, mark):
    return "".join(buf)[mark:]


def _damage():
    if not DAMAGE.is_file() or not FRAME.is_file():
        return None
    try:
        msg = json.loads(DAMAGE.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return None
    return msg, FRAME.read_bytes()


@pytest.mark.smoke
@pytest.mark.skipif(not KERNEL.is_file() or not ROOTFS.is_file(), reason="run distro/build.sh first")
@pytest.mark.skipif(not _have_qemu(), reason="needs qemu-system-x86_64")
def test_guest_boots_saved_session_and_switches_without_reboot():
    original = BOOT.read_text(encoding="utf-8")
    profile_path = OVERLAY / "run" / "profile.json"
    original_profile = profile_path.read_text(encoding="utf-8")
    input_dir = OVERLAY / "run" / "input"
    input_dir.mkdir(exist_ok=True)
    os.chmod(input_dir, 0o777)
    before = _damage()
    before_seq = before[0]["seq"] if before else 0
    BOOT.write_text("desktop\n", encoding="utf-8")
    command = [
        "qemu-system-x86_64",
        "-m",
        "512",
        "-kernel",
        str(KERNEL),
        "-drive",
        f"file={ROOTFS},if=virtio,format=raw,snapshot=on",
        "-append",
        "root=/dev/vda console=ttyS0 console=tty0",
        "-vga",
        "std",
        "-display",
        "none",
        "-serial",
        "stdio",
        "-no-reboot",
        "-virtfs",
        f"local,path={OVERLAY},mount_tag=shared,security_model=none,id=oseink",
    ]
    if Path("/dev/kvm").exists():
        command.insert(1, "-enable-kvm")

    proc = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        bufsize=0,
    )
    buf = []
    try:
        deadline = time.time() + 90
        while time.time() < deadline:
            text = _drain(proc, buf, 2)
            assert proc.poll() is None, text
            if "oseink-share:fail" in text:
                pytest.fail("guest failed to mount shared overlay:\n" + text[-2000:])
            if "login:" in text:
                break
        else:
            pytest.fail("guest did not reach login:\n" + "".join(buf)[-2000:])

        assert proc.stdin is not None
        proc.stdin.write(b"root\n")
        proc.stdin.flush()
        _drain(proc, buf, 2)

        profile = json.loads((OVERLAY / "run" / "profile.json").read_text(encoding="utf-8"))
        width = profile["width"]
        height = profile["height"]

        def guest(cmd, seconds=2):
            mark = len("".join(buf))
            proc.stdin.write(cmd.encode() + b"\n")
            proc.stdin.flush()
            _drain(proc, buf, seconds)
            return _fresh(buf, mark)

        def seat_line():
            text = guest(
                "printf '%s|%s|%s|%s|%s\\n' "
                '"$(cat /run/oseink-session 2>/dev/null)" '
                '"$(cat /run/oseink-desktop.status 2>/dev/null)" '
                '"$(pidof weston || echo x)" '
                '"$(pidof weston-terminal || echo x)" '
                '"$(pidof seatd || echo x)"'
            )
            for line in text.splitlines():
                line = line.strip()
                if line.startswith(("desktop|", "reader|")):
                    return line.split("|")
            return []

        def wait_desktop(seq_floor):
            deadline = time.time() + 60
            last = []
            got = None
            while time.time() < deadline:
                last = seat_line()
                got = _damage()
                if (
                    len(last) == 5
                    and last[0] == "desktop"
                    and last[1] == f"weston {width} {height}"
                    and last[2] != "x"
                    and last[3] != "x"
                    and last[4] != "x"
                    and got
                    and got[0].get("v") == 1
                    and got[0].get("format") == "gray8"
                    and got[0].get("mode") in ("quality", "fast", "full")
                    and got[0].get("seq", 0) > seq_floor
                    and got[0].get("panel_w") == width
                    and got[0].get("panel_h") == height
                    and len(got[1]) == width * height
                ):
                    return got[0]["seq"]
            log = guest("cat /run/oseink-weston.log /run/oseink-seatd.log", 3)
            pytest.fail(
                "desktop did not start Weston:\n"
                + "|".join(last)
                + "\n"
                + log[-2000:]
                + "\n"
                + "".join(buf)[-2000:]
                + f"\ndamage={got[0] if got else None}"
            )

        def wait_reader(seq_floor):
            deadline = time.time() + 45
            last = []
            got = None
            while time.time() < deadline:
                last = seat_line()
                got = _damage()
                if (
                    len(last) == 5
                    and last[0] == "reader"
                    and last[2:] == ["x", "x", "x"]
                    and got
                    and got[0].get("mode") == "quality"
                    and got[0].get("seq", 0) > seq_floor
                    and got[1] == PAGE.read_bytes()
                ):
                    return got[0]["seq"]
            notes = [
                line
                for line in "".join(buf).splitlines()
                if "oseink-" in line or "damage" in line
            ]
            match = None
            if got:
                match = got[1] == PAGE.read_bytes()
            pytest.fail(
                "reader did not return without Weston:\n"
                + "|".join(last)
                + f"\nseq_floor={seq_floor} bytes_match={match} damage={got[0] if got else None}\n"
                + "\n".join(notes[-40:])
            )

        def yes(cmd, seconds=4):
            text = guest(cmd, seconds)
            return any(line.strip() == "YES" for line in text.splitlines())

        def device_names():
            text = guest("cat /proc/bus/input/devices", 2)
            names = []
            for line in text.splitlines():
                line = line.strip()
                if line.startswith("N: Name="):
                    names.append(line.split("=", 1)[1].strip().strip('"'))
            return names

        def wait_named(expect, seq_floor):
            deadline = time.time() + 60
            last = []
            got = None
            while time.time() < deadline:
                last = device_names()
                got = _damage()
                seat = seat_line()
                oseink = [name for name in last if name.startswith("oseink ")]
                if (
                    set(oseink) == set(expect)
                    and "AT Translated Set 2 keyboard" not in last
                    and len(seat) == 5
                    and seat[0] == "desktop"
                    and seat[2] != "x"
                    and seat[3] != "x"
                    and got
                    and got[0].get("seq", 0) > seq_floor
                ):
                    return got[0]["seq"]
            log = guest("cat /run/oseink-weston.log /run/oseink-input.log", 3)
            pytest.fail(
                "profile devices did not match "
                + ",".join(expect)
                + ":\n"
                + "\n".join(last)
                + "\n"
                + log[-2000:]
            )

        def write_input(value):
            doc = json.loads(profile_path.read_text(encoding="utf-8"))
            doc["input"] = value
            profile_path.write_text(json.dumps(doc, indent=2) + "\n", encoding="utf-8")

        def clear_queue():
            if not input_dir.is_dir():
                input_dir.mkdir()
                os.chmod(input_dir, 0o777)
                return
            for child in input_dir.iterdir():
                if child.is_file() and child.name.isdigit():
                    child.unlink()

        def type_line(text):
            clear_queue()
            codes = {
                "a": 30, "b": 48, "c": 46, "d": 32, "e": 18, "f": 33, "g": 34,
                "h": 35, "i": 23, "j": 36, "k": 37, "l": 38, "m": 50, "n": 49,
                "o": 24, "p": 25, "q": 16, "r": 19, "s": 31, "t": 20, "u": 22,
                "v": 47, "w": 17, "x": 45, "y": 21, "z": 44,
                "/": 53, ".": 52, "-": 12, " ": 57,
            }
            n = 0

            def put(code, down):
                nonlocal n
                n += 1
                (input_dir / f"{n:08d}").write_text(f"k {code} {down}\n", encoding="utf-8")

            for ch in text:
                if ch == "\n":
                    put(28, 1)
                    put(28, 0)
                elif ch.isupper():
                    put(42, 1)
                    put(codes[ch.lower()], 1)
                    put(codes[ch.lower()], 0)
                    put(42, 0)
                elif ch.isdigit():
                    code = 11 if ch == "0" else 1 + int(ch)
                    put(code, 1)
                    put(code, 0)
                else:
                    put(codes[ch], 1)
                    put(codes[ch], 0)

        seq = wait_desktop(before_seq)
        pid = "/proc/$(pidof weston)/cmdline"
        assert yes(f"grep -a -q width={width} {pid} && echo YES")
        assert yes(f"grep -a -q height={height} {pid} && echo YES")
        assert yes(f"grep -a -q renderer=pixman {pid} && echo YES")
        assert yes(f"grep -a -q backend=headless {pid} && echo YES")
        assert yes(f"grep -a -q idle-time=0 {pid} && echo YES")
        assert yes("grep -q 'seat seat0' /run/oseink-seatd.log && echo YES")
        assert yes("weston --version | grep -q 14.0.1 && echo YES")
        assert yes("test ! -e /usr/bin/Xorg && echo YES")
        assert yes("test ! -e /usr/bin/Xwayland && echo YES")
        assert yes("find /usr -name drm-backend.so | grep -q . || echo YES")
        assert yes("find /usr -name gl-renderer.so | grep -q . || echo YES")
        assert yes("find /usr -name headless-backend.so | grep -q . && echo YES")
        assert yes("find /usr/lib -name 'libinput.so*' | grep -q . && echo YES")
        names = device_names()
        assert [name for name in names if name.startswith("oseink ")] == ["oseink buttons"]
        assert "AT Translated Set 2 keyboard" not in names
        assert yes("grep -F 'device is a keyboard' /run/oseink-weston.log && echo YES")

        write_input("touch_and_stylus")
        seq = wait_named(["oseink touch", "oseink stylus"], seq)
        assert yes("grep -F 'device is a touch device' /run/oseink-weston.log && echo YES")
        assert yes("grep -F 'device is a tablet' /run/oseink-weston.log && echo YES")
        write_input("buttons")
        seq = wait_named(["oseink buttons"], seq)

        mark = len("".join(buf))
        proc.stdin.write(b"cat /proc/sys/kernel/random/boot_id\n")
        proc.stdin.flush()
        text = _drain(proc, buf, 3)
        boot_id = _boot_id(_fresh(buf, mark))
        assert boot_id, "could not read boot_id:\n" + text[-1500:]

        type_line("/oseink/etc/init.d/S60oseink-session restart reader\n")
        seq = wait_reader(seq)
        console = guest("cat /dev/vcs", 2)
        assert "S60oseink" not in console
        assert not any(name.startswith("oseink ") for name in device_names())

        mark = len("".join(buf))
        proc.stdin.write(b"cat /proc/sys/kernel/random/boot_id\n")
        proc.stdin.flush()
        text = _drain(proc, buf, 3)
        assert _boot_id(_fresh(buf, mark)) == boot_id, (
            "kernel boot changed after session switch:\n" + text[-1500:]
        )

        # A file write alone restarts the presenter. The image stays up.
        BOOT.write_text("desktop\n", encoding="utf-8")
        wait_desktop(seq)
        mark = len("".join(buf))
        proc.stdin.write(b"cat /proc/sys/kernel/random/boot_id\n")
        proc.stdin.flush()
        text = _drain(proc, buf, 3)
        assert _boot_id(_fresh(buf, mark)) == boot_id, (
            "kernel boot changed after boot-target write:\n" + text[-1500:]
        )
    finally:
        try:
            BOOT.write_text(original, encoding="utf-8")
            profile_path.write_text(original_profile, encoding="utf-8")
            if input_dir.is_dir():
                for child in input_dir.iterdir():
                    if child.is_file() and child.name.isdigit():
                        child.unlink()
        finally:
            proc.kill()
            proc.wait(timeout=10)
