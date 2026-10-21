import json
import os
import select
import subprocess
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
DISTRO = ROOT / "distro"
OVERLAY = DISTRO / "overlay"
PROFILE = OVERLAY / "run" / "profile.json"
READER = OVERLAY / "bin" / "oseink-profile"
SHELL = OVERLAY / "bin" / "oseink-shell"
IMAGES = ROOT / ".build" / "oseink" / "images"
KERNEL = IMAGES / "bzImage"
ROOTFS = IMAGES / "rootfs.ext2"

GRAY_LINE = "gray 16 buttons no 400 300 300"


def _read(path: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["sh", str(READER), str(path)],
        capture_output=True,
        text=True,
        check=False,
    )


def test_published_profile_is_button_only_gray():
    assert READER.is_file() and READER.stat().st_mode & 0o111
    ignored = subprocess.run(
        ["git", "check-ignore", "-q", "--", "distro/overlay/run/profile.json"],
        cwd=ROOT,
        check=False,
    )
    assert ignored.returncode != 0
    result = _read(PROFILE)
    assert result.returncode == 0, result.stderr
    assert result.stdout == GRAY_LINE + "\n"


def test_touch_and_stylus_color_profiles_parse(tmp_path: Path):
    for color in ("kaleido", "full"):
        path = tmp_path / f"{color}.json"
        path.write_text(
            "\n".join(
                [
                    "{",
                    f'  "color": "{color}",',
                    '  "gray_levels": 2,',
                    '  "input": "touch_and_stylus",',
                    '  "frontlight": "yes",',
                    '  "width": 400,',
                    '  "height": 300,',
                    '  "dpi": 300',
                    "}",
                    "",
                ]
            ),
            encoding="utf-8",
        )
        result = _read(path)
        assert result.returncode == 0, result.stderr
        assert result.stdout == f"{color} 2 touch_and_stylus yes 400 300 300\n"


def test_reader_rejects_a_device_model_and_a_bad_level(tmp_path: Path):
    kindle = tmp_path / "kindle.json"
    kindle.write_text(
        PROFILE.read_text(encoding="utf-8").replace('"gray"', '"kindle"'),
        encoding="utf-8",
    )
    assert _read(kindle).returncode != 0

    levels = tmp_path / "levels.json"
    levels.write_text(
        PROFILE.read_text(encoding="utf-8").replace('"gray_levels": 16', '"gray_levels": 4'),
        encoding="utf-8",
    )
    assert _read(levels).returncode != 0
    assert _read(tmp_path / "missing.json").returncode != 0


def test_distro_has_no_device_model_table():
    blob = []
    for path in DISTRO.rglob("*"):
        if path.is_file() and path.suffix in {".sh", ".json", ".py", ""}:
            blob.append(path.read_text(encoding="utf-8", errors="replace").lower())
    text = "\n".join(blob)
    assert "kindle" not in text
    assert "kobo" not in text
    assert "remarkable" not in text


def test_shell_reads_profile_at_session_start():
    text = (OVERLAY / "bin" / "oseink-reader").read_text(encoding="utf-8")
    assert text.index("/oseink/bin/oseink-profile") < text.index("/dev/tty0")
    assert "/oseink/run/profile.json" in text
    assert "/run/oseink-profile" in text
    desktop = (OVERLAY / "bin" / "oseink-desktop").read_text(encoding="utf-8")
    assert desktop.index("/oseink/bin/oseink-profile") < desktop.index("/dev/tty0")
    assert "/run/oseink-profile" in desktop
    stop = (OVERLAY / "etc" / "init.d" / "S50oseink-devd").read_text(encoding="utf-8")
    assert "/run/oseink-profile" in stop


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


@pytest.mark.smoke
@pytest.mark.skipif(not KERNEL.is_file() or not ROOTFS.is_file(), reason="run distro/build.sh first")
@pytest.mark.skipif(not _have_qemu(), reason="needs qemu-system-x86_64")
def test_guest_reads_profile_at_session_start():
    original = PROFILE.read_text(encoding="utf-8")
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
        proc.stdin.write(b"cat /run/oseink-profile\n")
        proc.stdin.flush()

        deadline = time.time() + 30
        while time.time() < deadline:
            text = _drain(proc, buf, 2)
            if GRAY_LINE in text:
                break
        else:
            pytest.fail("guest did not read the profile:\n" + "".join(buf)[-3000:])

        assert proc.stdin is not None
        proc.stdin.write(b"cat /proc/sys/kernel/random/boot_id\n")
        proc.stdin.flush()
        text = _drain(proc, buf, 3)
        boot_id = None
        for line in text.splitlines():
            line = line.strip()
            if len(line) == 36 and line.count("-") == 4:
                boot_id = line
        assert boot_id, "could not read boot_id:\n" + text[-1500:]

        changed = json.loads(original)
        changed["color"] = "kaleido"
        changed["input"] = "buttons_and_touch"
        color_line = (
            f"{changed['color']} {changed['gray_levels']} {changed['input']} "
            f"{changed['frontlight']} {changed['width']} {changed['height']} {changed['dpi']}"
        )
        # Write the profile, then the share stamp. The image's devd restarts
        # the shell when share content changes.
        text = json.dumps(changed, indent=2) + "\n"
        PROFILE.write_text(text, encoding="utf-8")
        stamp = OVERLAY / "share" / "profile.stamp"
        stamp.write_text(text, encoding="utf-8")

        deadline = time.time() + 30
        while time.time() < deadline:
            proc.stdin.write(b"cat /run/oseink-profile\n")
            proc.stdin.flush()
            text = _drain(proc, buf, 2)
            if color_line in text:
                break
        else:
            pytest.fail("guest did not reread the profile:\n" + "".join(buf)[-3000:])

        deadline = time.time() + 15
        while time.time() < deadline:
            proc.stdin.write(b"cat /proc/sys/kernel/random/boot_id\n")
            proc.stdin.flush()
            text = _drain(proc, buf, 2)
            ids = [
                line.strip()
                for line in text.splitlines()
                if len(line.strip()) == 36 and line.strip().count("-") == 4
            ]
            if len(ids) >= 2:
                assert ids[-1] == boot_id, "kernel boot changed after profile write:\n" + text[-1500:]
                break
        else:
            pytest.fail("guest did not reread boot_id:\n" + "".join(buf)[-1500:])
    finally:
        PROFILE.write_text(original, encoding="utf-8")
        stamp = OVERLAY / "share" / "profile.stamp"
        stamp.unlink(missing_ok=True)
        proc.kill()
        proc.wait(timeout=10)
