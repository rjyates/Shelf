# Emulator MCP

MCP server so **any** AI agent can see and drive the e-ink panel on the Docker emulator. One launch script: `tools/emulator-mcp/run.sh` (installs deps on first run).

When debugging the panel or guest, call `panel_screenshot` before guessing from logs. The picture on `:8006` is the guest damage frame (`/guest/damage.json` and its gray8 pixels). That page covers the QEMU VGA console. Call `display_screenshot` for the framebuffer and `console_read` for tty0 and the serial tty.

Live tools need the qemu container (`emulator_up`, or `docker compose -p oseink up -d --build --force-recreate` from `emulator/`). They talk to Docker / `:8006` only — no host overlay bypass.

## Host setup

| Host | Config (committed) | Notes |
| --- | --- | --- |
| Cursor | `.cursor/mcp.json` | Reload MCP servers after clone |
| Claude Code | `.mcp.json` | Or: `claude mcp add --transport stdio oseink-emulator -- ./tools/emulator-mcp/run.sh` |
| VS Code / Copilot | `.vscode/mcp.json` | Enable MCP in the project |
| Windsurf / Continue / others | point stdio at `./tools/emulator-mcp/run.sh` | Same env vars as below |
| Claude Desktop | see snippet | Absolute path to `run.sh` |

Claude Desktop `claude_desktop_config.json` fragment:

```json
{
  "mcpServers": {
    "oseink-emulator": {
      "command": "/ABS/PATH/TO/oseink/tools/emulator-mcp/run.sh",
      "env": {
        "OSEINK_VIEWER_URL": "http://127.0.0.1:8006/",
        "OSEINK_COMPOSE_PROJECT": "oseink"
      }
    }
  }
}
```

Check: `node tools/emulator-mcp/selfcheck.mjs`

Env: `OSEINK_VIEWER_URL` (default `http://127.0.0.1:8006/`), `OSEINK_COMPOSE_PROJECT` (default `oseink`).

## Tools

| Tool | Job |
| --- | --- |
| `panel_screenshot` | PNG of the live `:8006` e-ink viewer (primary for the panel) |
| `display_screenshot` | PNG of the live QEMU VGA display, plus decoded tty0 text |
| `console_read` | Text of tty0 (VGA console) and ttyS0 (serial). `which` is `all`, `vga`, or `ttyS0` |
| `guest_type` | Click panel, type into the VGA console, return the e-ink screenshot |
| `viewer_full_refresh` | Click Full refresh, return screenshot |
| `panel_status` | Container + viewer health |
| `emulator_up` / `emulator_down` | Compose up/down |
| `emulator_logs` / `emulator_ps` | Logs / process list |
| `overlay_touch` | Touch `/shared/…` so `oseink-devd` reloads the shell |
| `damage_publish` | Write a quality/fast/full test frame in the container |
| `damage_render` | Render `:8006/guest/` damage through the panel simulator |

## Agent rule

If the task touches the emulator panel, guest UI, or e-ink modes, use `panel_screenshot` (and `guest_type` when you need input). If it touches the VGA console or a tty, use `display_screenshot` and `console_read`. Do not rely on logs alone.
