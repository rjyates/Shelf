# oseink

Linux image for an e-ink device: a default reader/notes session, a full desktop when you want it, plus a self-hosted book service (`shelf`, not in-tree yet). Session model: [docs/product.md](docs/product.md).

Docs: [docs/product.md](docs/product.md) · [docs/emulator.md](docs/emulator.md) · [AGENTS.md](AGENTS.md)

## Emulator

Needs [Docker](https://docs.docker.com/get-docker/) and KVM (`/dev/kvm`).

```
cd emulator && docker compose -p oseink up -d --build --force-recreate
```

Stop it with:

```
cd emulator && docker compose -p oseink down
```

`-p oseink` is the Compose project the emulator MCP uses. Panel: [http://127.0.0.1:8006](http://127.0.0.1:8006)

The userspace shell and assets under `distro/overlay/` reload while the VM stays up. A kernel or initramfs change boots a new image (`./distro/build.sh`, then compose again).

## Tests

```
poetry install && poetry run pytest
```
