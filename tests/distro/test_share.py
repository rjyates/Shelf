import os
import select
import subprocess
import time
import uuid
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
IMAGES = ROOT / ".build" / "oseink" / "images"
KERNEL = IMAGES / "bzImage"
ROOTFS = IMAGES / "rootfs.ext2"
OVERLAY = ROOT / "distro" / "overlay"
PROBE = OVERLAY / ".oseink-share-probe"

pytestmark = pytest.mark.smoke


def _have_qemu():
    return subprocess.run(["which", "qemu-system-x86_64"], capture_output=True).returncode == 0


def _drain(proc, buf, seconds):
    """Read available serial bytes. Avoid readline: the login prompt has no newline."""
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


@pytest.mark.skipif(not KERNEL.is_file() or not ROOTFS.is_file(), reason="run distro/build.sh first")
@pytest.mark.skipif(not _have_qemu(), reason="needs qemu-system-x86_64")
def test_host_file_visible_in_guest_without_reboot():
    token = f"probe-{uuid.uuid4().hex}"
    if PROBE.exists():
        PROBE.unlink()

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

        # Confirm the share came up (status file written by S30share).
        proc.stdin.write(b"cat /run/oseink-share\n")
        proc.stdin.flush()
        text = _drain(proc, buf, 3)
        if "9p" not in text and "virtiofs" not in text:
            # Older image may lack /run/oseink-share; fall through to the probe check.
            proc.stdin.write(b"grep -E '9p|virtiofs' /proc/mounts\n")
            proc.stdin.flush()
            text = _drain(proc, buf, 3)
            if "9p" not in text and "virtiofs" not in text:
                pytest.fail("share not mounted after login:\n" + text[-2000:])

        # Host write after mount; guest must see it without reboot (cache=mmap).
        PROBE.write_text(token + "\n", encoding="utf-8")
        proc.stdin.write(f"cat /oseink/{PROBE.name}\n".encode())
        proc.stdin.flush()

        deadline = time.time() + 20
        while time.time() < deadline:
            text = _drain(proc, buf, 2)
            if token in text:
                return
        pytest.fail(
            "host file not visible in guest without reboot:\n" + "".join(buf)[-3000:]
        )
    finally:
        if PROBE.exists():
            PROBE.unlink()
        proc.kill()
        proc.wait(timeout=10)
