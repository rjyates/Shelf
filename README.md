# Shelf

A self-hosted Rust book server with a dark academia web interface and one Flutter/Dart app for Android and iPhone.

Shelf supports account setup, a persistent catalogue, manual page progress, and EPUB importing and reading with saved positions. EPUB support is new since v0.1.0; build this branch from source until its server and mobile releases are published. Book Orbit is the functional reference; other readers, audio, offline access, and external sync remain on the [feature roadmap](docs/parity.md). See the [approved v3 design](docs/design/v3.md).

## Run with Docker

Copy this into `compose.yaml`:

```yaml
services:
  shelf:
    image: ghcr.io/rjyates/shelf-server:latest
    ports:
      - "127.0.0.1:8080:8080"
    volumes:
      - shelf-data:/data
      - ./books:/books:ro
    restart: unless-stopped

volumes:
  shelf-data:
```

Run:

```sh
docker compose up -d
```

Open [localhost:8080](http://localhost:8080) and create your account. There is no default password, database service, or required environment file. `/data` stores the SQLite database, sessions, imported EPUB bytes, covers, and saved reader positions. Back up this volume. A host directory works in place of the named volume, for example `./data:/data`. A root-owned folder is given to uid 10001 on startup. A folder you already own is left alone, and the server runs as that user. The optional books mount is reserved for future folder scanning; it is not automatically imported.

For repeatable installations, use the version-pinned Compose file attached to a [server release](https://github.com/rjyates/Shelf/releases). The published image includes Linux amd64 and arm64.

### Build the unreleased source

To try changes that have not been released, build locally from this repository:

```sh
docker build -f deploy/Dockerfile -t shelf-local .
docker run --rm -p 127.0.0.1:8080:8080 -v shelf-data:/data shelf-local
```

### Access from a phone

The quick start binds to loopback for private setup. To connect a phone, complete account setup locally, then put the server behind an HTTPS reverse proxy and enter that HTTPS address in the app. Set `SHELF_SECURE_COOKIE=true` on the server when using HTTPS. The mobile app rejects remote plain HTTP and never disables certificate validation. Do not expose initial setup on a public address.

The foundation supports one account. Sessions expire after seven days; signing out revokes that account's sessions on all devices. Mobile sessions stay in memory and require sign-in after the app closes. Sign-in attempts are limited to twenty per minute across the server. Multiple users, persisted mobile sessions, and device management are later milestones.

## Import and read EPUBs

In Library, choose **Import EPUBs** and select your files. On mobile, use the import icon and your phone's document picker. Exact duplicates reuse the existing entry without resetting progress. Title, author, and an available JPEG/PNG cover come from the EPUB; no metadata provider receives the file.

Choose **Read book** to open the shared reader. Use the chapter menu, Previous/Next, arrow keys, and text-size selector. The reader saves an EPUB CFI and percentage to your account; reopening a book restores that position, including across online devices. Page totals remain for manually added books; EPUB percentages do not invent printed page counts. Update older mobile clients before using an EPUB library.

Uploads are limited to 50 MiB, with bounded archive resources. Encrypted/obfuscated resources and scripted interactivity are unsupported. Book scripts, popups, external links, and external resources are blocked in the reader. Location generation happens when opening a book and can take a moment. Offline reading, annotations, and conflict-aware synchronization remain planned.

## Develop and test

The server toolchain is pinned in `rust-toolchain.toml`; Flutter is pinned in `.flutter-version`.

```sh
cargo run --manifest-path server/Cargo.toml
cargo fmt --manifest-path server/Cargo.toml --check
cargo clippy --manifest-path server/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path server/Cargo.toml --locked
python3 -m unittest discover -s tests -v
```

From `mobile/`:

```sh
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --fatal-infos
flutter test
flutter run
```

Enable the local commit hook with `git config core.hooksPath .githooks`. CI enforces Conventional Commit subjects and PR titles independently of local hooks.

## Releases

- **Server:** `server-vX.Y.Z`, Docker image, pinned Compose file, image digest/source manifest, and generated changelog.
- **Mobile:** `mobile-vX.Y.Z`, Android/iPhone artifacts and a separate generated changelog.

Every shipped component change releases at least a patch; features produce a minor and breaking changes a major. The first release starts at `0.1.0`. A server-only change does not build or release mobile. Shared API changes check both. Releases publish from main only after semantic checks, relevant tests, and relevant builds pass. See [release policy](docs/releases.md).

Android artifacts initially use development signing. iOS builds run on GitHub macOS runners and produce an unsigned IPA. Install it with a sideload tool that re-signs the app using your Apple ID. Store signing and publishing are deferred.

## Project references

- [Architecture decisions](docs/decisions/0001-rust-server-and-shared-flutter-mobile.md)
- [API contract](protocol/openapi.json)
- [Feature parity and next milestones](docs/parity.md)
- [Build and release decisions](docs/decisions/0002-component-releases-and-selective-ci.md)

Shelf's original code is licensed under AGPL-3.0-only. Book Orbit is credited as a functional reference; its implementation is not included in this foundation. Bundled fonts retain their Open Font License notices. Reader dependencies retain the notices in `web/vendor/`; see [the shared reader decision](docs/decisions/0003-shared-epub-reader.md).
