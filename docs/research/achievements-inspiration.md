# Achievements inspiration (platforms → Shelf)

**Date:** 2026-11-19  
**Shelf today:** 10 badges in `server/src/achievements.rs` `SEED` (sessions, streak, finishes, library size, multi-source, hours, formats).  
**Book Orbit pin:** [`achievement-seed.ts` @ c77966cc](https://github.com/bookorbit/bookorbit/blob/c77966cc529423e993fae8a298aa77048b43bba2/server/src/modules/achievement/seed/achievement-seed.ts) — **87** badges across reading / library / exploration / dedication / devices.

This note is idea inventory only. Do not copy Goodreads seasonal list titles or curated community lists verbatim; adapt mechanics that fit private self-hosted data.

---

## What each platform actually does

| Platform | Achievement / habit model | Useful for Shelf? |
| --- | --- | --- |
| **Goodreads** | Time-boxed **seasonal challenges** + **monthly finish** badges + year-end goal badges (`Nailed It`, `Stepped Up`, `Grand Slam`). Theme badges need curated title lists (Choice Awards, “Fall Vibes”, heritage/genre lists). Annual reading challenge is a **count goal**, not a badge tree. Fan catalogue: [BenRogersWPG/Goodreads-Achievements](https://github.com/BenRogersWPG/Goodreads-Achievements). | **Yes** for calendar/finish ladders and YoY beat-last-year. **No** for Amazon curated lists (needs external editorial). |
| **Letterboxd** | Almost **no unlock badges**. Pro **stats**: diary density, hours watched, genres/countries/languages, decade/director breakdowns, **milestone lists**, Year in Review (≥10 logs). Habit signal is **logging with dates**, not trophies. | **Yes** as UX pattern: milestones as progress toward thresholds + year-in-review cards. Prefer stats over gacha badges. |
| **StoryGraph** | Mood/pace tags, deep charts, **streaks**, annual books/pages/**listening** goals, custom challenges (A–Z, country, genre-per-week), buddy reads. Stats > badges. | **Yes** for listening goals, A–Z / language / country challenges once metadata is rich. |
| **Hardcover** | Track/want/DNF, yearly goals, Year in Review; **no daily streak** in current comparisons. Discovery-forward. | Mild — goal + YIR; skip social discovery trophies. |
| **Fable / Bookly / Bookmory** | Club social + **daily streaks**; Bookly leans timers/session infographics and badge ladders. | **Yes** for session-length / power-hour / marathon (Shelf already has sessions). |
| **Book Orbit** | Full permanent catalogue with tiers + hidden one-offs + hints. Best functional reference for Shelf (already used for slice 3). | **Primary expansion source** — port selectively, keep journal tone. |

---

## Patterns worth stealing (mapped to Shelf data)

### A. Already covered (or close) by current `SEED`

| Idea | Source | Shelf status |
| --- | --- | --- |
| First session / N sessions | BO / habit apps | `first_session`, `sessions_10` |
| Finish ladder | GR Page-Turner… / BO `books_finished_*` | `finish_1`, `finish_5` (thin) |
| Streak | StoryGraph / BO `streak_*` | `streak_7` only |
| Library size | BO `library_builder_*` | `library_25`, `library_100` |
| Multi-device | BO `full_orbit` / Letterboxd “watched on” | `multi_source` |
| Hours | BO `hours_read_*` | `hours_10` |
| Format variety | BO `multi_format` | `formats_3` |

### B. High-fit next badges (local data only — recommend)

Ship these before theme/list badges. All evaluable from sessions, daily stats, books, metadata, annotations we already (or nearly) have.

| Proposed key | Mechanic | Inspiration |
| --- | --- | --- |
| `streak_30` / `streak_100` | Longer dedication tiers | BO `streak_*`, StoryGraph streaks |
| `finish_10` / `finish_25` / `finish_50` | Extend finish ladder | GR challenge ladders, BO |
| `hours_100` | Next hours tier | BO |
| `marathoner` | Single session ≥ 90 minutes | BO, Bookly |
| `power_hour` | Session ≥ 60 minutes | BO |
| `sprint` | Finish a book in one calendar day | BO |
| `binge_reader` | ≥3 finishes in one week | BO / GR Speed Reader window |
| `monthly_reader` | Finish ≥1 book in a given month | GR January…December Reader |
| `year_round` / `grand_slam` | Finish ≥1 book in every month of a year | GR Grand Slam, BO `year_round_reader` |
| `goal_met` | Hit annual `reading_goals` books target | GR Nailed It |
| `beat_last_year` | Finishes this year > last year | GR Stepped Up |
| `long_book_300` / `tome_500` | Finish book ≥ N pages (metadata) | BO long_book / GR Lightning Round length |
| `genre_5` | Finish books in N distinct genres | BO genre_explorer, StoryGraph challenges |
| `polyglot_2` | Finish books in ≥2 languages | BO polyglot, Letterboxd language stats |
| `listen_hours_10` | Audiobook / listen seconds (exclude from work-audio double-count per Shelf fold rule) | StoryGraph listening goals |
| `combine_first` | First successful readalong Combine | Shelf-native (no GR analogue) |
| `annotator_10` | N highlights/notes | BO annotator |
| `rating_spread` | Use full star range at least once | BO Across the Board / Letterboxd ratings |
| `weekend_warrior` | Sessions on Sat+Sun same week | BO weekend_* |
| `comeback` | Session after ≥14 days idle | BO `comeback_kid` |
| `full_orbit_3` | Three distinct sources | BO Full Orbit (upgrade of `multi_source`) |

### C. Letterboxd-shaped (stats first, badge optional)

These are often better as Insights widgets / Year-in-Review than permanent badges:

- Hours read this year, avg books/month, busiest week  
- Genre / language / decade breakdowns (already adjacent to library-health)  
- “Milestone” progress bars (10 / 50 / 100 finishes) instead of separate rarity spam  
- Diary density heatmap (we have reading heatmap)

### D. Low-fit / defer

| Pattern | Why defer |
| --- | --- |
| GR curated list badges (Choice Awards, Poolside Puzzlers, …) | Needs editorial lists + licensing; not self-hosted friendly |
| Social clubs / buddy-read trophies (Fable) | Multi-user is M16 |
| Mood-tag achievements | Need StoryGraph-like mood taxonomy first |
| Country A–Z challenges | Metadata coverage uneven |
| Hidden “all-nighter” time-of-day | Fun but e-ink / timezone edge cases; optional later |

---

## Recommended expansion waves

1. **Wave A (data-ready):** finish ladder + streak tiers + hours_100 + marathoner/power_hour + monthly_reader + goal_met.  
2. **Wave B (metadata):** long-book tiers, genre_5, polyglot_2, rating_spread, annotator_10.  
3. **Wave C (Shelf-native):** combine_first, listen_hours, full_orbit_3, beat_last_year, year_round.  
4. **Wave D (optional seasonal):** time-boxed challenge window (start/end) like GR summer challenge — generic, no curated lists.

Keep copy in v3 journal voice (short, concrete). Prefer BO-style permanent catalogue over GR’s rotating seasonal art unless we add a challenge calendar later.

---

## Sources

1. Shelf `server/src/achievements.rs` `SEED` (10)  
2. Book Orbit `achievement-seed.ts` @ c77966cc (87)  
3. [Goodreads Achievements Guide (community)](https://github.com/BenRogersWPG/Goodreads-Achievements)  
4. [Letterboxd welcome / Pro stats](https://letterboxd.com/welcome/), Year in Review FAQs  
5. StoryGraph / Hardcover / Fable feature comparisons (App Store / Play listings and 2026 tracker roundups)  
6. M12 fly path: [`m12-stats-and-integrations.md`](m12-stats-and-integrations.md)  
