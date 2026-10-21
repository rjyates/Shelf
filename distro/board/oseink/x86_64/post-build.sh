#!/bin/sh
# Keep a serial getty for headless checks. tty0 is the Buildroot getty (framebuffer).
# /oseink is the mount point for the host overlay share (issue 4); not part of the share tree.
set -eu
TARGET="$1"
mkdir -p "$TARGET/oseink"
# seatd's packaged init starts the daemon at boot with group video, which this
# image does not have. The desktop presenter starts seatd for its own seat.
rm -f "$TARGET/etc/init.d/S70seatd"
if ! grep -q 'ttyS0' "$TARGET/etc/inittab"; then
	printf '%s\n' 'ttyS0::respawn:/sbin/getty -L ttyS0 115200 vt100' >>"$TARGET/etc/inittab"
fi
