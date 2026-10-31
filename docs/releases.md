# Builds and releases

One repository, two version streams:

- Server: `server-v0.1.0`, container `ghcr.io/rjyates/shelf-server:0.1.0`.
- Mobile: `mobile-v0.1.0`, shared Android/iPhone version with a CI build number.

The examples describe the first release; an image is available only once its release has completed.

## Pull requests and main

Use Conventional Commits for every commit and PR title. PR semantic validation, release-policy tests, and all selected build/test jobs must pass `ci-gate`. Main requires a PR and current checks. Squash merging preserves the validated title as the release commit.

| Changed paths | Jobs |
| --- | --- |
| `server/`, `web/`, `deploy/`, Compose, Rust toolchain | Server tests and, in parallel, amd64 and arm64 Docker builds and smoke tests |
| `mobile/` application, platform files, assets, dependencies, or `.flutter-version` | Mobile analysis/tests, Android build, macOS iOS build |
| `protocol/`, shared CI scripts/tests/workflows | Both components and both platforms |
| Root/project documentation | Semantic checks and CI-policy tests; no artifacts or version bump |

## Version calculation

Only commits affecting a component contribute to its version and changelog. `feat` produces a minor; a `!` marker or `BREAKING CHANGE:` footer produces a major. Every other change to a shipped component produces a patch, including refactors and dependency updates. The first release of each component is `0.1.0`.

A PR title must retain the highest release impact of its commits: use `feat` when it contains a feature, and `!` when any commit declares a breaking change. CI enforces this before squash merging so commit footers cannot be lost. Prefer focused component PRs; a mixed-component squash commit applies its release impact to each affected component.

Publication compares with the latest reachable component tag so an earlier failed or skipped release is not lost. Main runs are serialized. Published versions are immutable; retries leave existing GitHub releases unchanged. A server-only change skips the mobile jobs on purpose. Publish still runs, because a skipped job would otherwise stop every later job in that chain.

## Artifacts

The server workflow exports the amd64 and arm64 images it tested. The publishing job loads those images and pushes one multi-architecture index for the exact-version, major/minor, and latest tags. The release contains a pinned Compose file, image digests and source revision, a generated `CHANGELOG.md`, and `SHA256SUMS`.

GitHub initially creates GHCR packages as private even when the repository is public. After the first image push, set the `shelf-server` package visibility to public in GitHub's package settings. The workflow checks anonymous image access before announcing a server release and fails with instructions if the package is still private. Rerun that main workflow after changing visibility. This is a one-time package setting, not a requirement for people installing Shelf.

Every mobile release attaches a development-signed APK and an unsigned iPhone IPA with the same semantic version, plus the generated changelog and checksums. Even a platform-specific mobile change advances this shared mobile version; it leaves the server version unchanged. The IPA uses the normal `Payload` layout, so Sideloadly, AltStore, or Xcode can re-sign it with an Apple ID and install it for development. CI does not sign it, so iOS will not install the file as-is and it cannot be submitted to App Store Connect. Signing keys and TestFlight/App Store/Play publishing are not configured.

The standard source archives are associated with each GitHub tag. Changelogs are release assets and release descriptions, not bot commits to protected main.
