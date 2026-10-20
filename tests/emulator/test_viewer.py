import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VIEWER = ROOT / "emulator" / "viewer"
DOCKERFILE = ROOT / "emulator" / "Dockerfile"


def test_viewer_layout():
    assert (VIEWER / "panel.js").is_file()
    assert (VIEWER / "boot.js").is_file()
    assert (VIEWER / "index.html").is_file()
    assert (VIEWER / "viewer.css").is_file()
    assert (VIEWER / "selfcheck.mjs").is_file()


def test_dockerfile_installs_viewer_as_novnc_landing_page():
    text = DOCKERFILE.read_text(encoding="utf-8")
    assert "COPY viewer/index.html /usr/share/novnc/vnc.html" in text
    assert "viewer/boot.js" in text
    assert "viewer/panel.js" in text


def test_panel_source_keeps_eink_behavior_in_the_viewer():
    text = (VIEWER / "panel.js").read_text(encoding="utf-8")
    assert "quantize" in text
    assert "ghost" in text
    assert "delayMs" in text
    assert "applyPartial" in text
    assert "applyFull" in text
    assert "flashPhase" in text


def test_boot_pipes_vnc_through_the_panel():
    text = (VIEWER / "boot.js").read_text(encoding="utf-8")
    assert 'from "./core/rfb.js"' in text
    assert "websockify" in text
    assert "applyPartial" in text or "panel.partial" in text
    assert "applyFull" in text
    assert "shortcut:" in text


def test_partial_updates_degrade_and_full_refresh_clears():
    result = subprocess.run(
        ["node", str(VIEWER / "selfcheck.mjs")],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    assert "partial degrades, full clears" in result.stdout
