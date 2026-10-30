# 0001. Rust server and shared Flutter mobile app

- Status: Accepted
- Date: 2026-10-30

## Context

The user requested Book Orbit's functionality with Shelf's approved v3 design, a Rust implementation, one self-hosted container, and one mobile codebase for Android and iPhone. The repository was reset for this work. Historical Node/server-rendered and Vue/NestJS fork proposals are superseded.

## Options considered

1. Adapt Book Orbit's TypeScript server and Vue client. Reuses implemented behavior but does not meet the Rust requirement.
2. Build the server in Rust and mobile in Flutter/Dart. Meets the requested server language and shares mobile UI and behavior. Full Book Orbit parity must be implemented and verified incrementally.
3. Use Rust and Tauri for mobile. Could share browser UI, but background audio, readers, and offline integration would need platform validation.

## Decision

Use Rust with Axum and SQLite for the server, serving locally bundled browser assets from one process. SQLite avoids a separate database service in the copy-and-paste Compose setup. Use Flutter/Dart for Android and iPhone, with the same versioned HTTP API used by the browser. Keep sessions in mobile memory until secure persisted sessions are implemented.

The initial vertical slice provides first-account setup, login, a persistent metadata catalogue, manually recorded page progress, and real catalogue totals. File ingestion, readers, playback, and external integrations remain explicit roadmap items.

## Consequences

The server remains a single container with persistent data mounted separately. Browser and mobile designs share tokens but not widget code. API changes require compatibility tests for both. iOS builds run on GitHub-hosted macOS; no local Mac is required. Store signing and publishing require later credentials and authorization.

Book Orbit is a functional reference. No upstream implementation code is imported in this foundation. If upstream material is reused later, review and retain its applicable AGPL and additional notices.
