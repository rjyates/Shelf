#!/usr/bin/env bash
# Build the oseink x86_64 guest image. One command from the repo root or distro/.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DISTRO="$ROOT/distro"
BUILD="$ROOT/.build"
BR_VER="2025.02.6"
BR_URL="https://buildroot.org/downloads/buildroot-${BR_VER}.tar.xz"
BR_DIR="$BUILD/buildroot-${BR_VER}"
OUT="$BUILD/oseink"
IMAGE="oseink-buildroot:bookworm"

mkdir -p "$BUILD"

if [[ ! -d "$BR_DIR" ]]; then
	echo "downloading Buildroot ${BR_VER}"
	curl -fsSL "$BR_URL" | tar -xJ -C "$BUILD"
fi

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
	docker build -t "$IMAGE" -f "$DISTRO/build.Dockerfile" "$DISTRO"
fi

# BR2_EXTERNAL is resolved relative to the Buildroot source tree, so use an
# absolute path inside the container.
docker run --rm \
	--user "$(id -u):$(id -g)" \
	-e HOME=/tmp \
	-v "$ROOT:/work:rw" \
	-w /work \
	"$IMAGE" \
	bash -c "
		set -euo pipefail
		make -C '${BR_DIR#"$ROOT"/}' O='/work/${OUT#"$ROOT"/}' BR2_EXTERNAL=/work/distro oseink_x86_64_defconfig
		make -C '${BR_DIR#"$ROOT"/}' O='/work/${OUT#"$ROOT"/}' -j\$(nproc)
	"

echo "images: $OUT/images/bzImage $OUT/images/rootfs.ext2"
