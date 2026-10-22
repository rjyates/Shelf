# 0001. Define the protocol with JSON Schema and fixtures

- **Status:** Proposed
- **Date:** 2026-10-22

## Context

`protocol/` holds the request and response types shared by the device and shelf (product.md, Two products). The device shell is C, built static with the Buildroot toolchain. Shelf is TypeScript on Node 22. Both sides must agree on every message, and a change on one side must not break the other without a test noticing. OPDS is a published standard and stays outside `protocol/`.

## Options considered

1. **A written spec only.** A Markdown file describes the JSON. Simplest. Nothing checks either side against it, so drift shows up as bugs on the other side.
2. **JSON Schema plus fixtures.** Each message has a JSON Schema in `protocol/schemas/`. Real example messages live in `protocol/fixtures/`, valid and invalid. Shelf validates requests with the same schemas and tests against the fixtures. The device tests parse the same fixtures. Plain HTTP and JSON are easy to produce and read in C.
3. **Protobuf or gRPC.** Generated, strongly typed code. Heavy for a static C binary on BusyBox, hard to read when debugging on a panel, and a second transport beside OPDS over HTTP.

## Decision

Option 2. The transport is JSON over HTTP. Schemas use JSON Schema draft 2020-12. Fixtures are the contract between the two sides: a change to a message changes its schema and its fixtures in the same pull request, and both sides' tests run against them.

Compatibility rules, recorded in [protocol/README.md](../../protocol/README.md):

- Every request carries `Oseink-Protocol: 1`.
- A receiver ignores fields and record types it does not know.
- Adding an optional field or a new record type does not change the version.
- Removing a field, changing a field's meaning, or adding a required field raises the version. Shelf keeps serving the previous version until devices have moved.

## Consequences

- Both sides catch a broken message in their own tests before the other side sees it.
- The schemas are written once and serve as documentation, request validation in Fastify (0004), and test input.
- The device side needs a JSON reader in C and a test that walks the fixtures. That is device work.
- JSON is larger than a binary format. If phase 12 ink strokes make sync payloads large, revisit the encoding for that record type only.
