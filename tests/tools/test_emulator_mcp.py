import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MCP = ROOT / "tools" / "emulator-mcp"


def test_emulator_mcp_layout_and_hosts():
    assert (MCP / "server.mjs").is_file()
    assert (MCP / "run.sh").is_file()
    assert (MCP / "run.sh").stat().st_mode & 0o111
    assert (MCP / "selfcheck.mjs").is_file()
    assert (MCP / "lib" / "viewer.mjs").is_file()
    assert (MCP / "lib" / "docker.mjs").is_file()
    assert (ROOT / "docs" / "emulator-mcp.md").is_file()

    for rel in [".cursor/mcp.json", ".mcp.json", ".vscode/mcp.json"]:
        path = ROOT / rel
        assert path.is_file(), rel
        text = path.read_text(encoding="utf-8")
        assert "oseink-emulator" in text
        assert "emulator-mcp/run.sh" in text

    server = (MCP / "server.mjs").read_text(encoding="utf-8")
    for tool in (
        "panel_screenshot",
        "display_screenshot",
        "console_read",
        "guest_type",
        "viewer_full_refresh",
        "emulator_up",
        "emulator_down",
        "overlay_touch",
        "damage_render",
        "damage_publish",
    ):
        assert f'"{tool}"' in server

    docs = (ROOT / "docs" / "emulator-mcp.md").read_text(encoding="utf-8")
    assert "panel_screenshot" in docs
    assert "display_screenshot" in docs
    assert "console_read" in docs
    assert "Claude Code" in docs
    assert "run.sh" in docs


def test_emulator_mcp_selfcheck():
    result = subprocess.run(
        ["node", str(MCP / "selfcheck.mjs")],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    assert "ok: offline parse/png" in result.stdout
    assert "ok: tools + host configs" in result.stdout or "ok: live panel_screenshot" in result.stdout
