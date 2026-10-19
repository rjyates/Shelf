#!/bin/sh
# Fail when the emulator compose contract drifts from the running stack.
set -eu

cd "$(dirname "$0")"

expected_from='FROM qemux/qemu:7.50@sha256:e7f6fda52503a546fd649670ba46e4bc23dc6dcef275bc3fac48877fbbc430df'
actual_from=$(grep -E '^FROM ' Dockerfile)
if [ "$actual_from" != "$expected_from" ]; then
  echo "Dockerfile pin is '$actual_from'" >&2
  echo "expected '$expected_from'" >&2
  exit 1
fi

docker compose config --format json | python3 -c '
import json, sys

doc = json.load(sys.stdin)
services = doc.get("services") or {}
if list(services) != ["qemu"]:
    sys.exit("compose service must be qemu, got %s" % list(services))

svc = services["qemu"]
if not svc.get("build"):
    sys.exit("qemu must build from emulator/Dockerfile")

ports = []
for port in svc.get("ports") or []:
    if isinstance(port, dict):
        ports.append((str(port.get("published")), str(port.get("target"))))
if ("8006", "8006") not in ports:
    sys.exit("port 8006 is not published: %s" % ports)

kvm = False
for device in svc.get("devices") or []:
    if isinstance(device, str) and device.split(":")[0] == "/dev/kvm":
        kvm = True
    elif isinstance(device, dict) and "/dev/kvm" in (device.get("source"), device.get("target")):
        kvm = True
if not kvm:
    sys.exit("container does not receive /dev/kvm")

disk_volume = None
for volume in svc.get("volumes") or []:
    if (
        isinstance(volume, dict)
        and volume.get("type") == "volume"
        and volume.get("target") == "/storage"
    ):
        disk_volume = volume.get("source")
if not disk_volume:
    sys.exit("disks are not on a named volume mounted at /storage")

declared = (doc.get("volumes") or {}).get(disk_volume) or {}
if declared.get("external"):
    sys.exit("disk volume %s is external" % disk_volume)

env = svc.get("environment") or {}
if isinstance(env, list):
    env = dict(item.split("=", 1) for item in env)
if env.get("BOOT") != "alpine":
    sys.exit("BOOT must be the alpine ISO, got %r" % env.get("BOOT"))

grace = svc.get("stop_grace_period")
if grace not in ("2m", "2m0s", "120s"):
    sys.exit("stop_grace_period must be 2m so QEMU can flush disks, got %r" % grace)
'

echo "emulator contract ok"
