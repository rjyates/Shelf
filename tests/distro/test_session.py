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
    assert "path=/usr/bin/weston-terminal" in ini
    shell = (OVERLAY / "bin" / "oseink-shell").read_text(encoding="utf-8")
    assert "S60oseink-session foreground" in shell


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
                seq = got[0]["seq"] if got else 0
                if (
                    len(last) == 5
                    and last[0] == "desktop"
                    and last[1] == f"weston {width} {height}"
                    and last[2] != "x"
                    and last[3] != "x"
                    and last[4] != "x"
                    and seq == seq_floor
                ):
                    return seq
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
            pytest.fail(
                "reader did not return without Weston:\n"
                + "|".join(last)
                + "\n"
                + "".join(buf)[-3000:]
                + f"\ndamage={got[0] if got else None}"
            )

        def yes(cmd, seconds=4):
            text = guest(cmd, seconds)
            return any(line.strip() == "YES" for line in text.splitlines())

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

        mark = len("".join(buf))
        proc.stdin.write(b"cat /proc/sys/kernel/random/boot_id\n")
        proc.stdin.flush()
        text = _drain(proc, buf, 3)
        boot_id = _boot_id(_fresh(buf, mark))
        assert boot_id, "could not read boot_id:\n" + text[-1500:]

        proc.stdin.write(b"/oseink/etc/init.d/S60oseink-session restart reader\n")
        proc.stdin.flush()
        seq = wait_reader(seq)

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
        BOOT.write_text(original, encoding="utf-8")
        proc.kill()
        proc.wait(timeout=10)
