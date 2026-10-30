# Builds and releases

One repository, two version streams:

- Server: `server-v0.1.0`, container `ghcr.io/rjyates/shelf-server:0.1.0`.
- Mobile: `mobile-v0.1.0`, shared Android/iPhone version with a CI build number.

The examples describe the first release; an image is available only once its release has completed.

## Pull requests and main

Use Conventional Commits for every commit and PR title. PR semantic validation, release-policy tests, and all selected build/test jobs must pass `ci-gate`. Main requires a PR and current checks. Squash merging preserves the validated title as the release commit.

| Changed paths | Jobs |
| --- | --- |
| `server/`, `web/`, `deploy/`, Compose, Rust toolchain | Server tests, web syntax, Compose validation, Docker build and smoke test |
| Shared `mobile/` application, assets, or dependencies | Mobile analysis/tests, Android build, macOS iOS build |
| `mobile/android/` only | Mobile analysis/tests and Android build |
| `mobile/ios/` only | Mobile analysis/tests and macOS iOS build |
| `protocol/`, shared CI scripts/tests/workflows | Both components and both platforms |
| Root/project documentation | Semantic checks and CI-policy tests; no artifacts or version bump |

## Version calculation

Only commits affecting a component contribute to its version and changelog. `feat` produces a minor; a `!` marker or `BREAKING CHANGE:` footer produces a major. Every other change to a shipped component produces a patch, including refactors and dependency updates. The first release of each component is `0.1.0`.

Publication compares with the latest reachable component tag so an earlier failed or skipped release is not lost. Main runs are serialized. Published versions are immutable; retries leave existing GitHub releases unchanged.

## Artifacts

The server workflow exports the image it tested. The publishing job loads that image and pushes exact-version, major/minor, and latest tags. The release contains a pinned Compose file, image digests and source revision, a generated `CHANGELOG.md`, and `SHA256SUMS`. The initial image targets Linux amd64; ARM images are not yet built.

GitHub initially creates GHCR packages as private even when the repository is public. After the first image push, set the `shelf-server` package visibility to public in GitHub's package settings. The workflow checks anonymous image access before announcing a server release and fails with instructions if the package is still private. Rerun that main workflow after changing visibility. This is a one-time package setting, not a requirement for people installing Shelf.

Mobile releases attach a development-signed APK and/or an unsigned iOS `.app` zip, plus the generated changelog and checksums. Platform-only releases attach only the changed platform. The iOS zip is for build verification; it cannot be installed directly or submitted to App Store Connect. Signing keys and TestFlight/App Store/Play publishing are not configured.

The standard source archives are associated with each GitHub tag. Changelogs are release assets and release descriptions, not bot commits to protected main.
