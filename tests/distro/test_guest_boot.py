import select
import subprocess
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
IMAGES = ROOT / ".build" / "oseink" / "images"
KERNEL = IMAGES / "bzImage"
ROOTFS = IMAGES / "rootfs.ext2"

pytestmark = pytest.mark.smoke


def _have_qemu():
    return subprocess.run(["which", "qemu-system-x86_64"], capture_output=True).returncode == 0


@pytest.mark.skipif(not KERNEL.is_file() or not ROOTFS.is_file(), reason="run distro/build.sh first")
@pytest.mark.skipif(not _have_qemu(), reason="needs qemu-system-x86_64")
def test_guest_reaches_framebuffer():
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
    ]
    if Path("/dev/kvm").exists():
        command.insert(1, "-enable-kvm")

    proc = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
    )
    output = []
    try:
        deadline = time.time() + 90
        while time.time() < deadline:
            assert proc.poll() is None, "".join(output)
            ready, _, _ = select.select([proc.stdout], [], [], 1)
            if not ready:
                continue
            line = proc.stdout.readline()
            if not line:
                continue
            output.append(line)
            text = "".join(output)
            has_fb = "fb0" in text and ("bochs" in text.lower() or "frame buffer" in text)
            has_login = "Welcome to oseink" in text or "login:" in text
            if has_fb and has_login:
                return
        pytest.fail("guest did not reach framebuffer login:\n" + "".join(output[-80:]))
    finally:
        proc.kill()
        proc.wait(timeout=10)
