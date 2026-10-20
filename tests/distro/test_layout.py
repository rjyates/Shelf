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
    share = DISTRO / "overlay" / "etc" / "init.d" / "S30share"
    assert share.is_file() and share.stat().st_mode & 0o111
    devd_init = DISTRO / "overlay" / "etc" / "init.d" / "S50oseink-devd"
    assert devd_init.is_file() and devd_init.stat().st_mode & 0o111
    devd = DISTRO / "overlay" / "usr" / "sbin" / "oseink-devd"
    assert devd.is_file() and devd.stat().st_mode & 0o111
    shell = DISTRO / "overlay" / "bin" / "oseink-shell"
    assert shell.is_file() and shell.stat().st_mode & 0o111
    assert (DISTRO / "overlay" / "share" / "banner").is_file()


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


def test_kernel_config_enables_virtiofs_and_9p():
    text = (DISTRO / "board" / "oseink" / "x86_64" / "linux.config").read_text(
        encoding="utf-8"
    )
    assert "CONFIG_VIRTIO_FS=y" in text
    assert "CONFIG_FUSE_FS=y" in text
    assert "CONFIG_NET_9P_VIRTIO=y" in text
    assert "CONFIG_9P_FS=y" in text


def test_one_command_build_and_host_run_scripts_exist():
    build = DISTRO / "build.sh"
    run = DISTRO / "run.sh"
    assert build.is_file() and build.stat().st_mode & 0o111
    assert run.is_file() and run.stat().st_mode & 0o111
    assert "oseink_x86_64_defconfig" in build.read_text(encoding="utf-8")
    run_text = run.read_text(encoding="utf-8")
    assert "qemu-system-x86_64" in run_text
    assert "distro/overlay" in run_text
    assert "mount_tag=shared" in run_text or "tag=$TAG" in run_text


def test_post_build_creates_oseink_mountpoint():
    text = (DISTRO / "board" / "oseink" / "x86_64" / "post-build.sh").read_text(
        encoding="utf-8"
    )
    assert 'mkdir -p "$TARGET/oseink"' in text


def test_share_init_prefers_virtiofs_then_9p():
    text = (DISTRO / "overlay" / "etc" / "init.d" / "S30share").read_text(
        encoding="utf-8"
    )
    assert "mount -t virtiofs" in text
    assert "mount -t 9p" in text
    assert "cache=mmap" in text
    assert "TAG=shared" in text
    assert "MNT=/oseink" in text
    assert "oseink-share:fail" in text


def test_devd_watches_share_and_restarts_shell():
    text = (DISTRO / "overlay" / "usr" / "sbin" / "oseink-devd").read_text(
        encoding="utf-8"
    )
    assert "/oseink/bin" in text
    assert "/oseink/share" in text
    assert "/oseink/bin/oseink-shell" in text
    assert "oseink-devd:reload" in text
    shell = (DISTRO / "overlay" / "bin" / "oseink-shell").read_text(encoding="utf-8")
    assert "/oseink/share/banner" in shell
    assert "/run/oseink-shell" in shell
