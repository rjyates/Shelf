import re
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HEADER = ROOT / "distro" / "panel" / "oseink-refresh.h"
SOURCE = ROOT / "distro" / "panel" / "oseink-refresh.c"
DAMAGE = ROOT / "emulator" / "viewer" / "damage.js"
PATCH = ROOT / "distro" / "patches" / "weston" / "0001-headless-repaint-damage.patch"
WESTON = ROOT / ".build" / "oseink" / "build" / "weston-14.0.1"
DEFCONFIG = ROOT / "distro" / "configs" / "oseink_x86_64_defconfig"


def _macro(name):
    match = re.search(rf"#define {name} (\d+)", HEADER.read_text(encoding="utf-8"))
    assert match, name
    return int(match.group(1))


def _delay(mode):
    text = DAMAGE.read_text(encoding="utf-8")
    match = re.search(rf"{mode}: \{{[^}}]*delayMs: (\d+)", text)
    assert match, mode
    return int(match.group(1))


def test_viewer_and_hook_share_the_busy_table():
    assert _delay("fast") == _macro("OSEINK_BUSY_FAST_MS") == 120
    assert _delay("quality") == _macro("OSEINK_BUSY_QUALITY_MS") == 450
    assert _delay("full") == _macro("OSEINK_BUSY_FULL_MS") == 2000
    assert _delay("full") > _delay("quality") > _delay("fast")
    assert _macro("OSEINK_PARTIAL_BUDGET") == 4
    assert _macro("OSEINK_FAST_RECT_DIV") == 8


def test_hook_publishes_gray8_without_a_waveform_api():
    source = SOURCE.read_text(encoding="utf-8")
    patch = PATCH.read_text(encoding="utf-8")
    external = (ROOT / "distro" / "external.mk").read_text(encoding="utf-8")
    desktop = (ROOT / "distro" / "overlay" / "bin" / "oseink-desktop").read_text(encoding="utf-8")
    assert "finish_frame_timer" in patch
    assert "oseink_refresh_publish" in patch
    assert "sleep(" not in patch
    assert "oseink-refresh.c" in patch
    assert "oseink-refresh.c" in external
    assert "oseink-refresh.h" in external
    assert 'BR2_GLOBAL_PATCH_DIR="$(BR2_EXTERNAL_OSEINK_PATH)/patches"' in DEFCONFIG.read_text(
        encoding="utf-8"
    )
    assert "damage.json" not in desktop
    assert "oseink-damage" not in desktop
    for name in ("waveform", "EBC_SEND", "ioctl"):
        assert name not in source
        assert name not in patch
    for blob in ("pvi_waveform_v8.S", "rkf_waveform_v8.S", "ht_ebc.px"):
        assert not list((ROOT / "distro").rglob(blob))
        assert not list((ROOT / "emulator").rglob(blob))


def test_refresh_modes_and_gray_frame(tmp_path):
    binary = tmp_path / "oseink-refresh-test"
    errors = []
    for cc in ("gcc", "clang", "cc"):
        if shutil.which(cc) is None:
            continue
        result = subprocess.run(
            [
                cc,
                "-Wall",
                "-Wextra",
                "-D",
                "OSEINK_REFRESH_MAIN",
                "-O2",
                "-o",
                str(binary),
                str(SOURCE),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode == 0:
            break
        errors.append(f"{cc}: {result.stderr}")
        binary.unlink(missing_ok=True)
    else:
        raise AssertionError("refresh self-check did not compile\n" + "\n".join(errors))

    ran = subprocess.run([str(binary)], capture_output=True, text=True, check=False)
    assert ran.returncode == 0, ran.stdout + ran.stderr
    assert "ok: mode from rect, time, and budget" in ran.stdout
    assert "ok: gray8 frame" in ran.stdout


def test_headless_patch_applies():
    headless = WESTON / "libweston" / "backend-headless" / "headless.c"
    if not headless.is_file():
        return
    already = "oseink_damage_busy" in headless.read_text(encoding="utf-8")
    with tempfile.TemporaryDirectory() as tmp:
        tree = Path(tmp)
        dest = tree / "libweston" / "backend-headless"
        dest.mkdir(parents=True)
        (dest / "headless.c").write_bytes(headless.read_bytes())
        meson = WESTON / "libweston" / "backend-headless" / "meson.build"
        (dest / "meson.build").write_bytes(meson.read_bytes())
        command = ["patch", "-p1", "--dry-run", "-i", str(PATCH)]
        if already:
            command.insert(1, "-R")
        result = subprocess.run(
            command,
            cwd=tree,
            capture_output=True,
            text=True,
            check=False,
        )
        assert result.returncode == 0, result.stdout + result.stderr
