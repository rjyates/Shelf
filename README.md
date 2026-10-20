# oseink

Linux image for an e-ink reader, plus a self-hosted book service (`shelf`, not in-tree yet).

Docs: [docs/product.md](docs/product.md) · [docs/emulator.md](docs/emulator.md) · [AGENTS.md](AGENTS.md)

```
./distro/build.sh
cd emulator && docker compose up
```

Panel: [http://127.0.0.1:8006](http://127.0.0.1:8006)

```
poetry install && poetry run pytest
```
