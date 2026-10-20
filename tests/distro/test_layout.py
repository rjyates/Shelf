from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DISTRO = ROOT / "distro"


def test_external_tree_shape():
    assert (DISTRO / "external.desc").is_file()
    assert (DISTRO / "Config.in").is_file()
    assert (DISTRO / "external.mk").is_file()
    assert (DISTRO / "configs" / "oseink_x86_64_defconfig").is_file()
    assert (DISTRO / "board" / "oseink" / "x86_64" / "linux.config").is_file()
    assert (DISTRO / "overlay" / "etc" / "init.d" / "S40framebuffer").is_file()


def test_defconfig_targets_framebuffer_console():
    text = (DISTRO / "configs" / "oseink_x86_64_defconfig").read_text(encoding="utf-8")
    assert 'BR2_TARGET_GENERIC_GETTY_PORT="tty0"' in text
    assert "BR2_ROOTFS_OVERLAY=" in text
    assert "linux.config" in text


def test_kernel_config_enables_fb():
    text = (DISTRO / "board" / "oseink" / "x86_64" / "linux.config").read_text(
        encoding="utf-8"
    )
    assert "CONFIG_FB=y" in text
    assert "CONFIG_FRAMEBUFFER_CONSOLE=y" in text
    assert "CONFIG_DRM_BOCHS=y" in text


def test_one_command_build_and_host_run_scripts_exist():
    build = DISTRO / "build.sh"
    run = DISTRO / "run.sh"
    assert build.is_file() and build.stat().st_mode & 0o111
    assert run.is_file() and run.stat().st_mode & 0o111
    assert "oseink_x86_64_defconfig" in build.read_text(encoding="utf-8")
    assert "qemu-system-x86_64" in run.read_text(encoding="utf-8")
