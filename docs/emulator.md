# Emulator

`:8006` is the e-ink **display** attached to the Buildroot guest — the web stand-in for the panel the OS will drive. You see the guest framebuffer through e-ink simulation (quantize, delay, ghosting, full-refresh flash) and type into the guest by clicking the panel.

Milestone issues 1–8; see [AGENTS.md](../AGENTS.md) for status.

## Build and run

```
./distro/build.sh
./distro/run.sh          # host QEMU
cd emulator && docker compose up   # browser + KVM
```

Open [http://127.0.0.1:8006](http://127.0.0.1:8006). Click the panel to focus the guest; **Full refresh** clears ghosting. Keys `1` / `2` / `3` set refresh class `quality` / `fast` / `full` while developing.

`build.sh` writes images under `.build/oseink/images/`. Compose and `run.sh` share `distro/overlay/` into the guest at `/oseink`.

## How the panel gets pixels

Today the guest console is on the VGA framebuffer. The viewer pulls that over VNC and runs it through `emulator/viewer/panel.js`. That is a temporary bridge: the long-term path is the damage protocol (`quality` / `fast` / `full` + rect + pixels) in `damage.js`, which a real shell/panel driver will speak. MCP can still publish test damage frames; see [emulator-mcp.md](emulator-mcp.md).

`oseink-devd` restarts `/oseink/bin/oseink-shell` when that binary or `/oseink/share/` changes. Kernel or initramfs changes need a new build.

## Agent MCP

Any AI host: [emulator-mcp.md](emulator-mcp.md) (`panel_screenshot`, `guest_type`, compose up/down). Launch: `tools/emulator-mcp/run.sh`.
