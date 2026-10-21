import json
import os
import socket
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VIEWER = ROOT / "emulator" / "viewer"
DOCKERFILE = ROOT / "emulator" / "Dockerfile"
COMPOSE = ROOT / "emulator" / "compose.yml"
PROFILE = ROOT / "distro" / "overlay" / "run" / "profile.json"
CHOICES = VIEWER / "profile-choices.json"
WRITER = VIEWER / "profile-writer.py"


def test_viewer_layout():
    assert (VIEWER / "panel.js").is_file()
    assert (VIEWER / "boot.js").is_file()
    assert (VIEWER / "damage.js").is_file()
    assert (VIEWER / "index.html").is_file()
    assert (VIEWER / "viewer.css").is_file()
    assert (VIEWER / "selfcheck.mjs").is_file()


def test_dockerfile_installs_viewer_as_novnc_landing_page():
    text = DOCKERFILE.read_text(encoding="utf-8")
    assert "COPY viewer/index.html /usr/share/novnc/vnc.html" in text
    assert "viewer/boot.js" in text
    assert "viewer/panel.js" in text
    assert "viewer/damage.js" in text
    assert "viewer/profile-choices.json" in text
    assert "viewer/profile-writer.py" in text
    assert "viewer/profile-location.conf" in text
    assert "COPY viewer/start.sh /run/start.sh" in text


def test_compose_mounts_guest_damage_dir():
    text = COMPOSE.read_text(encoding="utf-8")
    assert "distro/overlay/run:/usr/share/novnc/guest" in text


def test_compose_does_not_set_the_profile():
    assert "profile" not in COMPOSE.read_text(encoding="utf-8").lower()


def test_panel_source_keeps_eink_behavior_in_the_viewer():
    text = (VIEWER / "panel.js").read_text(encoding="utf-8")
    assert "quantize" in text
    assert "ghost" in text
    assert "delayMs" in text
    assert "applyPartial" in text
    assert "applyFull" in text
    assert "flashPhase" in text


def test_damage_protocol_defines_three_modes():
    text = (VIEWER / "damage.js").read_text(encoding="utf-8")
    assert "quality" in text
    assert "fast" in text
    assert '"full"' in text or "full:" in text
    assert "applyDamage" in text
    assert "panel_w" in text


def test_boot_paints_damage_frames_not_the_console():
    text = (VIEWER / "boot.js").read_text(encoding="utf-8")
    assert 'from "./core/rfb.js"' in text
    assert "websockify" in text
    assert "getImageData" not in text
    assert 'fetch("/guest/damage.json"' in text
    assert "applyDamage" in text
    assert "parseDamage" in text
    assert "shortcut:" in text


def test_guest_writer_publishes_a_v1_gray8_frame(tmp_path):
    writer = ROOT / "distro" / "overlay" / "bin" / "oseink-damage"
    page = ROOT / "distro" / "overlay" / "share" / "demo" / "page.gray"
    run = tmp_path / "run"
    for mode in ("quality", "fast", "full"):
        result = subprocess.run(
            ["sh", str(writer), "400", "300", str(run), str(page), mode],
            capture_output=True,
            text=True,
            check=False,
        )
        assert result.returncode == 0, result.stderr
        msg = json.loads((run / "damage.json").read_text(encoding="utf-8"))
        assert msg["v"] == 1
        assert msg["mode"] == mode
        assert msg["format"] == "gray8"
        assert msg["pixels"] == "frame.gray"
        assert [msg["x"], msg["y"], msg["w"], msg["h"]] == [0, 0, 400, 300]
        assert [msg["panel_w"], msg["panel_h"]] == [400, 300]
        assert (run / "frame.gray").read_bytes() == page.read_bytes()
        parsed = subprocess.run(
            [
                "node",
                "--input-type=module",
                "-e",
                "import { parseDamage } from './emulator/viewer/damage.js';"
                "const msg = parseDamage(process.argv[1]);"
                "if (msg.mode !== process.argv[2] || msg.format !== 'gray8') process.exit(1);",
                json.dumps(msg),
                mode,
            ],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        assert parsed.returncode == 0, parsed.stderr

    seqs = []
    for _ in range(2):
        subprocess.run(
            ["sh", str(writer), "400", "300", str(run), str(page)],
            check=True,
        )
        seqs.append(json.loads((run / "damage.json").read_text(encoding="utf-8"))["seq"])
    assert seqs[1] == seqs[0] + 1

    short = tmp_path / "short.gray"
    short.write_bytes(b"\x00" * 10)
    bad = subprocess.run(
        ["sh", str(writer), "400", "300", str(run), str(short)],
        capture_output=True,
        text=True,
        check=False,
    )
    assert bad.returncode != 0
    assert json.loads((run / "damage.json").read_text(encoding="utf-8"))["seq"] == seqs[1]

    for path in (ROOT / "distro" / "overlay").rglob("*"):
        assert not path.is_block_device()
        assert not path.is_char_device()


def test_shell_publishes_damage_and_keeps_the_console():
    text = (ROOT / "distro" / "overlay" / "bin" / "oseink-reader").read_text(encoding="utf-8")
    assert "/dev/tty0" in text
    assert "/oseink/share/banner" in text
    assert "page.gray" not in text
    assert "damage.json" not in text
    assert "/oseink/bin/oseink-damage" in text
    desktop = (ROOT / "distro" / "overlay" / "bin" / "oseink-desktop").read_text(encoding="utf-8")
    assert "/dev/tty0" in desktop
    assert "menu.gray" not in desktop
    assert "oseink-damage" not in desktop
    assert "damage.json" not in desktop
    assert "/usr/bin/weston" in desktop
    writer = (ROOT / "distro" / "overlay" / "bin" / "oseink-damage").read_text(encoding="utf-8")
    assert "damage.json" in writer
    assert "frame.gray" in writer
    assert "page.gray" in writer
    assert "/dev/" not in writer
    assert "mknod" not in writer


def test_viewer_sets_color_and_interaction_separately():
    page = (VIEWER / "index.html").read_text(encoding="utf-8")
    assert 'data-color="bw"' in page
    assert "Black and white" in page
    assert 'data-color="color"' in page
    assert ">Color<" in page
    for name in ("buttons", "touch", "stylus"):
        assert f'data-input="{name}"' in page
    assert 'id="panel-w"' in page
    assert 'id="panel-h"' in page
    assert ">Width<" in page
    assert ">Height<" in page
    assert 'max="4096"' in page
    script = (VIEWER / "boot.js").read_text(encoding="utf-8")
    assert 'fetch("/profile"' in script
    assert "POST" in script
    assert "Pick at least one." in script
    assert "Width and height are whole numbers from 1 to 4096." in script
    assert "--panel-w" in script
    css = (VIEWER / "viewer.css").read_text(encoding="utf-8")
    assert "--panel-w" in css
    assert "aspect-ratio" in css
    choices = json.loads(CHOICES.read_text(encoding="utf-8"))
    assert choices["bw"] == "gray"
    assert choices["color_on"] in {"kaleido", "full"}
    assert set(choices["input"]) >= {
        "buttons",
        "touch",
        "stylus",
        "touch_and_stylus",
        "buttons_and_touch",
        "buttons_and_stylus",
        "buttons_and_touch_and_stylus",
    }
    reader = (ROOT / "distro" / "overlay" / "bin" / "oseink-profile").read_text(encoding="utf-8")
    for name in choices["input"]:
        assert name in reader
    hook = (VIEWER / "start.sh").read_text(encoding="utf-8")
    assert "profile-writer.py" in hook
    assert "return 0" in hook
    location = (VIEWER / "profile-location.conf").read_text(encoding="utf-8")
    assert "location = /profile" in location
    assert "127.0.0.1:8765" in location


def _free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _request(port, method, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}/profile",
        data=data,
        method=method,
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=2) as res:
            return res.status, res.read()
    except urllib.error.HTTPError as err:
        return err.code, err.read()


def test_profile_endpoint_updates_color_and_input_independently(tmp_path):
    dest = tmp_path / "profile.json"
    published = json.loads(PROFILE.read_text(encoding="utf-8"))
    published["color"] = "gray"
    published["input"] = "buttons"
    dest.write_text(json.dumps(published, indent=2) + "\n", encoding="utf-8")
    port = _free_port()
    env = os.environ.copy()
    env.update(
        {
            "OSEINK_PROFILE_CHOICES": str(CHOICES),
            "OSEINK_PROFILE_DEST": str(dest),
            "OSEINK_PROFILE_STAMP": str(tmp_path / "profile.stamp"),
            "OSEINK_PROFILE_HOST": "127.0.0.1",
            "OSEINK_PROFILE_PORT": str(port),
        }
    )
    proc = subprocess.Popen(
        ["python3", str(WRITER)],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        deadline = time.time() + 5
        while time.time() < deadline:
            try:
                status, _ = _request(port, "GET")
            except (urllib.error.URLError, ConnectionError):
                if proc.poll() is not None:
                    err = proc.stderr.read() if proc.stderr else ""
                    raise AssertionError(f"profile writer exited: {err}")
                time.sleep(0.05)
                continue
            assert status == 200
            break
        else:
            raise AssertionError("profile writer did not listen")

        status, body = _request(
            port, "POST", {"color": "color", "input": ["buttons", "touch"]}
        )
        assert status == 200, body
        written = json.loads(dest.read_text(encoding="utf-8"))
        assert written["color"] == "kaleido"
        assert written["input"] == "buttons_and_touch"
        assert written["gray_levels"] == published["gray_levels"]
        assert written["frontlight"] == published["frontlight"]
        assert written["width"] == published["width"]
        assert written["height"] == published["height"]
        assert (tmp_path / "profile.stamp").read_text(encoding="utf-8") == dest.read_text(
            encoding="utf-8"
        )
        parsed = subprocess.run(
            ["sh", str(ROOT / "distro" / "overlay" / "bin" / "oseink-profile"), str(dest)],
            capture_output=True,
            text=True,
            check=False,
        )
        assert parsed.returncode == 0, parsed.stderr
        assert "buttons_and_touch" in parsed.stdout

        before = dest.read_text(encoding="utf-8")
        status, _ = _request(port, "POST", {"color": "color", "input": []})
        assert status == 400
        assert dest.read_text(encoding="utf-8") == before

        full = json.loads(before)
        full["color"] = "full"
        dest.write_text(json.dumps(full, indent=2) + "\n", encoding="utf-8")
        status, _ = _request(port, "POST", {"color": "color", "input": ["stylus"]})
        assert status == 200
        kept = json.loads(dest.read_text(encoding="utf-8"))
        assert kept["color"] == "full"
        assert kept["input"] == "stylus"

        status, _ = _request(port, "POST", {"color": "bw", "input": ["buttons"]})
        assert status == 200
        restored = json.loads(dest.read_text(encoding="utf-8"))
        assert restored["color"] == "gray"
        assert restored["input"] == "buttons"
        assert restored["gray_levels"] == published["gray_levels"]

        status, body = _request(
            port,
            "POST",
            {"color": "bw", "input": ["buttons"], "width": 800, "height": 600},
        )
        assert status == 200, body
        sized = json.loads(dest.read_text(encoding="utf-8"))
        assert sized["width"] == 800
        assert sized["height"] == 600
        assert sized["dpi"] == published["dpi"]
        parsed = subprocess.run(
            ["sh", str(ROOT / "distro" / "overlay" / "bin" / "oseink-profile"), str(dest)],
            capture_output=True,
            text=True,
            check=False,
        )
        assert parsed.returncode == 0, parsed.stderr
        assert parsed.stdout.split() == [
            "gray",
            "16",
            "buttons",
            "no",
            "800",
            "600",
            str(published["dpi"]),
        ]

        kept = dest.read_text(encoding="utf-8")
        for bad in (
            {"color": "bw", "input": ["buttons"], "width": 0, "height": 600},
            {"color": "bw", "input": ["buttons"], "width": 4097, "height": 600},
            {"color": "bw", "input": ["buttons"], "width": True, "height": 600},
            {"color": "bw", "input": ["buttons"], "width": 800.5, "height": 600},
        ):
            status, _ = _request(port, "POST", bad)
            assert status == 400
            assert dest.read_text(encoding="utf-8") == kept
    finally:
        proc.kill()
        proc.wait(timeout=5)


def test_partial_updates_degrade_and_modes_differ():
    result = subprocess.run(
        ["node", str(VIEWER / "selfcheck.mjs")],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    assert "partial degrades, full clears" in result.stdout
    assert "quality slow gray, fast mono, full flashes" in result.stdout


def test_damage_scales_the_demo_page_to_the_profile_size(tmp_path):
    writer = ROOT / "distro" / "overlay" / "bin" / "oseink-damage"
    src = tmp_path / "page.gray"
    raw = bytes((i % 256) for i in range(400 * 300))
    src.write_bytes(raw)
    run = tmp_path / "run"
    result = subprocess.run(
        ["sh", str(writer), "8", "6", str(run), str(src), "quality"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
    frame = (run / "frame.gray").read_bytes()
    assert len(frame) == 48
    msg = json.loads((run / "damage.json").read_text(encoding="utf-8"))
    assert [msg["panel_w"], msg["panel_h"]] == [8, 6]

    def at(x, y):
        sx = x * 400 // 8
        sy = y * 300 // 6
        return raw[sy * 400 + sx]

    assert frame[0] == at(0, 0) == 0
    assert frame[4] == at(4, 0)
    assert frame[8 * 5 + 7] == at(7, 5)

    other = tmp_path / "other.gray"
    other.write_bytes(b"\x00" * 10)
    bad = subprocess.run(
        ["sh", str(writer), "4", "4", str(run), str(other), "quality"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert bad.returncode != 0
