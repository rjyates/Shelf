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
SHELL = ROOT / "distro" / "overlay" / "bin" / "oseink-shell"
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


def test_boot_shows_guest_framebuffer_through_eink_panel():
    text = (VIEWER / "boot.js").read_text(encoding="utf-8")
    assert 'from "./core/rfb.js"' in text
    assert "websockify" in text
    assert "getImageData" in text
    assert "applyPartial" in text or "panel.partial" in text
    assert "applyFull" in text
    assert "configurePanel" in text
    assert "shortcut:" in text


def test_shell_draws_framebuffer_not_demo_slideshow():
    text = SHELL.read_text(encoding="utf-8")
    assert "/dev/tty0" in text
    assert "/oseink/share/banner" in text
    assert "page.gray" not in text
    assert "damage.json" not in text


def test_viewer_sets_color_and_interaction_separately():
    page = (VIEWER / "index.html").read_text(encoding="utf-8")
    assert 'data-color="bw"' in page
    assert "Black and white" in page
    assert 'data-color="color"' in page
    assert ">Color<" in page
    for name in ("buttons", "touch", "stylus"):
        assert f'data-input="{name}"' in page
    script = (VIEWER / "boot.js").read_text(encoding="utf-8")
    assert 'fetch("/profile"' in script
    assert "POST" in script
    assert "Pick at least one." in script
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
