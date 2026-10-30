# Book Orbit feature roadmap

Reference: [Book Orbit](https://github.com/bookorbit/bookorbit), reviewed 2026-10-30. This is a parity target, not a claim that Shelf already implements it.

| Capability | Shelf status |
| --- | --- |
| First-account setup, login, session revocation | Implemented in foundation |
| Manual catalogue entries and persisted page progress | Implemented on web and shared mobile |
| Overview, library search, real catalogue/progress totals | Implemented on web; catalogue/totals on mobile |
| Android and iPhone shared client | Foundation with login, catalogue, add book, record progress |
| Single-container Compose packaging | Implemented; published image begins after a successful main release |
| Independent semantic server/mobile releases | Workflow implemented; publication verified only after main release |
| EPUB upload/import and authenticated downloads | Implemented on this branch; exact-byte deduplication and embedded raster covers |
| Folder scanning and multiple libraries | Planned |
| EPUB reading with chapter navigation, text size, and saved CFI/percentage | Implemented on web and shared Flutter WebView; verification recorded in PR |
| KEPUB, MOBI, AZW, FB2 reading | Planned |
| PDF reading | Planned |
| Comic archive reading | Planned |
| Audiobook playback and background media controls | Planned |
| EPUB Media Overlay read-along and alignment | Planned |
| Offline downloads and conflict-aware sync | Planned |
| Annotations, highlights, searchable/exportable notes | Planned |
| Kobo and KOReader integrations | Planned |
| OPDS and Send-to-Kindle | Planned |
| Metadata providers and cover enrichment | Planned |
| Authors, series, smart scopes, collections | Planned |
| Session-based statistics, heatmaps, streaks, Reading DNA | Planned; current totals use manual page progress only |
| Annual goals, monthly challenges, achievements | Planned |
| Hardcover, Readwise, StoryGraph | Planned; external accounts remain unconfigured |
| Multiple users, roles, OIDC/SSO | Planned; initial foundation is one account |
| Automated ingestion and book requests/download clients | Planned |
| Reading journal | Planned; approved design includes a journal panel |
| Localization | Planned |
| Signed Android/iOS distribution and stores | Deferred by user; artifacts first |

## Next implementation milestones

1. Folder scanning, editable metadata, and larger-library storage/performance.
2. PDF reading and offline mobile access; extend the shared EPUB reader with annotations.
3. Audio playback and synchronized positions/highlights.
4. Rich organization and reading-session statistics.
5. External integrations, administration, and the remaining parity targets.

Every milestone needs a branch/PR, meaningful integration tests, updated API contracts, and an accurate entry in this table. Do not implement document parsers from scratch or enable paid services to fill a feature gap.

## EPUB milestone handoff, 2026-10-30

Local verification passed Rust formatting, Clippy, four Rust tests, eleven Flutter tests and analysis, five web tests, nine release-policy tests, and actionlint. Three user-provided EPUBs imported into the private local preview; browser checks confirmed cover rendering, chapter navigation, and restoration of a saved position. The user files and preview database are Git-excluded. Public tests use only synthetic fixtures.

Docker container builds, the extended Compose persistence check, and Android/iPhone artifact builds still require this branch's GitHub run. The embedded mobile reader has not been exercised on a physical device or emulator. This work is not merged or released; v0.1.0 remains the published foundation. Folder scanning, offline access, annotations, DRM/obfuscated resources, and other reading formats remain separate work.
