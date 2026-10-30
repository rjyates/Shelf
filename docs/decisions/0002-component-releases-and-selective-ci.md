# 0002. Independent server and mobile releases in one repository

- Status: Accepted
- Date: 2026-10-30

## Context

The user requested semantic commits and PRs, automated versions and changelogs, tests before releases, and no mobile rebuild for server-only changes. Android and iPhone share a mobile version. Every change to a shipped component should release, including internal maintenance changes.

## Options considered

1. One repository-wide version. Simple, but forces unrelated component versions and builds to advance together.
2. Two independent release streams with dependency-aware CI. Requires component selection and release planning, but matches the requested behavior.
3. Separate repositories. Separates release tooling but loses the requested monorepo and complicates shared API changes.

## Decision

Use `server-vX.Y.Z` and `mobile-vX.Y.Z`, starting independently at `0.1.0`. Component-relevant commits determine each bump: breaking marker or footer produces major, feat produces minor, and every other shipped change produces patch. API definitions and shared workflow/tooling changes affect both components. Documentation-only changes do not publish.

Every PR validates its title and all commit subjects. Require PRs, current passing `ci-gate`, resolved conversations, and no force pushes on main. Use squash merges so the validated PR title becomes the main commit. Do not require a human reviewer count that would block a solo maintainer from merging their own reviewed PR.

The CI workflow always starts. Change detection selects the expensive jobs, and an always-reporting gate fails if any selected job fails or is cancelled. Publication is main-only and depends on the gate. Export and publish the tested Docker image rather than rebuild it. Generate a release-specific changelog, source revision, and checksums next to the artifacts without a changelog bot bypassing main protections.

Mobile-only changes run shared analysis and tests and build both platforms with the same mobile version. This includes platform-specific changes: Android and iPhone are one release family, while the server remains independent. Android uses development signing initially. iOS is an unsigned IPA that a sideload tool can re-sign; it is not an App Store submission.

## Consequences

Required status checks cannot rely on workflow-level path exclusions. Main publication runs are serialized. A failed earlier publication is included in the next plan by comparing with the latest reachable component release tag. Server and mobile need not be released together, and each has its own changelog. End-to-end publication still requires observing a permitted main merge; PR validation cannot prove registry or store publication.
