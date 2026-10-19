import json
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
EMULATOR = ROOT / "emulator"
QEMU_IMAGE = (
    "qemux/qemu:7.50@sha256:e7f6fda52503a546fd649670ba46e4bc23dc6dcef275bc3fac48877fbbc430df"
)


@pytest.fixture(scope="session")
def emulator_dir():
    return EMULATOR


@pytest.fixture(scope="session")
def compose_config(emulator_dir):
    raw = subprocess.check_output(
        ["docker", "compose", "config", "--format", "json"],
        cwd=emulator_dir,
        text=True,
    )
    return json.loads(raw)
