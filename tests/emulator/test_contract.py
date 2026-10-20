from pathlib import Path

from tests.emulator.conftest import QEMU_IMAGE, ROOT


def test_image_pin(emulator_dir):
    pinned = [
        line.strip()
        for line in (emulator_dir / "Dockerfile").read_text(encoding="utf-8").splitlines()
        if line.startswith("FROM ")
    ]
    assert pinned == [f"FROM {QEMU_IMAGE}"]


def test_service_builds_local_dockerfile(compose_config):
    services = compose_config["services"]
    assert list(services) == ["qemu"]
    assert services["qemu"]["build"]


def test_publishes_the_browser_console(compose_config):
    ports = [
        (str(port.get("published")), str(port.get("target")))
        for port in compose_config["services"]["qemu"]["ports"]
    ]
    assert ("8006", "8006") in ports


def test_passes_kvm_into_the_guest(compose_config):
    devices = compose_config["services"]["qemu"]["devices"]
    assert any(
        device.get("source") == "/dev/kvm" or device.get("target") == "/dev/kvm"
        for device in devices
    )


def test_keeps_disks_on_a_named_volume(compose_config):
    volumes = compose_config["services"]["qemu"]["volumes"]
    disk = next(
        volume
        for volume in volumes
        if volume.get("type") == "volume" and volume.get("target") == "/storage"
    )
    assert not compose_config["volumes"][disk["source"]].get("external")


def test_boots_the_oseink_rootfs_not_an_iso(compose_config):
    env = compose_config["services"]["qemu"]["environment"]
    assert "BOOT" not in env or env.get("BOOT") in ("", None)
    assert env["DISK_DISABLE"] in ("Y", "y", "true", "1")
    assert env["DISK_TYPE"] == "blk"
    assert "-kernel /oseink/bzImage" in env["ARGUMENTS"]
    assert "root=/dev/vda" in env["ARGUMENTS"]

    mounts = {
        volume.get("target"): volume.get("source")
        for volume in compose_config["services"]["qemu"]["volumes"]
        if volume.get("type") == "bind"
    }
    images = ROOT / ".build" / "oseink" / "images"
    assert Path(mounts["/boot.img"]).resolve() == (images / "rootfs.ext2").resolve()
    assert Path(mounts["/oseink/bzImage"]).resolve() == (images / "bzImage").resolve()


def test_shares_overlay_as_qemux_shared(compose_config):
    mounts = {
        volume.get("target"): volume.get("source")
        for volume in compose_config["services"]["qemu"]["volumes"]
        if volume.get("type") == "bind"
    }
    overlay = ROOT / "distro" / "overlay"
    assert Path(mounts["/shared"]).resolve() == overlay.resolve()


def test_lets_qemu_flush_disks_on_shutdown(compose_config):
    assert compose_config["services"]["qemu"]["stop_grace_period"] in ("2m", "2m0s", "120s")
