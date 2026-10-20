#!/usr/bin/env bash
# Boot the built guest with host QEMU (outside Docker). Needs qemu-system-x86_64.
# Shares distro/overlay into the guest at /oseink (virtiofs if virtiofsd exists, else 9p).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMG="$ROOT/.build/oseink/images"
KERNEL="$IMG/bzImage"
ROOTFS="$IMG/rootfs.ext2"
SHARE="$ROOT/distro/overlay"
TAG=shared

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

virtiofsd_bin=""
if command -v virtiofsd >/dev/null; then
	virtiofsd_bin="$(command -v virtiofsd)"
elif [[ -x /usr/lib/virtiofsd ]]; then
	virtiofsd_bin=/usr/lib/virtiofsd
elif [[ -x /usr/libexec/virtiofsd ]]; then
	virtiofsd_bin=/usr/libexec/virtiofsd
fi

share_args=()
VIRTIOFSD_PID=""
VIRTIOFS_SOCK=""

cleanup() {
	if [[ -n "$VIRTIOFSD_PID" ]]; then
		kill "$VIRTIOFSD_PID" 2>/dev/null || true
		wait "$VIRTIOFSD_PID" 2>/dev/null || true
	fi
	if [[ -n "$VIRTIOFS_SOCK" ]]; then
		rm -f "$VIRTIOFS_SOCK"
	fi
}
trap cleanup EXIT

if [[ -n "$virtiofsd_bin" ]]; then
	VIRTIOFS_SOCK="$(mktemp -u /tmp/oseink-virtiofs.XXXXXX.sock)"
	"$virtiofsd_bin" --socket-path="$VIRTIOFS_SOCK" --shared-dir="$SHARE" --cache=auto --sandbox=none &
	VIRTIOFSD_PID=$!
	for _ in $(seq 1 50); do
		[[ -S "$VIRTIOFS_SOCK" ]] && break
		sleep 0.1
	done
	if [[ ! -S "$VIRTIOFS_SOCK" ]]; then
		echo "virtiofsd did not create $VIRTIOFS_SOCK; falling back to 9p" >&2
		kill "$VIRTIOFSD_PID" 2>/dev/null || true
		wait "$VIRTIOFSD_PID" 2>/dev/null || true
		VIRTIOFSD_PID=""
		rm -f "$VIRTIOFS_SOCK"
		VIRTIOFS_SOCK=""
		share_args+=(-virtfs "local,path=$SHARE,mount_tag=$TAG,security_model=none,id=oseink")
	else
		# memfd + numa is required for vhost-user-fs; size matches -m below.
		share_args+=(
			-object "memory-backend-memfd,id=mem,size=512M,share=on"
			-numa "node,memdev=mem"
			-chardev "socket,id=char0,path=$VIRTIOFS_SOCK"
			-device "vhost-user-fs-pci,queue-size=1024,chardev=char0,tag=$TAG"
		)
	fi
else
	share_args+=(-virtfs "local,path=$SHARE,mount_tag=$TAG,security_model=none,id=oseink")
fi

drive="file=$ROOTFS,if=virtio,format=raw"
# -vga std + console=tty0: fullscreen framebuffer console in the QEMU window.
# Pass DISPLAY_MODE=serial for a headless serial-only check.
if [[ "${DISPLAY_MODE:-graphical}" == "serial" ]]; then
	drive+=",snapshot=on"
	qemu-system-x86_64 \
		"${extra[@]}" \
		-m 512 \
		-kernel "$KERNEL" \
		-drive "$drive" \
		-append "root=/dev/vda console=ttyS0 console=tty0" \
		-vga std \
		-display none \
		-serial stdio \
		-no-reboot \
		"${share_args[@]}"
	exit $?
fi

qemu-system-x86_64 \
	"${extra[@]}" \
	-m 512 \
	-kernel "$KERNEL" \
	-drive "$drive" \
	-append "root=/dev/vda console=tty0 console=ttyS0" \
	-vga std \
	-serial mon:stdio \
	"${share_args[@]}"
exit $?
