# M12 research: reading stats inventory (Book Orbit + Storyteller)

**Status:** capture **shipped** (sessions, daily rollups, progression history, lifecycle events, prefs/goals); charts / heatmap / streak UI still Planned.
**Roadmap home:** Implementation milestone **12** (Rich stats and external integrations) in [`docs/parity.md`](../parity.md). Milestone **11** remains library scale/organization.
**Pins:**

| Source | Pin |
| --- | --- |
| Book Orbit | [`c77966cc`](https://github.com/bookorbit/bookorbit/commit/c77966cc529423e993fae8a298aa77048b43bba2) (`main` as of 2026-11-17) |
| Storyteller | [`2de166da`](https://gitlab.com/storyteller-platform/storyteller/-/commit/2de166daa9eb5b2b120fd639434f856c19cad823) (same pin as align parity) |

Shelf stores Book Orbit–shaped reading sessions (web Foliate + Flutter) plus daily rollups and lifecycle events. Catalogue totals (`GET /api/v1/stats`) remain; heatmaps/streaks/charts are still Planned. See [Shelf baseline](#shelf-baseline-today).

---

## 1. Book Orbit — what feeds every graph

Almost all user reading charts are derived from **reading sessions** (plus completion / attempt metadata). Capture is the foundation; charts are aggregations.

### 1.1 Capture: reading sessions

| Field / concept | Meaning | Source |
| --- | --- | --- |
| `sessionId` | Client UUID; idempotent save | [save-reading-session.dto.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/reading-session/dto/save-reading-session.dto.ts) |
| `startedAt` / `endedAt` | Wall-clock bounds | same |
| `durationSeconds` | **Active** reading time (idle/hidden excluded); must not exceed wall span | same + [useReadingSession.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/client/src/features/reader/shared/composables/useReadingSession.ts) |
| `progressDelta` | Progress change (may be negative on backtrack) | DTO |
| `endProgress` | 0–100 end position | DTO |
| `sessionType` | `read` \| `tts` \| `listen` | DTO + [reading-session.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/packages/types/src/reading-session.ts) |
| `source` | `web`, `ios`, `watchos`, `android`, `koreader`, `manual`, `kobo` | types + server defaults |

**Client timer rules** ([useReadingSession.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/client/src/features/reader/shared/composables/useReadingSession.ts)):

- Idle timeout **5 minutes** → end session
- Minimum session **10 seconds** before POST
- Elapsed UI refresh every **30 seconds**
- Activity pauses when tab hidden; resumes on activity

**Write APIs:**

| Route | Role | Source |
| --- | --- | --- |
| `POST /books/files/:fileId/sessions` | Auto session from reader (204) | [reading-session.controller.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/reading-session/reading-session.controller.ts) |
| `GET/POST/DELETE /books/:bookId/sessions` | Per-book log + manual sessions | [book-reading-session.controller.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/reading-session/book-reading-session.controller.ts) |

**Per-book session aggregates** (`BookReadingSessionStats` in [reading-session.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/packages/types/src/reading-session.ts)): total/avg duration, first/last session, daily minutes, pace, progress summary, `bySource`, longest session, `backtrackCount`, `latestEndProgress`.

**External session injectors** (same session table → same charts):

| Source | Module |
| --- | --- |
| KOReader | [koreader-stats.service.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/koreader/koreader-stats.service.ts) |
| Kobo | [kobo-statistics-session.util.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/kobo/kobo-statistics-session.util.ts) |

Daily rollups / timezone rebuild helpers: `server/src/common/utils/reading-daily-stats.utils.ts`, aggregation job under `server/src/modules/user-statistics/`.

---

### 1.2 User statistics charts (session-derived)

Controller: [user-statistics.controller.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/user-statistics/user-statistics.controller.ts).
Types: [user-statistics.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/packages/types/src/user-statistics.ts), chart ids in [statistics.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/packages/types/src/statistics.ts).

| Chart id / endpoint | Captured / computed metric |
| --- | --- |
| `GET …/summary` | tracked / started / in-progress / completed book counts; mean progress % |
| `daily-reading` | per-day `readingSeconds`, `progressDelta`, `eventsCount` |
| `reading-heatmap` | same daily series + optional `bySource` tooltip |
| `reading-source-distribution` | seconds by source bucket |
| `peak-hours` | hour-of-day seconds/events (+ format/source splits) |
| `favorite-days` | day-of-week seconds/events |
| `session-timeline` (+ PATCH) | weekly list of sessions (id, book, duration, times) |
| `completion-timeline` | books finished by year/month |
| `goal-trajectory` | cumulative finishes vs annual goal |
| `progress-funnel` | started → 25/50/75% → completed |
| `completion-latency` | days start→finish distribution |
| `genre-reading-time` | seconds by genre (+ bySource) |
| `reading-pace` | duration vs progressDelta scatter |
| `reading-survival` | % of starts surviving progress thresholds |
| `completion-race` | per-book progress vs days-since-start |
| `session-archetypes` | hour × duration × weekday points |
| `author-genre-chord` | author↔genre chord |
| iOS activity APIs (`activity-overview`, `activity-calendar/:year`, `activity-days/:day`, `activity-details/*`) | App Attest–gated activity calendar / detail |

Default user chart order (UI): heatmap → peak hours → favorite days → completion timeline → goal trajectory → progress funnel → completion latency → genre time → pace → books completed → reading clock → session timeline → session archetypes → source distribution ([statistics.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/packages/types/src/statistics.ts)).

---

### 1.3 Library statistics charts (catalogue metadata — not session time)

Controller: [statistics.controller.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/statistics/statistics.controller.ts).

| Endpoint / chart id | Metric |
| --- | --- |
| `summary` | total books/authors/series/publishers/genres/languages/storage; pub year range; books added this year |
| `format-distribution` | counts by format |
| `language-distribution` | counts by language |
| `books-added-over-time` | adds by year/month |
| `storage-by-format` | bytes by format |
| `publication-decade` / `publication-year-timeline` | pub-date histograms |
| `top-authors` / `top-series` | ownership counts |
| `metadata-completeness` / `library-metadata-completeness` | field presence |
| `metadata-score-distribution` | score bins + percentiles |
| `genre-distribution` / `genre-cooccurrence` | genre counts / chord |
| `format-share-over-time` | format mix over time |
| `page-count-distribution` | box stats per format |
| `metadata-freshness-gauge` | never/fresh/stale buckets + score |
| `library-integrity-gauge` | file/metadata presence score |
| `acquisition-lag-scatter` | years between pub and add |
| `largest-books` | top size rows |

These need rich metadata (authors, series, genres, languages, scores) — closer to Shelf **M11/M17** data than to session capture.

---

### 1.4 Dashboard widgets (goals, streaks, DNA, challenges)

Controller: [dashboard.controller.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/dashboard/dashboard.controller.ts).
Types: [dashboard.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/packages/types/src/dashboard.ts).
Calculations (streaks, DNA, rhythm, challenges, projection): [dashboard-widget.calculations.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/dashboard/dashboard-widget.calculations.ts).

| Widget | Metrics |
| --- | --- |
| `reading-streak` | `currentStreak`, `longestStreak`, `lastSevenDays[7]` (reader timezone) |
| `reading-goal` | `goalBooks`, `completedBooks`, `year` |
| `reading-dna` | archetype + length/variety/rhythm/time/speed scores + labels + `booksAnalyzed` |
| `monthly-challenge` | type (`short-read`, `genre-explorer`, `finish-oldest`, `streak-builder`, `new-author`, `page-milestone`), progress/target |
| `reading-rhythm` | last-N day seconds, `consistencyPercent`, `avgSecondsPerDay`, active/total days |
| `year-projection` | projected books/pages/hours, YTD completes, trend |
| `diversity-score` | overall + genre/author/era/language scores |
| `currently-reading` | in-progress books + resume file ids |
| `library-overview` | catalogue totals (subset of library summary) |
| `highlight-of-the-day` / `highlights` | annotation showcase |
| `neglected-gems` | high-rated long-waiting unread |
| `long-wait` | oldest unread wait |

Batch: `POST /dashboard/widgets/batch`.

---

### 1.5 Achievements (86 seeded)

API: [achievement.controller.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/achievement/achievement.controller.ts).
Seed catalogue: [achievement-seed.ts](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/achievement/seed/achievement-seed.ts).

| Category | Count | Evaluators dir |
| --- | --- | --- |
| `reading` | 28 | `evaluators/reading.evaluator.ts` |
| `library` | 22 | `library.evaluator.ts` |
| `exploration` | 18 | `exploration.evaluator.ts` |
| `dedication` | 15 | `dedication.evaluator.ts` (streaks, seasons, weekends) |
| `devices` | 3 | `devices.evaluator.ts` |
| **Total** | **86** | registry: `evaluator-registry.ts` |

Also: rating evaluator, milestones, hints (`GET /achievements/hints`), celebration claim/ack, admin backfill.

Product blurb (“daily time, heatmaps, streaks, Reading DNA, 50+ achievements”): [README.md](https://github.com/bookorbit/bookorbit/blob/main/README.md).

---

### 1.6 Shared / admin reading insights

Module: `server/src/modules/shared-reading-insights/` (opt-in household/admin aggregates). Types: Shelf vendor copy `web/orbit-reader/vendor/bookorbit/types/shared-reading-insights.ts` mirrors upstream `packages/types/src/shared-reading-insights.ts`.

---

## 2. Storyteller — progress telemetry (not a stats product)

Storyteller ([gitlab.com/storyteller-platform/storyteller](https://gitlab.com/storyteller-platform/storyteller)) does **not** ship heatmaps, streaks, goals, Reading DNA, or session-duration analytics. What it captures is **position / progression sync** plus a thin history table.

### 2.1 In-memory “reading session” (UI only)

[readingSessionSlice.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/2de166daa9eb5b2b120fd639434f856c19cad823/applications/web/src/store/slices/readingSessionSlice.ts) holds live reader state: current book, mode (`epub` / `audiobook` / `readaloud`), locator, syncing flag, sleep timer, highlight fragment, errors. **Not persisted as timed sessions.**

### 2.2 Persisted progression

| Store | Fields | Source |
| --- | --- | --- |
| `progression` | per user+book+format: `progression` (0–1), device id/name, `modified`, title | [124_v3_progression.sql](https://gitlab.com/storyteller-platform/storyteller/-/blob/2de166daa9eb5b2b120fd639434f856c19cad823/applications/web/migrations/124_v3_progression.sql) |
| `progression_reference` | locator refs (`kind` + `value`) | same |
| `progression_history` | append-only-ish history: progression, device, `source_format`, `char_ref`, `media_ref`, `modified` | [126_progress_history.sql](https://gitlab.com/storyteller-platform/storyteller/-/blob/2de166daa9eb5b2b120fd639434f856c19cad823/applications/web/migrations/126_progress_history.sql) |

Write path: `POST /api/v3/books/[bookId]/progress?format=` → [progress/route.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/2de166daa9eb5b2b120fd639434f856c19cad823/applications/web/src/app/api/v3/books/[bookId]/progress/route.ts) → [upsertProgression](https://gitlab.com/storyteller-platform/storyteller/-/blob/2de166daa9eb5b2b120fd639434f856c19cad823/applications/web/src/database/progressions.ts).

History throttle: insert into `progressionHistory` **at most once per 5 minutes** per user+book (same file).

Legacy `position` table + `/api/v2/books/…/positions` still exist for older clients.

**Implication for Shelf graphs:** Storyteller is a reference for **multi-format position + sparse history**, not for reading-time analytics. Session-time graphs should follow Book Orbit’s session model.

---

## 3. Shelf baseline (today)

| Surface | What it stores / shows | Status |
| --- | --- | --- |
| `GET /api/v1/stats` | `books`, `pages_read`, `finished`, `reading`, `average_progress` (hides work audio/readaloud) | Shipped |
| Web `libraryStats` | folded catalogue counts | Shipped |
| `books.reading_status` | `open` / `finished` / `unfinished` / `rereading` | Shipped |
| EPUB / audio location | CFI / % / playback position | Shipped |
| `reading_sessions` + POST/GET session APIs | BO fields; Foliate unstubbed; Flutter tracker (read/tts/listen) | **Capture shipped** |
| `reading_daily_stats` | timezone-offset day buckets on session write | **Capture shipped** |
| `progression_history` | throttled ≤1 / 5 min on location/playback save | **Capture shipped** |
| `reading_events` | opened / finished / unfinished / rereading / progress milestones | **Capture shipped** |
| `user_reading_defaults` + `reading_goals` | timezone + annual goal books | **Capture shipped** |
| Heatmap / streak / DNA / chart UI | Insights empty until aggregation slice | Planned |

Parity: session capture usable; graphs Planned ([parity.md](../parity.md)).

---

## 4. Capture stack vs remaining M12 work

**Shipped (this foundation):**

1. **Reading sessions** — schema + `POST /books/files/{id}/sessions` (Foliate) + `POST/GET/DELETE /books/{id}/sessions`; Foliate `useReadingSession`; Flutter `ReadingSessionTracker` (5 min idle, 10s min, background pause).
2. **Daily rollup** — `reading_daily_stats` upserted on session insert; `GET /reading-daily`.
3. **Progression history + lifecycle events** — location/playback throttle; mark-finished/unfinished/rereading + milestones.
4. **Prefs/goals** — `GET/PUT /reading-prefs` (timezone + annual goal).

**Still Planned (charts / presentation):**

1. Heatmap, streak widget, source distribution, session timeline UI
2. Goals / challenges / DNA / achievements evaluators + chrome
3. Library-health charts (mostly M11/M17 metadata)
4. KOReader / Kobo session injectors (`source` values reserved)

---

## 5. Explicit non-goals of this note

- No heatmap / streak / DNA chart UI in the capture foundation (charts stay later M12).
- No milestone reorder (M11 stays library scale).
- No claim of pin-for-pin BO chart parity until aggregation + presentation land on this data.

---

## Sources

1. [Book Orbit repo](https://github.com/bookorbit/bookorbit)
2. [Book Orbit README](https://github.com/bookorbit/bookorbit/blob/main/README.md)
3. [statistics.controller.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/statistics/statistics.controller.ts)
4. [user-statistics.controller.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/user-statistics/user-statistics.controller.ts)
5. [reading-session.controller.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/reading-session/reading-session.controller.ts)
6. [book-reading-session.controller.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/reading-session/book-reading-session.controller.ts)
7. [dashboard.controller.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/dashboard/dashboard.controller.ts)
8. [achievement.controller.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/achievement/achievement.controller.ts)
9. [useReadingSession.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/client/src/features/reader/shared/composables/useReadingSession.ts)
10. [save-reading-session.dto.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/reading-session/dto/save-reading-session.dto.ts)
11. [packages/types statistics.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/packages/types/src/statistics.ts)
12. [packages/types user-statistics.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/packages/types/src/user-statistics.ts)
13. [packages/types dashboard.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/packages/types/src/dashboard.ts)
14. [packages/types reading-session.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/packages/types/src/reading-session.ts)
15. [achievement-seed.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/achievement/seed/achievement-seed.ts)
16. [dashboard-widget.calculations.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/dashboard/dashboard-widget.calculations.ts)
17. [koreader-stats.service.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/koreader/koreader-stats.service.ts)
18. [kobo-statistics-session.util.ts @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/kobo/kobo-statistics-session.util.ts)
19. [Storyteller readingSessionSlice.ts @ 2de166da](https://gitlab.com/storyteller-platform/storyteller/-/blob/2de166daa9eb5b2b120fd639434f856c19cad823/applications/web/src/store/slices/readingSessionSlice.ts)
20. [Storyteller 126_progress_history.sql @ 2de166da](https://gitlab.com/storyteller-platform/storyteller/-/blob/2de166daa9eb5b2b120fd639434f856c19cad823/applications/web/migrations/126_progress_history.sql)
21. [Storyteller progressions.ts @ 2de166da](https://gitlab.com/storyteller-platform/storyteller/-/blob/2de166daa9eb5b2b120fd639434f856c19cad823/applications/web/src/database/progressions.ts)
22. [Storyteller progress route @ 2de166da](https://gitlab.com/storyteller-platform/storyteller/-/blob/2de166daa9eb5b2b120fd639434f856c19cad823/applications/web/src/app/api/v3/books/[bookId]/progress/route.ts)
23. [Storyteller 124_v3_progression.sql @ 2de166da](https://gitlab.com/storyteller-platform/storyteller/-/blob/2de166daa9eb5b2b120fd639434f856c19cad823/applications/web/migrations/124_v3_progression.sql)
24. [Shelf parity.md](../parity.md)
