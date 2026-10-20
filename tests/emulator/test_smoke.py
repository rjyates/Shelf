import subprocess
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

import pytest

from tests.emulator.conftest import ROOT

pytestmark = pytest.mark.smoke

IMAGES = ROOT / ".build" / "oseink" / "images"
KERNEL = IMAGES / "bzImage"
ROOTFS = IMAGES / "rootfs.ext2"


def _run(command, cwd):
    subprocess.run(command, cwd=cwd, check=True)


def _viewer_up():
    try:
        with urllib.request.urlopen("http://127.0.0.1:8006/", timeout=2) as response:
            body = response.read().decode("utf-8", "replace")
    except OSError:
        return False
    return "<title>noVNC</title>" in body


def _container_id(emulator_dir, project):
    return subprocess.check_output(
        ["docker", "compose", "-p", project, "ps", "-q"],
        cwd=emulator_dir,
        text=True,
    ).strip()


def _logs(emulator_dir, project, since=""):
    container = _container_id(emulator_dir, project)
    if not container:
        return ""
    command = ["docker", "logs"]
    if since:
        command.extend(["--since", since])
    command.append(container)
    logs = subprocess.run(command, check=False, capture_output=True, text=True)
    return logs.stdout + logs.stderr


def _guest_up(emulator_dir, project, since=""):
    text = _logs(emulator_dir, project, since)
    return "Welcome to oseink" in text


def _downloaded_an_iso(emulator_dir, project):
    text = _logs(emulator_dir, project).lower()
    if "downloading" in text and ".iso" in text:
        return True
    if "retrieving latest" in text:
        return True
    volume = f"{project}_disks"
    result = subprocess.run(
        [
            "docker",
            "run",
            "--rm",
            "--entrypoint",
            "sh",
            "-v",
            f"{volume}:/storage",
            "alpine:3.20",
            "-c",
            "find /storage -iname '*.iso' 2>/dev/null | head -5",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    return bool(result.stdout.strip())


def _rootfs_id():
    # Inode and size only: the guest writes the rootfs, so mtime moves.
    result = subprocess.run(
        ["stat", "-c", "%i %s", str(ROOTFS)],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return ""
    return result.stdout


def _wait(predicate, emulator_dir, project):
    for _ in range(150):
        if predicate():
            return
        time.sleep(2)
    subprocess.run(
        ["docker", "compose", "-p", project, "logs", "--tail", "40"],
        cwd=emulator_dir,
        check=False,
    )
    pytest.fail("timed out waiting for the emulator")


@pytest.mark.skipif(not Path("/dev/kvm").exists(), reason="needs /dev/kvm")
@pytest.mark.skipif(not KERNEL.is_file() or not ROOTFS.is_file(), reason="run distro/build.sh first")
def test_oseink_guest_without_iso_and_disk_survives_restart(emulator_dir):
    project = "oseink-smoke"
    try:
        _run(["docker", "compose", "-p", project, "up", "-d", "--build"], emulator_dir)
        _wait(lambda: _viewer_up(), emulator_dir, project)
        _wait(lambda: _guest_up(emulator_dir, project), emulator_dir, project)
        assert not _downloaded_an_iso(emulator_dir, project)
        before = _rootfs_id()
        assert before
        since = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        _run(["docker", "compose", "-p", project, "restart"], emulator_dir)
        _wait(lambda: _viewer_up(), emulator_dir, project)
        _wait(lambda: _guest_up(emulator_dir, project, since), emulator_dir, project)
        assert _rootfs_id() == before
        assert not _downloaded_an_iso(emulator_dir, project)
    finally:
        subprocess.run(
            ["docker", "compose", "-p", project, "down", "-v", "--remove-orphans"],
            cwd=emulator_dir,
            check=False,
            capture_output=True,
        )
