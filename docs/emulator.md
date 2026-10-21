# Emulator

`:8006` is the e-ink **display** attached to the Buildroot guest — the web stand-in for the panel the OS will drive. You see the guest framebuffer through e-ink simulation (quantize, delay, ghosting, full-refresh flash) and type into the guest by clicking the panel.

Issues 1–8 are done. Day-to-day run path is in [README.md](../README.md). See [AGENTS.md](../AGENTS.md) for what comes next.

## Build and run

Entry point (Docker + KVM):

```
cd emulator && docker compose -p oseink up -d --build --force-recreate
```

Stop: `cd emulator && docker compose -p oseink down`

Open [http://127.0.0.1:8006](http://127.0.0.1:8006). Click the panel to focus the guest; **Full refresh** clears ghosting. Keys `1` / `2` / `3` set refresh class `quality` / `fast` / `full` while developing.

`./distro/build.sh` writes images under `.build/oseink/images/` (needed before the first compose, and again for a kernel or initramfs change). `./distro/run.sh` boots the same image on the host. Compose and `run.sh` share `distro/overlay/` into the guest at `/oseink`.

The userspace shell and assets under `distro/overlay/` reload while the VM stays up. `distro/overlay/run/profile.json` is the capability profile the guest reads at `/oseink/run/profile.json` when the shell starts. The `:8006` page sets color and interaction separately. Color is black and white or color. Interaction is any combination of button, touch, and stylus. Choosing one writes that file and `distro/overlay/share/profile.stamp`. `oseink-devd` restarts the shell when share content changes, so the guest reads the new profile without a new image. Compose does not set the profile.

## How the panel gets pixels

Today the guest console is on the VGA framebuffer. The viewer pulls that over VNC and runs it through `emulator/viewer/panel.js`. That is a temporary bridge: the long-term path is the damage protocol (`quality` / `fast` / `full` + rect + pixels) in `damage.js`, which both the reader/notes session and the desktop session use so ordinary Linux drawing still hits e-ink safeguards. MCP can still publish test damage frames; see [emulator-mcp.md](emulator-mcp.md).

`oseink-devd` restarts `/oseink/bin/oseink-shell` when that binary, `/oseink/share/`, or `/oseink/run/profile.json` changes. The devd in the current image watches the binary and share only, so the profile picker also updates `profile.stamp` under share. Kernel or initramfs changes need a new build. Session modes (reader/notes vs desktop) are defined in [product.md](product.md).

## Agent MCP

Any AI host: [emulator-mcp.md](emulator-mcp.md) (`panel_screenshot`, `guest_type`, compose up/down). Launch: `tools/emulator-mcp/run.sh`.
