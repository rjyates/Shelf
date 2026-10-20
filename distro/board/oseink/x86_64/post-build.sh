#!/bin/sh
# Keep a serial getty for headless checks. tty0 is the Buildroot getty (framebuffer).
set -eu
TARGET="$1"
if ! grep -q 'ttyS0' "$TARGET/etc/inittab"; then
	printf '%s\n' 'ttyS0::respawn:/sbin/getty -L ttyS0 115200 vt100' >>"$TARGET/etc/inittab"
fi
