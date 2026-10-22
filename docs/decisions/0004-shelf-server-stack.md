# 0004. Build shelf on Fastify, better-sqlite3, and server-rendered HTML

- **Status:** Proposed
- **Date:** 2026-10-22

## Context

product.md locks shelf to TypeScript on Node 22, one container, SQLite, and an admin UI served by the same process. Inside those limits we still choose a web framework, a SQLite driver, a migration approach, a test runner, and how the admin UI is built. Shelf is maintained by a small team and should stay small.

## Options considered

1. **Express, an ORM, and a single-page admin app.** Familiar. An ORM hides the SQL that sync correctness depends on, and a single-page app adds a front-end build and a second set of dependencies.
2. **Fastify, better-sqlite3 with plain SQL, `node:test`, server-rendered HTML.** Fastify validates requests with JSON Schema, which reuses the `protocol/` schemas from 0001. better-sqlite3 is synchronous, fast, and widely used. Plain SQL files keep the merge queries readable. `node:test` is built in. Server-rendered pages need no front-end build.
3. **Node's built-in `node:sqlite`.** No native module. Still marked experimental on Node 22.

## Decision

Option 2.

| Piece | Choice |
| --- | --- |
| HTTP | Fastify |
| Request validation | JSON Schema from `protocol/schemas/` |
| SQLite | better-sqlite3, WAL mode |
| Migrations | Numbered `.sql` files in `shelf/migrations/`, applied in order at start, tracked in `meta` |
| Passwords | Node `crypto.scrypt` |
| Tests | `node:test` |
| Admin UI | HTML rendered by the server, a small stylesheet, no front-end framework |
| Build | `tsc` to `dist/`, run with `node` |

Libraries for reading EPUB and PDF metadata, OPDS XML, and the Open Library client are picked in the milestone that needs them, and each one is noted in its pull request.

## Consequences

- The schemas in `protocol/` protect the HTTP surface directly.
- The SQL behind merge and cursor paging is visible in review.
- better-sqlite3 is a native module. The Docker image builds it for Linux. On Windows and macOS it installs from prebuilt binaries.
- Server-rendered pages limit how interactive the admin UI can be. If it later needs live updates, add small scripts to those pages before considering a front-end framework.
- Revisit `node:sqlite` when shelf moves to a Node release where it is stable.
