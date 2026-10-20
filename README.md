# oseink

oseink will be a Linux image for an e-ink reader, and a book service named shelf that you host yourself. Shelf is not in the repo yet. The guest image definition lives in `distro/`.

The current work is the emulator, issues [1](https://github.com/rjyates/oseink/issues/1) through [8](https://github.com/rjyates/oseink/issues/8), tracked on the [oseink board](https://github.com/users/cayubweeums/projects/3). That board is on the `cayubweeums` account. GitHub would not link it to [rjyates/oseink](https://github.com/rjyates/oseink).

Read [AGENTS.md](AGENTS.md) before changing the repo. Claude Code gets there through [CLAUDE.md](CLAUDE.md). After those issues, the spec is [docs/product.md](docs/product.md).

## Guest image

```
./distro/build.sh
./distro/run.sh
```

`build.sh` fetches Buildroot and builds the x86_64 kernel plus rootfs under `.build/`. `run.sh` boots that image with host QEMU outside Docker and shares `distro/overlay/` into the guest at `/oseink`. From `emulator/`, `docker compose up` boots the same images in the browser (no ISO download) with the same overlay share. Open [http://127.0.0.1:8006](http://127.0.0.1:8006) for the e-ink panel view (ghosting on updates, full refresh clears it). Guest `oseink-devd` restarts the stub shell when you edit `/oseink/bin/oseink-shell` or files under `/oseink/share/` on the host; the VM stays up. A kernel or initramfs change still needs a new build.

## Tests

```
curl -sSL https://install.python-poetry.org | python3 -
poetry install && poetry run pytest
```
