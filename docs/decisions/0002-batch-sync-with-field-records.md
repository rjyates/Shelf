# 0002. Sync through one batch endpoint with field-level records

- **Status:** Proposed
- **Date:** 2026-10-22

## Context

product.md locks the sync rules: each field has its own `updated_at`, a highlight from one device and a position from another both stick, the device keeps an outbox and is authoritative while offline, a delete is written to the outbox before the local row is removed, and Wi-Fi comes up to drain the outbox and then goes down. Phase 7 needs position, bookmark, and highlight. Later phases and the device ideas add more synced data: notes on marks, ink strokes, shelves, looked-up words, reading sessions.

## Options considered

1. **One endpoint per resource.** `PUT /positions/…`, `DELETE /highlights/…`. Familiar. Draining an outbox takes many requests while the radio is up, a dropped radio leaves a partial drain, and every new data type needs new endpoints on both sides.
2. **One batch endpoint with typed records.** The device sends every outbox change and its cursor in one request. Shelf merges, then returns what changed since that cursor and a new cursor. New data types are new record types, not new endpoints.

## Decision

Option 2.

**Record.** Every synced item is a record:

```json
{
  "id": "0b9a6f0e-6a4e-4d0c-9a51-2f8f0b6f3c11",
  "type": "highlight",
  "book": { "library": "lib_1", "hash": "sha256:9f2c…" },
  "fields": {
    "range": { "value": "epubcfi(/6/4!/4/2,/1:0,/1:42)", "updated_at": "2026-10-22T19:04:11.120Z" },
    "style": { "value": "solid", "updated_at": "2026-10-22T19:04:11.120Z" },
    "deleted": { "value": false, "updated_at": "2026-10-22T19:04:11.120Z" }
  }
}
```

- The device creates `id` (a UUID) and it never changes.
- Phase 7 types are `position`, `bookmark`, and `highlight`. Their fields are defined in `protocol/` (milestone 7).
- A delete sets `deleted` to true. The record stays on the server as a tombstone, so an older copy on another device cannot bring it back.

**Exchange.** `POST /sync` with a device token:

```json
{ "device_time": "2026-10-22T19:05:00.000Z", "cursor": 4817, "changes": [ /* records */ ] }
```

```json
{ "applied": ["0b9a6f0e-…"], "cursor": 4822, "changes": [ /* records */ ], "more": false }
```

1. Shelf computes the device's clock offset as its own time minus `device_time`. When the offset is larger than two minutes, it shifts every `updated_at` in the batch by that offset.
2. For each incoming field, shelf keeps the newer `updated_at`. On a tie the stored value stays.
3. Each record that changes gets the next value of a server counter, `seq`.
4. The whole batch is one transaction. `applied` lists the ids that were accepted.
5. The response holds records with `seq` greater than the request's cursor, in `seq` order, up to a page limit. This includes the device's own records as merged, so the device converges on the server's result. `more` is true when another page is waiting. The device repeats the request with no changes and the new cursor until `more` is false.
6. The device clears an outbox entry only after its id appears in `applied`.

Sending the same batch twice gives the same stored state, so a retry after a dropped radio is safe.

## Consequences

- One request per Wi-Fi window in the common case, which fits the radio rule.
- The cursor is a server counter, so pulling never depends on device clocks.
- Notes, ink, shelves, words, and reading sessions can be added as record types without new endpoints.
- Tombstones grow without limit. Pruning them needs a rule about how long a device may stay offline. Not needed for phase 7.
- Clock correction assumes the device clock did not jump between the edit and the sync. A device that lost its time between the two can still order its own edits wrongly. A hybrid logical clock fixes that and can replace the offset rule later without changing the record shape.
- Last-writer-wins per field loses one of two concurrent edits to the same field, for example the same highlight's style changed on two devices. product.md accepts that by locking field-level merge.
