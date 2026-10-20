import subprocess
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

import pytest

from tests.emulator.conftest import QEMU_IMAGE

pytestmark = pytest.mark.smoke


def _run(command, cwd):
    subprocess.run(command, cwd=cwd, check=True)


def _viewer_up():
    try:
        with urllib.request.urlopen("http://127.0.0.1:8006/", timeout=2) as response:
            body = response.read().decode("utf-8", "replace")
    except OSError:
        return False
    return "<title>noVNC</title>" in body


def _guest_up(emulator_dir, project, since):
    container = subprocess.check_output(
        ["docker", "compose", "-p", project, "ps", "-q"],
        cwd=emulator_dir,
        text=True,
    ).strip()
    if not container:
        return False
    command = ["docker", "logs"]
    if since:
        command.extend(["--since", since])
    command.append(container)
    logs = subprocess.run(command, check=False, capture_output=True, text=True)
    text = logs.stdout + logs.stderr
    return "Welcome to Alpine Linux" in text


def _disk_id(volume):
    result = subprocess.run(
        [
            "docker",
            "run",
            "--rm",
            "--entrypoint",
            "stat",
            "-v",
            f"{volume}:/storage",
            QEMU_IMAGE,
            "-c",
            "%n %i %s",
            "/storage/alpine/data.img",
            "/storage/alpine/boot.iso",
        ],
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
def test_console_and_disk_survive_a_restart(emulator_dir):
    project = "oseink-smoke"
    volume = f"{project}_disks"
    try:
        _run(["docker", "compose", "-p", project, "up", "-d", "--build"], emulator_dir)
        _wait(lambda: _viewer_up(), emulator_dir, project)
        _wait(lambda: _guest_up(emulator_dir, project, ""), emulator_dir, project)
        before = _disk_id(volume)
        assert before
        since = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        _run(["docker", "compose", "-p", project, "restart"], emulator_dir)
        _wait(lambda: _viewer_up(), emulator_dir, project)
        _wait(lambda: _guest_up(emulator_dir, project, since), emulator_dir, project)
        assert _disk_id(volume) == before
    finally:
        subprocess.run(
            ["docker", "compose", "-p", project, "down", "-v", "--remove-orphans"],
            cwd=emulator_dir,
            check=False,
            capture_output=True,
        )
