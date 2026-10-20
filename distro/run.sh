#!/usr/bin/env bash
# Boot the built guest with host QEMU (outside Docker). Needs qemu-system-x86_64.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMG="$ROOT/.build/oseink/images"
KERNEL="$IMG/bzImage"
ROOTFS="$IMG/rootfs.ext2"

if [[ ! -f "$KERNEL" || ! -f "$ROOTFS" ]]; then
	echo "missing images; run distro/build.sh first" >&2
	exit 1
fi

if ! command -v qemu-system-x86_64 >/dev/null; then
	echo "install qemu-system-x86_64 to boot outside Docker" >&2
	exit 1
fi

extra=()
if [[ -e /dev/kvm ]]; then
	extra+=(-enable-kvm)
fi

# -vga std + console=tty0: fullscreen framebuffer console in the QEMU window.
# Pass DISPLAY_MODE=serial for a headless serial-only check.
drive="file=$ROOTFS,if=virtio,format=raw"
# Serial smoke checks use a snapshot so the image stays clean.
if [[ "${DISPLAY_MODE:-graphical}" == "serial" ]]; then
	drive+=",snapshot=on"
	exec qemu-system-x86_64 \
		"${extra[@]}" \
		-m 512 \
		-kernel "$KERNEL" \
		-drive "$drive" \
		-append "root=/dev/vda console=ttyS0 console=tty0" \
		-vga std \
		-display none \
		-serial stdio \
		-no-reboot
fi

exec qemu-system-x86_64 \
	"${extra[@]}" \
	-m 512 \
	-kernel "$KERNEL" \
	-drive "$drive" \
	-append "root=/dev/vda console=tty0 console=ttyS0" \
	-vga std \
	-serial mon:stdio
