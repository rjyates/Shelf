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
MENU = OVERLAY / "share" / "demo" / "menu.gray"
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


def test_stubs_are_damage_presenters_not_weston_or_a_library():
    session = SESSION.read_text(encoding="utf-8")
    assert "/oseink/run/boot-target" in session or "run/boot-target" in session
    assert "oseink-reader" in session
    assert "oseink-desktop" in session
    reader = READER.read_text(encoding="utf-8")
    desktop = DESKTOP.read_text(encoding="utf-8")
    for text in (reader, desktop, session):
        lowered = text.lower()
        assert "weston" not in lowered
        assert "readium" not in lowered
        assert "mupdf" not in lowered
    assert "quality" not in desktop
    assert "menu.gray" in desktop
    assert "fast" in desktop
    assert "page.gray" not in reader
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

        def wait_session(name, pixels, mode, seq_floor):
            deadline = time.time() + 30
            while time.time() < deadline:
                mark = len("".join(buf))
                proc.stdin.write(b"cat /run/oseink-session\n")
                proc.stdin.flush()
                _drain(proc, buf, 1)
                lines = [line.strip() for line in _fresh(buf, mark).splitlines()]
                got = _damage()
                if (
                    name in lines
                    and got
                    and got[0].get("mode") == mode
                    and got[0].get("seq", 0) > seq_floor
                    and got[1] == pixels
                ):
                    return got[0]["seq"]
            got = _damage()
            pytest.fail(
                f"session did not become {name} ({mode}):\n"
                + "".join(buf)[-3000:]
                + f"\ndamage={got[0] if got else None}"
            )

        seq = wait_session("desktop", MENU.read_bytes(), "fast", before_seq)

        mark = len("".join(buf))
        proc.stdin.write(b"cat /proc/sys/kernel/random/boot_id\n")
        proc.stdin.flush()
        text = _drain(proc, buf, 3)
        boot_id = _boot_id(_fresh(buf, mark))
        assert boot_id, "could not read boot_id:\n" + text[-1500:]

        proc.stdin.write(b"/oseink/etc/init.d/S60oseink-session restart reader\n")
        proc.stdin.flush()
        seq = wait_session("reader", PAGE.read_bytes(), "quality", seq)

        mark = len("".join(buf))
        proc.stdin.write(b"cat /proc/sys/kernel/random/boot_id\n")
        proc.stdin.flush()
        text = _drain(proc, buf, 3)
        assert _boot_id(_fresh(buf, mark)) == boot_id, (
            "kernel boot changed after session switch:\n" + text[-1500:]
        )

        # A file write alone restarts the presenter. The image stays up.
        BOOT.write_text("desktop\n", encoding="utf-8")
        wait_session("desktop", MENU.read_bytes(), "fast", seq)
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
