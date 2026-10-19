#!/bin/sh
# Boot the compose stack, confirm the browser viewer and the Alpine console,
# then restart and confirm the disk volume still holds the same image.
# Requires /dev/kvm. Uses its own compose project so a running emulator is left alone.
set -eu

cd "$(dirname "$0")"
./check.sh

if [ ! -e /dev/kvm ]; then
  echo "smoke: /dev/kvm is missing" >&2
  exit 1
fi

project=oseink-smoke
volume="${project}_disks"
image='qemux/qemu:7.50@sha256:e7f6fda52503a546fd649670ba46e4bc23dc6dcef275bc3fac48877fbbc430df'
since=

cleanup() {
  docker compose -p "$project" down -v --remove-orphans >/dev/null 2>&1 || true
}
trap cleanup EXIT

container_id() {
  docker compose -p "$project" ps -q
}

viewer_up() {
  body=$(curl -fsS --max-time 2 http://127.0.0.1:8006/ 2>/dev/null || true)
  printf '%s' "$body" | grep -q '<title>noVNC</title>'
}

guest_up() {
  cid=$(container_id)
  [ -n "$cid" ] || return 1
  if [ -n "$since" ]; then
    docker logs --since "$since" "$cid" 2>&1 | grep -q 'Welcome to Alpine Linux'
  else
    docker logs "$cid" 2>&1 | grep -q 'Welcome to Alpine Linux'
  fi
}

disk_id() {
  docker run --rm --entrypoint stat -v "$volume":/storage "$image" -c '%n %i %s' \
    /storage/alpine/data.img /storage/alpine/boot.iso
}

wait_for() {
  name=$1
  i=0
  while [ "$i" -lt 150 ]; do
    if "$name"; then
      return 0
    fi
    i=$((i + 1))
    sleep 2
  done
  echo "smoke: timed out waiting for $name" >&2
  docker compose -p "$project" logs --tail 40 >&2 || true
  exit 1
}

docker compose -p "$project" up -d --build
wait_for viewer_up
wait_for guest_up
before=$(disk_id)

since=$(date -u +%Y-%m-%dT%H:%M:%SZ)
docker compose -p "$project" restart
wait_for viewer_up
wait_for guest_up
after=$(disk_id)

if [ "$before" != "$after" ]; then
  echo "smoke: disk identity changed across restart" >&2
  printf 'before:\n%s\n' "$before" >&2
  printf 'after:\n%s\n' "$after" >&2
  exit 1
fi

echo "emulator smoke ok"
