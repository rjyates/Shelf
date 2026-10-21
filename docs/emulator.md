# Emulator

`:8006` is the e-ink **display** attached to the Buildroot guest — the web stand-in for the panel the OS will drive. The guest writes a damage frame, and the page paints it through e-ink simulation (quantize, delay, ghosting, full-refresh flash). Click the panel to type into the guest console.

Issues 1–8 are done. Day-to-day run path is in [README.md](../README.md). See [AGENTS.md](../AGENTS.md) for what comes next.

## Build and run

Entry point (Docker + KVM):

```
cd emulator && docker compose -p oseink up -d --build --force-recreate
```

Stop: `cd emulator && docker compose -p oseink down`

Open [http://127.0.0.1:8006](http://127.0.0.1:8006). Click the panel to focus the guest console; **Full refresh** reapplies the current frame as `full` and clears ghosting. Keys `1` / `2` / `3` reapply that frame as `quality` / `fast` / `full`.

`./distro/build.sh` writes images under `.build/oseink/images/` (needed before the first compose, and again for a kernel or initramfs change). `./distro/run.sh` boots the same image on the host. Compose and `run.sh` share `distro/overlay/` into the guest at `/oseink`.

The userspace shell and assets under `distro/overlay/` reload while the VM stays up. `distro/overlay/run/profile.json` is the capability profile the guest reads at `/oseink/run/profile.json` when the shell starts. The `:8006` page sets color and interaction separately. Color is black and white or color. Interaction is any combination of button, touch, and stylus. Choosing one writes that file and `distro/overlay/share/profile.stamp`. `oseink-devd` restarts the shell when share content changes, so the guest reads the new profile without a new image. Compose does not set the profile.

## How the panel gets pixels

`oseink-shell` is the session entry. BusyBox init starts `oseink-devd`, which starts that shell. The shell runs `/oseink/etc/init.d/S60oseink-session`, and the script execs the presenter named in `/oseink/run/boot-target`: `reader` (default) or `desktop`. The reader presenter publishes one `quality` frame (`share/demo/page.gray`). The desktop presenter starts Weston 14 with the pixman renderer, desktop-shell, and the headless backend, sized from the profile, and launches weston-terminal. It starts seatd for that session and does not publish a damage frame, so `:8006` does not show Weston pixels until the repaint hook. `oseink-damage` writes `/oseink/run/damage.json` and `frame.gray` (a v1 message: `quality`, `fast`, or `full`, a rect, and gray8 pixels). Compose serves that directory at `/guest/`. The viewer polls it and applies the frame with `emulator/viewer/damage.js` (delay, ghosting, full-refresh flash). The picture is that frame. The mode delays in that file are still the placeholder (200ms, 35ms, 140ms). The busy times to install are in [product.md](product.md) under Panel timing. VNC stays connected so a click still reaches the VGA console. The e-ink page covers that console; `display_screenshot` and `console_read` read the framebuffer and the serial tty. Apps do not open a panel device. MCP can write the same files; see [emulator-mcp.md](emulator-mcp.md).

Writing `reader` or `desktop` into the boot-target file restarts the presenter. The guest command is `/oseink/etc/init.d/S60oseink-session restart reader` (or `desktop`). The image is not rebuilt and the VM is not rebooted. Desktop repaint is a later phase.

`oseink-devd` restarts `/oseink/bin/oseink-shell` when that binary, `/oseink/share/`, `/oseink/run/profile.json`, or `/oseink/run/boot-target` changes. A boot-target write is also noticed by the running presenter, which exits when the file no longer names it. The profile picker also updates `profile.stamp` under share. Kernel or initramfs changes need a new build. Session modes (reader/notes vs desktop) are defined in [product.md](product.md).

## Agent MCP

Any AI host: [emulator-mcp.md](emulator-mcp.md) (`panel_screenshot` for the e-ink page, `display_screenshot` and `console_read` for the VGA console and serial tty, `guest_type`, compose up/down). Launch: `tools/emulator-mcp/run.sh`.
