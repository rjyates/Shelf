# Mobile reader — possible future enrichment (Kindle-shaped)

**Status:** ideas only — not scheduled. Capture wishlist for Shelf’s Flutter/Readium e-ink reader so agents do not re-derive Kindle chrome from scratch.
**Today:** Readium EPUB/PDF/CBZ; bookmarks/highlights; listen (TTS/MO); chapter title + % in chrome footer when controls are up; session metrics capture.
**Related:** [`docs/parity.md`](../parity.md) (native mobile EPUB / annotations rows), `.cursor/skills/shelf-eink-android/SKILL.md` (Nomad QA).

When picking work, prefer e-ink-first (light paper / black ink, no motion chrome) and keep Foliate web reader separate unless a feature is intentionally shared.

---

## Footer / progress

Kindle: cycle via footer tap or **Aa → More → Reading Progress**.

| Idea | Notes |
| --- | --- |
| Time left in chapter | Estimate from recent pace (needs session/page timing) |
| Time left in book | Same, whole-title estimate |
| Page in book | When a page-list / fixed pages exist (PDF/CBZ already closer) |
| Location in book | Stable digital location # (Kindle-style; not CFI verbatim) |
| None | Hide the cycling progress metric |
| Percentage read | Keep lower-right (or equivalent); separate from the cycle |
| Tap footer to cycle modes | Kindle’s primary UX for switching the above |

Shelf already shows chapter (when known) + % when chrome is up.

---

## Chrome when you tap

| Idea | Shelf today (rough) |
| --- | --- |
| Book title (top) | Yes |
| Clock while reading (optional) | No |
| Font / layout (Aa): size, font, margins, spacing, orientation, theme | Partial / missing as a unified Aa sheet |
| Go to: TOC, cover, beginning, page/location | Incomplete vs Kindle |
| Bookmarks / notebook (notes + highlights list) | Bookmarks + highlights exist; notebook polish TBD |
| Search in book | No |
| Share / Goodreads | No (store-oriented; low priority for self-hosted) |

---

## In-book tools

| Idea | Notes |
| --- | --- |
| Dictionary on long-press | Selection exists for highlight/bookmark; no dictionary |
| Wikipedia / translation / web lookup | No |
| X-Ray (people, terms, images) | No — heavy content enrichment |
| Page Flip (skim without losing place) | No |
| Continuous scroll vs paginated | Readium defaults; user toggle TBD |
| WhisperSync-style read ↔ audiobook | Partial via companions / listen; not Kindle WhisperSync |

---

## Library (not in-reader)

| Idea | Shelf today |
| --- | --- |
| % on cover / list | Yes (progress on library cards) |

---

## Explicit non-goals for this note

- No commitment to pin-for-pin Kindle parity.
- No Goodreads/Amazon account coupling required for core reading.
- Web Foliate chrome stays under Book Orbit decision 0004 unless separately scoped.
