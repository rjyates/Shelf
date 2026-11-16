# Storyteller alignment — design attribution

Shelf’s EPUB↔audiobook alignment pipeline (`server/src/align/`) is a Rust
reimplementation of the approach used by Storyteller’s
`@storyteller-platform/align` (`stalign`) library:

- Upstream: https://github.com/smoores-dev/storyteller (also GitLab storyteller-platform)
- Package: `libraries/align` / `@storyteller-platform/align`
- License: MIT (see `LICENSE.txt` in this directory)
- Copyright: Shane Friedman

We follow the same stage structure (process → transcribe/emit → markup → align →
readaloud EPUB with Media Overlays) and port the solved problems (chapter
locate via n-gram boundary voting, monotonic audio-range enforcement, SMIL
emission, source files left intact). This is not a verbatim copy of the
TypeScript sources; algorithms and tests are adapted for Shelf’s Rust server.

## Shelf ports of Storyteller algorithms

- `enforceMonotonicAudioRanges` with global timeline + 30s out-of-order keep
- `collapseRanges` / hole detection (`holes.ts`)
- `interpolateSentenceRanges` including multi-file left-remainder / right-head + `chapterSplit`
- Matched-only anchors + Levenshtein `score > 0` gate (`match_score.rs`)
- Windowed `errorAlign` lite (token DP) + word-level unique-MATCH pass + `errorAlignWithNarrowing`
- `interpolateSentenceRanges` `dropUnread` via `InterpolateOptions`
- In-place XHTML markup + footnote/ruby skip (not Echogarden)
- Word granularity: EA walk on matched hyp spans (`words.rs`)
- `planAudioChapters` + EPUB `AudioChapters/` packaging (`audio_chapters.rs`)
- Filename spine exclusions (`exclusions.rs`)
- CTC frame math + Viterbi/greedy + `ctcForcedAlign` + STEM decode (`stem.rs`); CTC emit runtime **done** (ghost-story in Shelf image + `ctc_emit` client + pipeline)
- Multi-chapter `alignBook`-style spine walk + per-chapter SMIL
- Whisper→MO algorithms remain in-tree for cargo/`align-script` tests; production Combine is CTC-only

Living gap/backlog: `docs/research/storyteller-align-parity.md`.

