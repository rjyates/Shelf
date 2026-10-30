# Shelf working agreement

Shelf is a Rust server with an embedded web frontend and a shared Flutter/Dart Android and iPhone client. This repository was deliberately reset before this foundation; do not restore the old device OS code without an explicit request.

## Implementation

- Use feature branches and Conventional Commit subjects and PR titles: `type(scope): description`.
- Prefer small complete changes, existing components, and meaningful tests. Use the ponytail skill for coding and Picasso for UI work when available.
- Follow `.picasso.md` and `docs/design/v3.md`. Version 3 is approved; preserve its palette and layout direction.
- Book Orbit is the functional reference, not an assertion of implemented parity. Update `docs/parity.md` whenever a capability becomes usable.
- Keep private paths, books, credentials, generated signing keys, local databases, and SDK/build caches out of Git.
- Do not provision paid services or buy accounts. Store signing and store publishing remain deferred.

## Verification

- Server: `cargo fmt --manifest-path server/Cargo.toml --check`, `cargo clippy --manifest-path server/Cargo.toml --locked --all-targets -- -D warnings`, `cargo test --manifest-path server/Cargo.toml --locked`.
- Mobile, from `mobile/`: `flutter pub get --enforce-lockfile`, `dart format --output=none --set-exit-if-changed lib test`, `flutter analyze --fatal-infos`, `flutter test`.
- CI policy: `python3 -m unittest discover -s tests -v`.
- The GitHub workflow also validates Compose, builds and smoke-tests the container on Linux amd64 and arm64, builds Android, and builds an unsigned iPhone IPA on macOS.

## Releases

Server tags are `server-vX.Y.Z`; shared mobile tags are `mobile-vX.Y.Z`. Never use the historical `vX.Y.Z` series for either. Each changed shipped component releases independently after the required `ci-gate` passes on main. All component changes release at least a patch; `feat` produces a minor and breaking changes a major. A first release starts at `0.1.0`.

Changelogs are generated from component-relevant commits and attached to their GitHub release. Do not hand-edit a generated changelog or have a bot commit it directly to protected main. Publishing uses the exact tested image and mobile artifacts. Preserve the stable required `ci-gate` job even when component jobs are skipped.

Keep README, architecture, parity status, and API documentation consistent with the delivered behavior. Never report builds or workflows as passing until their results are verified.
