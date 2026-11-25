# M12 — Rich stats and external integrations (remaining work)

**Status:** active fly path; slices 5–6 on `feat/m12-koreader-kobo-sessions` (2026-11-21).  
**Capture foundation:** **shipped** on `main` via [#61](https://github.com/rjyates/Shelf/pull/61) — sessions, daily rollups, progression history, lifecycle events, prefs/goals. Inventory: [`m12-stats-inventory.md`](m12-stats-inventory.md).  
**Chart slices 1–4:** **usable** on `main` — reading insights, widgets, achievements, library health (`GET /api/v1/insights/{reading,widgets,achievements,library}` + Insights UI).  
**Injector slices 5–6:** **usable** — `POST /api/v1/books/{id}/koreader/page-stats` and `POST /api/v1/books/{id}/kobo/statistics` write unified `reading_sessions` (`source=koreader` / `kobo`).  
**Roadmap:** milestone **12** in [`docs/parity.md`](../parity.md).

**Pins (leverage, reimplement in Rust — do not vendor Nest/TS runtimes):**

| Source | Pin |
| --- | --- |
| Book Orbit | [`c77966cc`](https://github.com/bookorbit/bookorbit/commit/c77966cc529423e993fae8a298aa77048b43bba2) |
| Storyteller | [`2de166da`](https://gitlab.com/storyteller-platform/storyteller/-/commit/2de166daa9eb5b2b120fd639434f856c19cad823) |

**Storyteller rule of thumb:** ST does **not** implement heatmaps, streaks, goals, DNA, achievements, OPDS, Send-to-Kindle, or Hardcover/Readwise/StoryGraph sync. Its useful M12-adjacent work is already covered by capture (`progression` / `progression_history`). Prefer Book Orbit for everything in this todo.

Shelf local mirrors of BO public types (shapes only): `web/orbit-reader/vendor/bookorbit/types/{user-statistics,statistics,dashboard,achievement,koreader,kobo,opds,hardcover,readwise,storygraph,delivery}.ts`.

---

## Current todo (ordered slices)

Ship as incremental PRs on this branch (or stacked PRs). Each slice: **Rust API first**, then web Insights/Settings chrome; mobile where it fits e-ink.

| # | Slice | Shelf today | Book Orbit leverage (Rust rewrite) | Storyteller |
| --- | --- | --- | --- | --- |
| 1 | **Reading chart APIs + Insights UI (first graphs)** | Raw `GET /reading-daily`, `GET /reading-sessions`; Insights empty | Aggregate from `reading_sessions` / `reading_daily_stats` like BO `user-statistics`: [user-statistics.service.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/user-statistics/user-statistics.service.ts), [user-statistics.controller.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/user-statistics/user-statistics.controller.ts), aggregation job [user-statistics-aggregation.job.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/user-statistics/user-statistics-aggregation.job.ts), daily utils `server/src/common/utils/reading-daily-stats.utils.ts`. Chart ids / default order: [statistics.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/packages/types/src/statistics.ts) + vendor `user-statistics.ts`. **First UI:** heatmap, source distribution, session timeline, streak (slice 2 can share streak calc). | None — no session charts |
| 2 | **Streak / goal / challenge / DNA widgets** | `reading_goals` + timezone prefs only | [dashboard-widget.calculations.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/dashboard/dashboard-widget.calculations.ts) + [dashboard-widget.service.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/dashboard/dashboard-widget.service.ts); batch `POST /dashboard/widgets/batch`; types in vendor `dashboard.ts` (`reading-streak`, `reading-goal`, `monthly-challenge`, `reading-dna`, `reading-rhythm`, `year-projection`). Overview/Insights widgets, not Foliate chrome. | None |
| 3 | **Achievements** | No unlock tables | [achievement.service.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/achievement/achievement.service.ts), [achievement-seed.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/achievement/seed/achievement-seed.ts), `evaluators/*` (reading / library / exploration / dedication / devices), hints/celebrations. Port seed + evaluators against Shelf sessions/events/catalogue; keep e-ink UI minimal. | None |
| 4 | **Library-health charts** | Catalogue + M11 metadata | [statistics.service.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/statistics/statistics.service.ts) / [statistics.controller.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/statistics/statistics.controller.ts); library chart order in `DEFAULT_LIBRARY_CHART_ORDER` (format/language/storage/completeness/freshness/…). Query Shelf `book_metadata` / entities / `epub_files`. | None |
| 5 | **KOReader session (+ progress) injectors** | **Usable** — `POST …/koreader/page-stats` → `reading_sessions` (`source=koreader`); broader catalog/annotation sync later | [koreader-stats.service.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/koreader/koreader-stats.service.ts) + [koreader-stats.util.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/koreader/koreader-stats.util.ts) → same `reading_sessions` path. Broader sync/catalog later from `koreader-catalog.*`, annotation/bookmark exchange — start with **page-stat → session** so charts light up. Types: vendor `koreader.ts`. | None |
| 6 | **Kobo session injectors (+ optional sync)** | **Usable** — `POST …/kobo/statistics` → `reading_sessions` (`source=kobo`); full store sync later | [kobo-statistics-session.util.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/kobo/kobo-statistics-session.util.ts); device/sync around [kobo-sync.controller.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/kobo/kobo-sync.controller.ts). Same rule: sessions first, full store sync later. Types: vendor `kobo.ts`. | None |
| 7 | **OPDS catalog** | **Usable** — Basic-auth OPDS users (`GET/POST/DELETE /opds-users`); Atom feeds at `/opds`, `/opds/catalog`, `/opds/recent`, authors/series/collections; download `/opds/{id}/file`; Settings UI; session Bearer does not unlock feeds | [opds.controller.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/opds/opds.controller.ts), [opds.service.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/opds/opds.service.ts), [opds-book.service.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/opds/opds-book.service.ts), XML helpers; OPDS users [opds-user.*](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/opds/opds-user.controller.ts). Types: vendor `opds.ts`. Emit Atom/OPDS from Shelf catalogue in Rust (`axum` + XML). | None |
| 8 | **Send-to-Kindle (email delivery)** | **Usable** — `GET/PUT /kindle-delivery` + `POST …/send-to-kindle` (EPUB/PDF, SMTP, stub via `SHELF_KINDLE_SMTP_STUB`); no multi-provider/templates | BO `email` module: [email-send.controller.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/email/email-send.controller.ts), [email-send-orchestrator.service.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/email/email-send-orchestrator.service.ts), preferences/providers/file selector. Kindle = email attachment to user `@kindle.com` (and SMTP provider config). Library flag shapes: vendor `library.ts` (`fileWriteKindle*`). | None |
| 9 | **Hardcover sync** | Metadata **provider** only (M11); no account sync | [hardcover-sync.service.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/hardcover/hardcover-sync.service.ts), client/settings/queue/event-listener/import; GraphQL outbound. Types: vendor `hardcover.ts`. | None |
| 10 | **Readwise sync** | Annotations on Shelf; no push | [readwise-sync.service.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/readwise/readwise-sync.service.ts) + event listener/scheduler (create-only highlight push pattern from BO). Types: vendor `readwise.ts`. | None |
| 11 | **StoryGraph sync** | None | [storygraph-sync.service.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/storygraph/storygraph-sync.service.ts) + cookie auth/settings/match (session cookie + remember token). Types: vendor `storygraph.ts`. | None |

---

## Implementation notes (all slices)

1. **Leverage = algorithms + API contracts + tests ideas**, not a Nest port. Prefer SQLite SQL aggregations in `server/src/` (new modules e.g. `user_stats`, `dashboard`, `opds`, …) and OpenAPI updates.
2. **Reuse capture tables** — do not invent a second session model; KOReader/Kobo must write `reading_sessions` with `source` set so charts stay unified (BO’s design).
3. **Timezone** — streaks/heatmap days use reader timezone / `timezone_offset_minutes` already on sessions/daily rows; BO backfill job is a reference if we ever rebuild days: `user-reading-stats-timezone-backfill.service.ts`.
4. **UI** — journal Insights/Overview per `docs/design/v3.md` / `.picasso.md`; keep Foliate chrome alone (decision 0004). Mobile: e-ink-safe summaries, not dense BO dashboards.
5. **Secrets** — store API tokens/cookies encrypted at rest (follow BO email-encryption / settings patterns; do not commit tokens).

---

## Explicit non-goals (this milestone)

- Multi-user / OIDC → **M16**
- *arr / Seerr request UX → **M15**
- Kindle-shaped **reader chrome** wishlist → [`mobile-reader-future.md`](mobile-reader-future.md) (not M12 stats)
- Pin-for-pin every BO chart before the first heatmap/streak ships — slice **1** may ship a subset of `DEFAULT_USER_CHART_ORDER`

---

## Acceptance (milestone-level)

- [x] Insights shows session-derived heatmap (or daily series) + at least one of streak / source distribution / session timeline from live Shelf data  
- [x] Annual goal surfaces against finishes (widget or Insights)  
- [x] At least one external injector **or** OPDS **or** outbound sync lands end-to-end (slices 5–6 KOReader/Kobo; slice 7 OPDS; slice 8 Send-to-Kindle; Hardcover/Readwise/StoryGraph still open)  
- [x] OpenAPI + API tests for new aggregate/sync routes  
- [x] Docs: this file + parity rows updated as slices become usable  

---

## Sources

1. [`m12-stats-inventory.md`](m12-stats-inventory.md) (capture map)  
2. Book Orbit README / modules @ `c77966cc`  
3. Shelf vendor types under `web/orbit-reader/vendor/bookorbit/types/`  
4. Storyteller @ `2de166da` — progression only (see inventory §2)  
5. Platform badge ideas (GR / Letterboxd / StoryGraph / BO → Shelf): [`achievements-inspiration.md`](achievements-inspiration.md)  
