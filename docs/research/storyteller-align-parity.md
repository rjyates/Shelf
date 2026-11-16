# Storyteller align / readalong parity map

Upstream: [@storyteller-platform/align](https://gitlab.com/storyteller-platform/storyteller/-/tree/main/libraries/align) (`libraries/align`) — MIT, Shane Friedman.
Shelf: Rust reimplementation under `server/src/align/`.

**Compared against Storyteller commit** [`2de166da`](https://gitlab.com/storyteller-platform/storyteller/-/commit/2de166daa9eb5b2b120fd639434f856c19cad823) (local clone under `~/.hermes/cache/scratch/storyteller`). Links use `…/blob/main/…` for readability; pin that SHA when auditing diffs.

**Verdict (2026-11-16):** **CTC→MO EN Foliate production path at Storyteller parity** (close-first #1–#10 + residual waves A–E + SoT drain + quality close: fuller `model.ts` surface, Intl/`locale-currency` maximize rarer locales, report first/last/preview + audioChapters holes). Four Lamps on SoT image **PASSED** (90 clips, 1/90 &lt;50ms, ST span ids). **Remaining quality residuals (not lite critical-path):** intentional Shelf cursor soft-fail (ST has no identical guard), ICU/CJK east-Asian postprocessing (ST default off), grapheme clusters (Unicode-scalar shortcut; `unicode-segmentation` not in lock). Whisper VAD/UTF-16 residual API **shipped** (`vad.rs` + `map_transcription_timeline`; production Combine still CTC). **RotK on SoT image PASSED (user retest 2026-11-16):** work ready, ~12.9k clips, median ~4.5s, not SMIL-collapse.

Severity key:

| Severity | Meaning |
| --- | --- |
| **blocks-combine** | Empty/failed align on real books (RotK class) |
| **critical** | Readalong audio/text silent or structurally wrong |
| **high / quality** | Wrong chapter set, timings, or packaging for common inputs |
| **medium / polish** | Locale, reports, optional ST product surfaces |
| **defer** | Explicitly out of Foliate EN Combine scope for now |

---

## Layer summary

| Layer | Verdict | Notes |
| --- | --- | --- |
| CTC emit (ghost-story) | **Done** (runtime) | In-image; GPU tags `:latest` / `:cuda` / `:rocm` (AMD = WebGPU+Mesa) |
| CTC math (Viterbi / greedy / RANSAC) | **EN parity** | Algorithms + #3 + #8 + waves A/C + SoT drain; default Combine = sentence; rarer Intl/`locale-currency` maximize **shipped**; C++ skipped |
| Exclusions / spine classify | **COMPLETE** (algorithm) | `FILENAME_TEMPLATES` + zones + `classify`/`decide` + `AudioExclusion`/`excluded_by_rule` + title NFKC; residual: classification report UI defer; RotK live verify **done** (2026-11-16) |
| Markup / segmentation | **EN+locale close** | #4 + wave D + ST ids + Mapping + sequential wrap + CLDR suppressions + Echogarden words + fuller model.ts surface (incl. ResolvedPos); residual: ICU/CJK |
| Interpolate / holes / score>0 | **Close** | Core EN path shipped |
| errorAlign | **EN close** (#7) | Token beam + recursive narrowing + BacktraceGraph node + token-span MATCH; residuals: C++ native, production slug-atomic (not char `<>` beam), full transliteration tokenizer |
| Media overlay / SMIL packaging | **EN parity** | #2 holes + #5 seq + #9 EPUB2 + wave A–E; default `TextRef::IdFragment` = ST `{chapterId}-s{N}` |
| Audio chapters | **EN+locale close** (#6/#8/wave A) | plan/write + POSITIONAL_TITLES + track titles; Report v4 audio-only spine + holes **shipped** |
| Reports / guided nav / Readium | **wave E + UI polish** | `report.rs` v4 first/last/preview + audio-only holes; WebPub contributor roles / `inferPageCount` / TOC·landmarks·pageList **shipped**; residual: classification signals UI defer |
| Whisper production path | **Intentional omit** | Test/cargo only |

---

## 1. Exclusions (close-first #1 — algorithm COMPLETE)

Storyteller:

- [exclusionRules.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/ctc/exclusionRules.ts)
- [exclusions.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/ctc/exclusions.ts)
- [titleRules.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/ctc/titleRules.ts)
- [exclusions.test.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/ctc/__tests__/exclusions.test.ts)

Shelf: `server/src/align/exclusions.rs` (+ `spine.rs`, `pipeline.rs`).

**COMPLETE (algorithm):** `FILENAME_TEMPLATES` (Calibre `index_split_*`), spine zones (`ZONE_FRACTION=0.15` / `ZONE_MIN_ITEMS=6`), back-zone-only `index`, zone-scoped filename/title rules, full `EXCLUSION_RULES` (landmark/content-type/guide/filename review+include+divisions), `CONTENT_TYPE_SHARE=0.5` outermost types, multi-signal `classify_spine_item` + ranked `decide`, mechanical sources + `chapter_aligned_from`, `excluded_by_rule`, `AlignmentDecisions.audio: Vec<AudioExclusion>` (ST type parity; Aligner mask consumption waits for #3), `linear` / `media-type` / `no-text` / `nav`, title rules with locales (en+nl/de/fr/es/it/pt), percent-decode `normalize_href`, spine lists all itemrefs (exclusions classify).

Unit coverage mirrors ST: `index_split` back Include, 44-item `index_split_*` spine none filename-exclude, bare `index` back Exclude + `excluded_by_rule`, mid-body `newsletter` Include, `chapter` beats filename, `linear=no`, dedication Review still aligns, cover/guide/landmark/nav/copyright/about-author.

| Residual | Notes | Severity |
| --- | --- | --- |
| Title NFKC | ST `normalizeTitle` NFKC | **shipped** — `normalize_title` runs `nfkc()` via `unicode-normalization` before lower/apostrophe strip | **done** |
| Classification report UI | `signals[]` / `decidedBy` collected but not surfaced | **defer** |
| RotK live verify | SoT image `sha256:8d387270…` (2026-11-16): work ready, `readaloud_book_id=17`, package `365b8e15…`, **12,960** clips / **78,742s** audio, median **~4.54s**, **14.1%** &lt;50ms, **0** hour-scale — not SMIL-collapse; user retest OK | **done** |
| STEM preserve + emit cache | `reset_work_dir_preserving_stems` + `ctc_emit` reuse; SoT RotK re-SYNC reused staged `narration.stem` (2026-11-16) | **done** (proven) |
| Audio mask consumption | **shipped** (wave A) — `apply_emission_masks` (`MASKED_LOG_PROB=-1e4`) + search `excluded_ranges` | **done** |

## 2. CTC Aligner / forced align / search / emissions

Storyteller:

- [Aligner.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/ctc/Aligner.ts)
- [align.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/ctc/align.ts)
- [forcedAlign.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/ctc/forcedAlign.ts)
- [search.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/ctc/search.ts)
- [emissions.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/ctc/emissions.ts)
- [greedyDecode.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/ctc/greedyDecode.ts)

Shelf: `ctc_align.rs`, `ctc.rs`, `ctc_search.rs`, `ctc_emit.rs`, `stem.rs`.

**Already close:** Viterbi + `STAR_SCORE=-3`, `drop_labels`, RANSAC constants, `MAX_SEARCH_LENGTH` chunking, dominant-run + quality gates, `SENTENCE_PAD` / `refine_start_frame` + multi-file floor, `frame_map` concat, greedy `|` skip, emit client. CTC slugify matches ST Aligner `separator=""` (no `|` in query/baseline — pipe separators broke n-gram search on RotK).

| Gap | Storyteller | Shelf today | Severity |
| --- | --- | --- | --- |
| Prior-chapter `excludedRanges` | `getMatchedBoundaries` → `findCtcBoundaries(..., excludedRanges)` | **shipped** — pipeline accumulates matched chapter spans into next `align_chapter` | **done** (#3) |
| `narrowToAvailableBoundary` | Shrink search window after consumed audio | **shipped** — `narrow_to_available_boundary` + 4 ST unit cases | **done** (#3) |
| Search miss | Chunk `!boundaries` → `continue` (skip) | **shipped** — miss / narrow `start==end` → empty chunk (no full-window fallback) | **done** (#3) |
| Chapter pre-search + evidence merge | Full-chapter search then merge evidence/matched into chunks | **shipped** — pre-search + first/last chunk merge in `align_sentences_ctc` | **done** (#3) |
| OpenCC / locale slugify | [slugify.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/slugify.ts) OpenCC + transliteration | **shipped** (#8 + wave C + quality) — OpenCC + NFKD/Cyrillic + en/fr/ru cardinals + maximize/`locale-currency` shaped table (EN `$£€`, euro `€`, ja/zh `¥`, ko `₩`, sv/no/da `kr`, pl `zł`, cs `Kč`, ru `₽`, pt-BR `R$`, hi `₹`, tr `₺`, th `฿`, he `₪`, hu `Ft`, uk `₴`) + Mapping + `slugifiedWordRanges` + CLDR `segmentText` | **done** (#8/wave C/quality) |
| CTC word ranges from alignments | `buildWordRanges` / `slugifiedWordRanges` | **shipped** — helpers + CTC `AlignGranularity::Word` (`ctc_word_starts_for_sentence` → `build_word_ranges` → `CtcWordClips` → `expand_to_word_ranges_ctc` / `inject_word_spans`); default Combine = sentence; Whisper path still EA/proportional | **done** |
| Multi-file span → multi clips | `framesToAudioRanges` one clip per crossed file | **shipped** — `frames_to_audio_ranges` | **done** |
| Per-file `EmissionTimings` | `EmissionsReader.timings[]` | **shipped** — `CtcBookEmissions.file_timings` parallel to `frame_map`; `frames_to_audio_ranges` / masks / package duration / pipeline durations use per-STEM timings; boundary offsets still first-file (ST); emit rejects mismatched `seconds_per_frame` | **done** |
| Manual audio masks | `excludedAudio` → `withMasks` (blank emission rows) | **shipped** (wave A) — `apply_emission_masks` + search exclude; C++ native blank path skipped (Rust-only) | **done** |
| Audio↔emissions pairing check | `checkAudioEmissionsPairing` | **shipped** — `check_audio_emissions_pairing` after `emit_book` | **done** |
| Lazy STEM / decode cache | `decodedEmissions` + `ngramIndexes` WeakMaps (+ `blocks()` streaming) | **shipped** — `CtcBookEmissions.decoded` OnceLock + `PositionedDocument` ngram OnceLock (ST search.ts caches); full in-memory STEM load kept; `stem_blocks` / `BLOCK_FRAMES=1000` streaming decode (ST `EmissionsReader.blocks`) | **done** |
| Subword tokenizer reject | `tokenizer !== null` throws at align | **shipped** — `reject_subword_tokenizer` after STEM decode in `emit_book` (ST `align.ts` message; STEM still decodes UNIGRAM headers) | **done** |
| Weak-score → `droppedSlots` | `droppedSentences` for interpolate | **shipped** — `SentenceAudioOutcome::Drop` when weak score + within evidence + full-span `framesPerChar < 2` (ST buildSentenceRanges side-path); soft Skip otherwise | **done** |

---

## 3. Markup / segmentation

Storyteller: [libraries/align/src/markup/](https://gitlab.com/storyteller-platform/storyteller/-/tree/main/libraries/align/src/markup) — especially [transform.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/markup/transform.ts), [segmentation.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/markup/segmentation.ts), [markup.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/markup/markup.ts), [parseDom.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/markup/parseDom.ts), [model.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/markup/model.ts).

Shelf: `markup.rs` (+ `words.rs`).

**Already close (EN Combine):** in-place sentence wrap, cross-piece reopen (h1→p), EN abbrev/ender/trailing/min-letter, ruby `rt`/`rp` skip, SVG/script/style skip, **block-boundary `\n` + footnote lift/restore** (#4).

| Gap | Storyteller | Shelf today | Severity |
| --- | --- | --- | --- |
| Block-boundary forced sentence breaks | `liftText` inserts `\n` at textblock ends so titles/paragraphs do not merge | **shipped** — `lift_text` BLOCKS + trailing br/img + inter-block leading strip; synth `\n` not in offset map | **done** (#4) |
| ProseMirror-like DOM model | `Root`/`Node`/`TextNode`/`Mark`/`FootnoteNode` + descendants/findIndex/replace/cut + ResolvedPos | **shipped** (quality) — attrs/marks/flags/textContent + descendants/findIndex/replace/cut/FootnoteNode + ResolvedPos (`MarkRoot::resolve`); Piece wrap still production path | **done** (quality) |
| Real XHTML parse → model | `parseDom` / blocks vs inline / footnote roles | **wave D** — xmltree body flatten → Piece (regex fallback); skip script/style/svg/rt/rp | **partial** |
| Footnote **lift** | Move footnote body to noteref → segment/mark → restore | **shipped** — `inline_footnotes` / `restore_footnotes`; unpaired footnote bodies still segmented | **done** (#4) |
| Footnote id via `noteref` href | `FOOTNOTE_ROLES` + href `#id` without `epub:type` | **shipped** — noteref href ids collected; role or id match | **done** (#4) |
| Full Echogarden `segmentText` | CLDR locales + noun/TLD/number/date/phone + word tokens | **shipped** — `suppressions.rs` CLDR en/de/es/fr/it/pt/ru + `split_sentence_spans_locale` / `markup_xhtml_body_locale` from OPF `dc:language`; EN extras keep Dr./A.I.; residual: ICU date/phone pattern depth | **done** (quality) |
| ICU / CJK east-Asian postprocessing | `enableEastAsianPostprocessing` (ST currently `false`) | Deferred (matches ST default) | **defer** |
| Word markup in-chapter | Echogarden words, `{chapterId}-s{i}-w{j}` | **shipped** — Echogarden-style `tokenize_words` (suppressions/dotted/number + punct tokens); `inject_word_spans_locale` keeps punct between spans; ST ids | **done** |
| Span ID / vocab shape | `{chapterId}-s{i}`, `epub:type=storyteller:sentence-span`, prefix | **shipped** — ST `{chapterId}-s{i}` XHTML + SMIL `text_id` (default); `sentence-N` only when markup `chapter_id` is None | **done** |
| Mapping invert for lift offsets | `@storyteller-platform/mapping` | **shipped** — `Mapping::invert` / `invert_cursor` / `append_mapping`; markup wrap sequential `split_sentence_spans` (no `plain.find`) | **done** |

---

## 4. Match / interpolate / holes / audio chapters / errorAlign

Storyteller:

- [getSentenceRanges.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/getSentenceRanges.ts)
- [interpolateSentenceRanges.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/interpolateSentenceRanges.ts)
- [holes.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/holes.ts)
- [audioChapters.tsx](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/audioChapters.tsx)
- [slugify.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/slugify.ts)
- [search.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/search.ts) (Whisper-path)
- [errorAlign/](https://gitlab.com/storyteller-platform/storyteller/-/tree/main/libraries/align/src/errorAlign)
- [process/ranges.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/process/ranges.ts) (VAD / safe cuts)

Shelf: `interpolate.rs`, `holes.rs`, `audio_chapters.rs`, `slugify.rs`, `search.rs`, `match_score.rs`, `error_align.rs`, `ranges.rs`, `words.rs`, `pipeline.rs`.

**Already close:** `interpolate_sentence_ranges` (+ multi-file, `drop_unread`), `collapse_ranges` / `place_dropped`, `enforce_monotonic` / `expand_empty`, score>0 matched-only → interpolate, `findBoundaries` (+ short exact slug).

| Gap | Storyteller | Shelf today | Severity |
| --- | --- | --- | --- |
| Locale slugify | Currency/number→words, transliteration, OpenCC, `Mapping` | **shipped** (#8 + wave C + quality) — OpenCC + transliterate + en/fr/ru cardinals + maximize currency table (rarer locales) + Mapping invert/append + `slugifiedWordRanges` + CLDR `segmentText` | **done** |
| Beam + native `errorAlign` | `alignBeamSearch` / `native.ts` / cpp | **shipped** (#7) — Rust port of TS `errorAlignBeamSearch` on `-` slug tokens (Path/expand/prune/normCost/sortId/beamSize=100/unambiguous collapse); word-level pass = BacktraceGraph unambiguous MATCH → beam on gaps | **done** (#7); residuals: C++ native accel (optional), production still slug-atomic (not char `<>` beam); full transliteration tokenizer/normalizer |
| Narrowing recursion | Recursive re-slice | **shipped** (#7) — recurse + compose absolute slice offsets (ST `getSentenceRanges`) | **done** (#7) |
| Unambiguous MATCH graph | `BacktraceGraph.getUnambiguousNodeMatches` + `getUnambiguousTokenSpanMatches` | **shipped** — multi-path DAG node matches in word-level pass; `START_DELIMITER`/`END_DELIMITER` (`<>`) + `get_unambiguous_token_span_matches` on embed strings (beam collapse still frequency-based) | **done** |
| Multi-file sentence clip when start≠end file | Clip continues on end file | **shipped** — `frames_to_audio_ranges` multi-clip | **done** |
| Word tokens | Echogarden `sentence.words` | **shipped** — `tokenize_words` / non-punct `split_words` (CLDR+noun/TLD) | **done** |
| `planAudioChapters` full | Track/chapter titles, ≥50% titled track, `excludedBetween` reuse | **shipped** (#6 + wave A) — plan + `track_titles_from_audio_paths` (`.source.json` / ffprobe) | **done** (#6/wave A) |
| `writeAudioChapters` | Locale titles, spine insert at anchor, nav TOC, attach existing excluded | **shipped** (#6/#8) — spine insert + lastInserted, nav TOC, attach existing + POSITIONAL_TITLES locales from OPF `dc:language` | **done** (#6/#8) |
| VAD safe chapter cuts | `getSafeChapterRanges` / `chooseChapterCut` | **shipped** — `vad.rs` (`choose_chapter_cut` / `move_chapter_cuts_to_silence` / `get_safe_ranges` / `get_safe_chapter_ranges`); ffmpeg `silencedetect` + injectable silences (unit tests synthetic; no ghost-story VAD); CTC may not call yet | **done** (Whisper residual API) |
| UTF-16 timeline map | `mapTranscriptionTimeline` + mapping | **shipped** — `map_transcription_timeline` + `SlugTimeMap` UTF-16 code units (byte→UTF-16 at lookup); BMP ≡ scalar | **done** (Whisper polish) |
| `collapseSentenceRangeGaps` | Expand clips to next/file-end | **shipped** — `collapse_ranges` bridges small gaps / holes for large; OOO-safe | **done** |

Whisper as production Combine: **intentional Shelf omit** (not a close-the-gap item).

---

## 5. Media overlay / EPUB packaging / reports / Readium

Storyteller:

- [mediaOverlay.tsx](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/ctc/mediaOverlay.tsx)
- [upgradeEpub.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/upgradeEpub/upgradeEpub.ts)
- [textFragments.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/textFragments.ts)
- [guidedNavigation.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/readium/guidedNavigation.ts)
- [manifest.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/readium/manifest.ts)
- [report-v4.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/report-v4.ts)
- [reportBuilder.ts](https://gitlab.com/storyteller-platform/storyteller/-/blob/main/libraries/align/src/align/reportBuilder.ts)

Shelf: `readaloud.rs`, `smil.rs`, `pipeline.rs`, `audio_chapters.rs`.

**Already OK for Foliate EN:** manifest `media-overlay` on `<item>`, per-MO + package `media:duration`, `media:active-class` = `-epub-media-overlay-active`, `MediaOverlays/` + `Audio/` layout, relative SMIL paths, audio-chapter spine insert at anchor (#6).

| Gap | Storyteller | Shelf today | Severity |
| --- | --- | --- | --- |
| In-chapter hole `<par storyteller:audio-only>` | `holesAround` / remaining → `writeAlignedChapter(..., holes)` | **shipped** — `attach_remaining_holes` → SMIL before/after pars + duration includes holes | **done** (#2) |
| Block / sentence SMIL hierarchy | `text-range-large` / `text-range-small` seq nesting + word pars | **shipped** (#5 + wave B) — chapter seq + per-sentence `text-range-small`; `text-range-large` when block map present (`TextRef::TextFragment`) | **done** |
| Text Fragment URL refs | `TextFragmentFactory` / `--text-ref` | **shipped** (wave B) — `text_fragments.rs` + `TextRef`; default `IdFragment` (`#sentence-N`); optional `TextFragment` (`#:~:text=`) | **done** (default id-fragment) |
| EPUB2→3 upgrade | `upgradeEpub` before align | **shipped** (#9 + wave A) — upgrade + `collectManifestProperties` (svg/scripted/mathml/switch); EPUB3 no-op; residuals: epubcheck, cover-meta polish | **done** (#9/wave A) |
| Package `media:duration` basis | Sum of **audio file** durations | **shipped** (wave A) — `package_audio_duration_secs` from frame_map; per-MO still clip+holes | **done** |
| Per-chapter duration includes holes | `getChapterDuration([...sentences, ...holes])` | **shipped** — `overlay_duration_secs(sentences, holes)` | **done** (#2) |
| SMIL `epub:prefix` + vocab + state types | `SMIL_PREFIX` / `stateType` on every par/seq | **shipped** (#5) — prefix + `storyteller:{matched\|interpolated\|unmatched\|dropped}` on seq/pars; audio-only holes keep type | **done** (#5) |
| Chapter `<seq epub:type="chapter">` wrapper | Root seq with `epub:textref` | **shipped** (#5) — `{chapterId}_overlay` | **done** (#5) |
| Readaloud CSS asset + XHTML link | `Styles/storyteller-readaloud.css` | **shipped** (#5) — CSS in package + `<link>` in marked XHTML heads + OPF manifest | **done** (#5) |
| Audio href encode | `encodeURIComponent(basename)` | **shipped** (#5) — `encode_uri_component` on basename for sentence + hole clips | **done** (#5) |
| Align Report v4 | `assembleReport` / `buildAlignerReport` | **shipped** (wave E + quality) — assemble/spans + package JSON; `build_report_from_chapters` fills first/last match, preview, audio-only spine + hole after-0 | **done**; residual: excludedAudio / classification signals UI defer |
| Readium Guided Navigation + WebPub | `generateGuidedNavigationDocuments` / `generateReadiumManifest` | **shipped** (wave E + quality) — `GND/{id}.json` + `GND/manifest.json` + root `manifest.json`; contributor roles (aut/art/clr/ill/ink/ltr/nrt/pnc/trl/pbl + `dc:contributor`), `inferPageCount` (1024-byte ceil), optional toc/landmarks/pageList; Foliate SMIL path unchanged | **done** |
| Audio-chapter packaging fidelity | Insert at anchor, nav TOC, locale titles, reuse excluded | **shipped** (#6/#8 + quality) — insert/nav/attach + locale titles; Report v4 audio-only spine/holes polish **shipped** | **done** |

---

## Close-first queue (full parity)

Ordered for **complete** Storyteller readalong parity on the CTC path — not "lite done":

1. ~~**Exclusions full port**~~ — **algorithm COMPLETE** (`exclusions.rs` classify/decide/zones/templates/`AudioExclusion`/`excluded_by_rule`). RotK live verify deferred (STEM preserve+cache required).
2. ~~**Wire remaining holes into SMIL**~~ — **shipped** (`attach_remaining_holes` → `storyteller:audio-only` pars; duration includes holes). Seq hierarchy completed in #5.
3. ~~**CTC Aligner orchestration**~~ — **shipped** (prior-chapter `excludedRanges`, `narrow_to_available_boundary`, search-miss skip, chapter pre-search evidence/matched merge; audio masks → blank+search). Wave A: emission-row blanking. Multi-file `framesToAudioRanges` + `buildWordRanges`/`slugifiedWordRanges` + CTC Word wire + per-file timings + audio↔emissions pairing + subword tokenizer reject + lazy STEM decode/ngram cache shipped. **Shelf soft-fail:** discontinuous leap to outro (match in last 10% while prior CTC cursor still in first half) — replaces spine-index front-quarter heuristic; ST has no identical guard. C++ native skipped (Rust-only).
4. ~~**Markup block boundaries + footnote lift**~~ — **shipped** (`lift_text` BLOCKS/`\n`, `inline_footnotes`/`restore_footnotes`; unpaired footnotes segmented). ST ids + Mapping + sequential wrap + CLDR suppressions + Echogarden words + fuller model.ts surface (attrs/flags/descendants/findIndex/replace/cut/FootnoteNode/ResolvedPos).
5. ~~**SMIL structure**~~ — **shipped** (chapter + `text-range-small` seq, `epub:prefix` / `stateType`, basename `encodeURIComponent`, `Styles/storyteller-readaloud.css` + XHTML link + OPF). Wave A: package `media:duration` from audio-file frame_map. Wave B: Text Fragment factory + optional `text-range-large`/block map (`TextRef`, default id-fragment). EPUB2 closed in #9; Report/Readium closed in wave E.
6. ~~**Audio chapters full**~~ — **shipped** (`planAudioChapters` titles/`excludedBetween` + `writeAudioChapters` spine insert/nav/attach + #8 positional locales + wave A track titles + Report v4 audio-only spine/holes polish).
7. ~~**errorAlign beam/native** + recursive narrowing~~ — **shipped** (token beam + recursive narrowing + BacktraceGraph `getUnambiguousNodeMatches` + `getUnambiguousTokenSpanMatches`/`<>` delimiters; beam collapse still frequency-based; C++ native residual; production slug-atomic; full transliteration tokenizer residual).
8. ~~**Locale / OpenCC slugify + titleRules locales**~~ — **shipped** (OpenCC + transliterate/to-words + maximize/`locale-currency` rarer locales + Mapping + `slugifiedWordRanges`; POSITIONAL_TITLES; titleRules 27/27; CLDR suppressions + Echogarden words). shortcut: Unicode scalars only (slugify/text_fragments/CTC positions); `unicode-segmentation` not in `Cargo.lock` — upgrade when ZWJ/emoji grapheme spans matter.
9. ~~**EPUB2 upgrade** path~~ — **shipped** (`upgrade_epub_in_place` after worker writes `source.epub`; ST metadata/nav/guide/doctype/fonts + wave A `collectManifestProperties`; residuals: epubcheck, cover-meta polish).
10. ~~**Report v4 + Readium GND/WebPub**~~ — **shipped** (wave E + quality: first/last/preview + audio-only holes in `build_report_from_chapters`; `GND/` + WebPub roles/`inferPageCount`/toc·landmarks·pageList). Residual: classification signals UI defer. Still defer: ICU/CJK postprocessing (ST default off), Whisper production.

---

## Product (Shelf) — not ST library gaps

| Item | Status |
| --- | --- |
| Merge & Combine, one library card, live progress, Compose-only, CTC-only image | **done** |
| Four Lamps CI gate | **done** |
| RotK brownfield long-book measure | **owed** — `probe/rotk_full.stem` intact; rebuild SoT image then STEM-reuse re-SYNC + SMIL histogram |
| M14 native Rust CTC emit | Roadmap (`docs/parity.md`) — not required for ST align parity |

---

## Verification

- Unit: `cargo test --manifest-path server/Cargo.toml --lib align::` + clippy `-D warnings`
- Short path: Four Lamps Compose gate
- Long path: RotK re-SYNC after STEM preserve+cache; SMIL histogram + `storyteller:audio-only` hole coverage
- Regression: ST-shaped exclusion tests for `index_split_*` (incl. 44-item spine) / newsletter / chapter-beats-filename **added**; SMIL audio-only before/after hole tests **added**

## How to update this doc

When closing a gap: change the row severity/status, link the Shelf commit or module, and remove it from the close-first queue. Never mark a layer **done** while a Storyteller guard for that layer is still `shortcut:` / lite in code.
