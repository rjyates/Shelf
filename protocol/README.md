# protocol

The messages the device and shelf exchange: device pairing and sync. Book catalog and downloads use OPDS, which is a published standard and is not defined here.

Status: no schemas yet. The first ones arrive with phase 7 (Progress sync). Design: [docs/shelf/architecture.md](../docs/shelf/architecture.md), [0001](../docs/decisions/0001-protocol-json-schema-and-fixtures.md), [0002](../docs/decisions/0002-batch-sync-with-field-records.md), [0003](../docs/decisions/0003-device-pairing-codes.md).

## Layout

```text
protocol/
  schemas/      one JSON Schema (draft 2020-12) per message and per record type
  fixtures/     example messages: valid/ must pass, invalid/ must fail
```

A change to a message changes its schema and its fixtures in the same pull request. Shelf's tests and the device's tests both run against the fixtures.

## Rules

- JSON over HTTP. Every request carries `Oseink-Protocol: 1`.
- A receiver ignores fields and record types it does not know.
- Adding an optional field or a new record type keeps the version.
- Removing a field, changing a field's meaning, or adding a required field raises the version.
- Times are UTC, RFC 3339, with milliseconds.
- A book is `{ "library": "<library id>", "hash": "sha256:<hex>" }`.

## Messages

| Message | Endpoint | Phase |
| --- | --- | --- |
| Pairing start and token | `POST /pair/start`, `POST /pair/token` | 7 |
| Sync request and response | `POST /sync` | 7 |
| Record: `position`, `bookmark`, `highlight` | inside sync | 7 |

Later record types (notes, ink, shelves, reading sessions) are added here as they are needed.
