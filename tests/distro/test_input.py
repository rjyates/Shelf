import os
import select
import struct
import subprocess
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
BIN = ROOT / "distro" / "overlay" / "bin" / "oseink-input"
EVENT = struct.Struct("llHHi")
EV_KEY = 1
EV_ABS = 3
ABS_X = 0
ABS_MT_POSITION_X = 0x35
BTN_TOUCH = 0x14A
BTN_TOOL_PEN = 0x140


def test_input_binary_is_the_guest_helper():
    assert BIN.is_file()
    assert os.access(BIN, os.X_OK)
    missing = subprocess.run([BIN], capture_output=True, text=True, check=False)
    assert missing.returncode == 2
    assert "usage:" in missing.stderr


def _handlers():
    found = {}
    current = None
    text = Path("/proc/bus/input/devices").read_text(encoding="utf-8", errors="replace")
    for line in text.splitlines():
        if line.startswith("N: Name="):
            current = line.split("=", 1)[1].strip().strip('"')
        elif line.startswith("H: Handlers=") and current:
            rest = line.split("Handlers=", 1)[1]
            for part in rest.split():
                if part.startswith("event"):
                    found[current] = Path("/dev/input") / part
            current = None
    return found


def _open(node):
    deadline = time.time() + 2
    last = None
    while time.time() < deadline:
        try:
            return os.open(node, os.O_RDONLY | os.O_NONBLOCK)
        except PermissionError as err:
            last = err
            time.sleep(0.05)
    raise last


def _wait(fd, pred, seconds=2):
    deadline = time.time() + seconds
    while time.time() < deadline:
        ready, _, _ = select.select([fd], [], [], 0.2)
        if not ready:
            continue
        data = os.read(fd, EVENT.size)
        if len(data) < EVENT.size:
            continue
        _sec, _usec, etype, code, value = EVENT.unpack_from(data)
        if pred(etype, code, value):
            return etype, code, value
    return None


@pytest.mark.skipif(not os.access("/dev/uinput", os.W_OK), reason="needs /dev/uinput")
def test_profile_devices_emit_the_queued_event(tmp_path):
    queue = tmp_path / "input"
    queue.mkdir()

    def launch(*flags):
        return subprocess.Popen(
            [BIN, str(queue), "40", "30", "300", *flags],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )

    def wait_handlers(proc, ready):
        deadline = time.time() + 8
        while time.time() < deadline:
            if proc.poll() is not None:
                err = proc.stderr.read() if proc.stderr else ""
                pytest.fail(f"oseink-input exited: {err}")
            handlers = _handlers()
            if ready(handlers):
                return handlers
            time.sleep(0.05)
        raw = Path("/proc/bus/input/devices").read_text(encoding="utf-8", errors="replace")
        oseink = [line.strip() for line in raw.splitlines() if "oseink" in line]
        pytest.fail(f"devices did not appear: {oseink}")

    buttons = launch("buttons")
    try:
        handlers = wait_handlers(
            buttons,
            lambda found: "oseink buttons" in found
            and "oseink touch" not in found
            and "oseink stylus" not in found,
        )
        key_fd = _open(handlers["oseink buttons"])
        try:
            (queue / "00000001").write_text("k 30 1\n", encoding="utf-8")
            got = _wait(
                key_fd,
                lambda etype, code, value: etype == EV_KEY and code == 30 and value == 1,
            )
            assert got == (EV_KEY, 30, 1), got
        finally:
            os.close(key_fd)
    finally:
        buttons.kill()
        buttons.wait(timeout=3)

    both = launch("touch", "stylus")
    try:
        handlers = wait_handlers(
            both,
            lambda found: "oseink touch" in found
            and "oseink stylus" in found
            and "oseink buttons" not in found,
        )
        touch_fd = _open(handlers["oseink touch"])
        stylus_fd = _open(handlers["oseink stylus"])
        try:
            (queue / "00000002").write_text("t 8 9 1\n", encoding="utf-8")
            (queue / "00000003").write_text("s 4 5 1 80\n", encoding="utf-8")
            touch_ev = _wait(
                touch_fd,
                lambda etype, code, _value: etype == EV_ABS
                and code in (ABS_X, ABS_MT_POSITION_X),
            )
            stylus_ev = _wait(
                stylus_fd,
                lambda etype, code, _value: etype == EV_KEY and code in (BTN_TOUCH, BTN_TOOL_PEN),
            )
            assert touch_ev is not None, "touch device emitted nothing"
            assert stylus_ev is not None, "stylus device emitted nothing"
        finally:
            os.close(touch_fd)
            os.close(stylus_fd)
    finally:
        both.kill()
        both.wait(timeout=3)
