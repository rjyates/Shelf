import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VIEWER = ROOT / "emulator" / "viewer"
DOCKERFILE = ROOT / "emulator" / "Dockerfile"
COMPOSE = ROOT / "emulator" / "compose.yml"
SHELL = ROOT / "distro" / "overlay" / "bin" / "oseink-shell"


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


def test_compose_mounts_guest_damage_dir():
    text = COMPOSE.read_text(encoding="utf-8")
    assert "distro/overlay/run:/usr/share/novnc/guest" in text


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
