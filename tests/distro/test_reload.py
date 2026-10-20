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
BANNER = OVERLAY / "share" / "banner"

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
def test_shell_reloads_without_new_kernel_boot():
    original = BANNER.read_text(encoding="utf-8") if BANNER.is_file() else "oseink ready\n"
    token = f"reload-{uuid.uuid4().hex}"

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

        # Wait for the stub shell to publish its first banner.
        proc.stdin.write(b"cat /run/oseink-shell\n")
        proc.stdin.flush()
        deadline = time.time() + 30
        while time.time() < deadline:
            text = _drain(proc, buf, 2)
            if "oseink ready" in text or "oseink-shell:" in text:
                break
        else:
            pytest.fail("shell did not start before edit:\n" + "".join(buf)[-3000:])

        proc.stdin.write(b"cat /proc/sys/kernel/random/boot_id\n")
        proc.stdin.flush()
        text = _drain(proc, buf, 3)
        # boot_id is a uuid on its own line after the command echo.
        boot_id = None
        for line in text.splitlines():
            line = line.strip()
            if len(line) == 36 and line.count("-") == 4:
                boot_id = line
        assert boot_id, "could not read boot_id:\n" + text[-1500:]

        BANNER.write_text(token + "\n", encoding="utf-8")

        deadline = time.time() + 45
        while time.time() < deadline:
            proc.stdin.write(b"cat /run/oseink-shell\n")
            proc.stdin.flush()
            text = _drain(proc, buf, 2)
            if token in text:
                break
        else:
            pytest.fail(
                "edited shell assets did not update running UI:\n" + "".join(buf)[-4000:]
            )

        proc.stdin.write(b"cat /proc/sys/kernel/random/boot_id\n")
        proc.stdin.flush()
        text = _drain(proc, buf, 3)
        assert boot_id in text, "kernel boot changed after userspace reload:\n" + text[-1500:]
    finally:
        BANNER.write_text(original, encoding="utf-8")
        proc.kill()
        proc.wait(timeout=10)
