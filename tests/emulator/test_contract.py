from tests.emulator.conftest import QEMU_IMAGE


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


def test_boots_the_alpine_iso_until_our_image_exists(compose_config):
    assert compose_config["services"]["qemu"]["environment"]["BOOT"] == "alpine"


def test_lets_qemu_flush_disks_on_shutdown(compose_config):
    assert compose_config["services"]["qemu"]["stop_grace_period"] in ("2m", "2m0s", "120s")
