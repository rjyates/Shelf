//! CTC Aligner (Storyteller `Aligner.ts` subset).
//!
//! Emit STEM → decode → RANSAC CTC search + anchors → `ctc_forced_align` with
//! drop_labels → dominant-run sentence ranging / quality gates.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::ctc::{
    boundary_offset_frames, ctc_forced_align, detect_dropped_labels, frames_to_seconds,
    seconds_to_frames, EmissionTimings, Emissions, ForcedAlignAnchor, TokenAlignment,
};
use super::ctc_emit::{CtcEmitClient, CtcEmitOpts};
use super::ctc_search::{
    decode_positioned_document, find_ctc_boundaries_in_document, PositionedDocument,
};
use super::exclusions::AudioExclusion;
use super::ranges::{AudioRange, RangeState, SentenceRange};
use super::stem::{decode_stem_file, reject_subword_tokenizer, StemHeader};

/// Longest silence (frames) tolerated inside one sentence before a split.
const MAX_INTRA_SENTENCE_GAP_FRAMES: i32 = 150;
/// Storyteller `MAX_SEARCH_LENGTH` — chunk chapter baseline when longer.
const MAX_SEARCH_LENGTH: usize = 40_000;
/// Storyteller `SENTENCE_PAD` for `refineStartFrame`.
const SENTENCE_PAD_FRAMES: i32 = 25;

/// Concatenated book emissions + per-file frame spans (Storyteller `frameMap`).
#[derive(Debug)]
pub struct CtcBookEmissions {
    pub header: StemHeader,
    pub emissions: Emissions,
    /// `(audiofile, start_frame, end_frame)` exclusive end, in concatenated frame space.
    pub frame_map: Vec<(String, usize, usize)>,
    /// Parallel to `frame_map` — ST `EmissionsReader.timings[]` (offsets may differ per STEM).
    pub file_timings: Vec<EmissionTimings>,
    /// ST `decodedEmissions` WeakMap — greedy decode once after masks, reuse per search.
    decoded: OnceLock<PositionedDocument>,
}

impl Clone for CtcBookEmissions {
    fn clone(&self) -> Self {
        Self {
            header: self.header.clone(),
            emissions: self.emissions.clone(),
            frame_map: self.frame_map.clone(),
            file_timings: self.file_timings.clone(),
            decoded: OnceLock::new(),
        }
    }
}

impl CtcBookEmissions {
    /// Drop cached greedy decode (ST `withMasks` → new reader → empty WeakMap).
    pub fn clear_decoded(&mut self) {
        self.decoded = OnceLock::new();
    }

    /// Storyteller `decodedEmissions.get(emissionsReader)` — decode after masks.
    pub fn positioned_document(&self) -> Option<&PositionedDocument> {
        if self.decoded.get().is_some() {
            return self.decoded.get();
        }
        let cv = char_vocab(&self.header.vocab);
        let doc = decode_positioned_document(&self.emissions, &cv, self.header.blank_index)?;
        let _ = self.decoded.set(doc);
        self.decoded.get()
    }
}

type DroppedSlots = HashSet<(String, usize)>;
/// Per matched sentence: CTC word clips from `buildWordRanges` (ST always emits).
type CtcWordClips = HashMap<(String, usize), Vec<AudioRange>>;
type ChapterAlignOut = (Vec<SentenceRange>, DroppedSlots, CtcWordClips);
/// `((ranges, dropped, word_clips), last_abs_end_frame, matched_frame_span)`.
type ChapterAlignResult = (ChapterAlignOut, i32, Option<(usize, usize)>);

/// Storyteller `narrowToAvailableBoundary`: merge overlapping/touching consumed
/// regions, then keep the largest available gap intersecting `boundary`.
pub fn narrow_to_available_boundary(
    matched: &[(usize, usize)],
    boundary: (usize, usize),
) -> (usize, usize) {
    let (b_start, b_end) = boundary;
    let mut sorted: Vec<(usize, usize)> = matched.to_vec();
    sorted.sort_by_key(|(s, _)| *s);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in sorted {
        if let Some(prev) = merged.last_mut() {
            if start <= prev.1 {
                prev.1 = prev.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }

    // Storyteller: available = [-1, ...merged.flat(), Infinity], paired as gaps.
    let mut available: Vec<i64> = vec![-1];
    for &(s, e) in &merged {
        available.push(s as i64);
        available.push(e as i64);
    }
    available.push(i64::MAX);

    let mut within: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i + 1 < available.len() {
        let start = available[i];
        let end = available[i + 1];
        let intersects = (b_start as i64 <= start && b_end as i64 >= start)
            || (b_start as i64 <= end && b_end as i64 >= end);
        if intersects {
            let n_start = (b_start as i64).max(start + 1);
            let n_end = (b_end as i64).min(end - 1);
            if n_start <= n_end {
                within.push((n_start as usize, n_end as usize));
            }
        }
        i += 2;
    }

    let largest = within.into_iter().max_by_key(|(s, e)| e.saturating_sub(*s));
    match largest {
        Some((s, e)) => (s, e),
        None => (b_start, b_end),
    }
}

/// Storyteller `MASKED_LOG_PROB` — blanked emission rows force blank argmax.
pub const MASKED_LOG_PROB: f32 = -1e4;

/// Storyteller Aligner audio `withMasks` mapping: track seconds → absolute frames.
///
/// Seed CTC search `excluded_ranges` and pass the same ranges to
/// [`apply_emission_masks`] (ST blanks emission rows + search-excludes).
pub fn audio_exclusion_frame_masks(
    exclusions: &[AudioExclusion],
    frame_map: &[(String, usize, usize)],
    file_timings: &[EmissionTimings],
) -> Vec<(usize, usize)> {
    let mut masks = Vec::new();
    for ex in exclusions {
        let Some((idx, (_, file_start, file_end))) = frame_map
            .iter()
            .enumerate()
            .find(|(_, (n, _, _))| n == &ex.track)
        else {
            continue;
        };
        let timings = file_timings.get(idx).copied().unwrap_or(EmissionTimings {
            seconds_per_chunk: 1.0,
            seconds_per_frame: 0.02,
            frames_per_chunk: 50,
            start_offset_seconds: 0.0,
            end_offset_seconds: 0.0,
        });
        let file_len = file_end.saturating_sub(*file_start);
        let duration = frames_to_seconds(file_len, timings);
        let start_s = ex.start.unwrap_or(0.0).max(0.0);
        let end_s = ex.end.unwrap_or(duration).min(duration);
        if end_s <= start_s {
            continue;
        }
        let start = (*file_start).saturating_add(seconds_to_frames(start_s, timings));
        let end = (*file_end).min((*file_start).saturating_add(seconds_to_frames(end_s, timings)));
        if end > start {
            masks.push((start, end));
        }
    }
    masks.sort_by_key(|(s, _)| *s);
    masks
}

/// Storyteller `EmissionsReader.mask`: for each `[start, end)` frame range, fill
/// the row with [`MASKED_LOG_PROB`] and set `blank_id` to `0`.
pub fn apply_emission_masks(emissions: &mut Emissions, masks: &[(usize, usize)], blank_id: i32) {
    let classes = emissions.classes;
    if classes == 0 || emissions.frames == 0 {
        return;
    }
    let blank = blank_id.max(0) as usize;
    for &(start, end) in masks {
        if end <= start {
            continue;
        }
        let from = start.min(emissions.frames);
        let to = end.min(emissions.frames);
        for frame in from..to {
            let row = frame * classes;
            let row_end = row + classes;
            if row_end > emissions.values.len() {
                break;
            }
            for v in &mut emissions.values[row..row_end] {
                *v = MASKED_LOG_PROB;
            }
            if blank < classes {
                emissions.values[row + blank] = 0.0;
            }
        }
    }
}

/// Package-level unrefined `media:duration`: sum of per-file durations from
/// emission `frame_map` (ST `audioFileDurations` / `addAudio` total).
pub fn package_audio_duration_secs(
    frame_map: &[(String, usize, usize)],
    file_timings: &[EmissionTimings],
) -> f64 {
    frame_map
        .iter()
        .enumerate()
        .map(|(i, (_, start, end))| {
            let t = file_timings.get(i).copied().unwrap_or(EmissionTimings {
                seconds_per_chunk: 1.0,
                seconds_per_frame: 0.02,
                frames_per_chunk: 50,
                start_offset_seconds: 0.0,
                end_offset_seconds: 0.0,
            });
            frames_to_seconds(end.saturating_sub(*start), t)
        })
        .sum()
}

/// Storyteller `checkAudioEmissionsPairing` — audio and emission lists must
/// match 1:1 by index; a length mismatch silently shifts later pairings.
pub fn check_audio_emissions_pairing(
    audiofiles: &[String],
    emission_filepaths: &[String],
) -> Result<(), String> {
    if audiofiles.len() == emission_filepaths.len() {
        return Ok(());
    }
    let audio_longer = audiofiles.len() > emission_filepaths.len();
    let unmatched: &[String] = if audio_longer {
        &audiofiles[emission_filepaths.len()..]
    } else {
        &emission_filepaths[audiofiles.len()..]
    };
    let side = if audio_longer {
        "emissions for"
    } else {
        "audio file for"
    };
    Err(format!(
        "Found {} audio files but {} emission files. Each audio file needs its own emissions. Missing {side}:
{}",
        audiofiles.len(),
        emission_filepaths.len(),
        unmatched
            .iter()
            .map(|p| format!("  {p}"))
            .collect::<Vec<_>>()
            .join("
"),
    ))
}

/// Slugify sentence text for CTC baseline/search: OpenCC Han fold, then vocab chars.
///
/// Storyteller CTC Aligner calls `slugify(..., separator="")` — words are
/// concatenated with no `|`. Greedy decode also strips `|`, so inserting pipes
/// here breaks n-gram search (RotK: false EOF matches, collapsed SMIL).
///
/// Wave C + quality: locale number→words + transliterate + maximize currency
/// table, then OpenCC + vocab keep. Mapping invert/append in `mapping.rs`.
pub fn slugify_for_ctc(text: &str, vocab: &HashMap<String, i32>) -> String {
    slugify_for_ctc_with_locale(text, vocab, "en")
}

pub fn slugify_for_ctc_with_locale(
    text: &str,
    vocab: &HashMap<String, i32>,
    locale: &str,
) -> String {
    // Numbers→words first; OpenCC fold; transliterate only chars not in vocab
    // (ST ignore list — keep Cyrillic/Han when MMS has them).
    let prepared = super::slugify::locale_prepare_numbers(text, locale);
    let folded = super::slugify::fold_han_for_vocab(&prepared, vocab);
    let mut out = String::with_capacity(folded.len());
    for ch in folded.chars() {
        let key = ch.to_string();
        if vocab.contains_key(&key) {
            // Never keep `|` in the CTC query/baseline — decode document omits it.
            if key == "|" {
                continue;
            }
            out.push(ch);
            continue;
        }
        let roman = super::slugify::transliterate(&key);
        for c in roman.chars() {
            if c.is_ascii_alphanumeric() {
                out.push(c.to_ascii_lowercase());
            }
        }
        // Word/punct gaps: empty separator (ST), not `|`.
    }
    out
}

/// Inclusive slug offsets for non-punct words inside one CTC sentence baseline.
///
/// Concat of per-word `slugify_for_ctc` (empty separator) — ST `slugifiedWordRanges`
/// shape without Mapping when whitespace-split tokens match the sentence slug.
pub(crate) fn ctc_word_starts_for_sentence(
    sentence: &str,
    vocab: &HashMap<String, i32>,
) -> Vec<(usize, usize)> {
    let chars: Vec<char> = sentence.chars().collect();
    let mut cursor = 0usize;
    let mut out = Vec::new();
    for w in super::slugify::word_entries_whitespace(sentence) {
        if w.is_punctuation {
            continue;
        }
        let token: String = chars[w.start_offset..w.end_offset].iter().collect();
        let n = slugify_for_ctc(&token, vocab).chars().count();
        if n == 0 {
            continue;
        }
        let start = cursor;
        let end = cursor + n - 1;
        out.push((start, end));
        cursor += n;
    }
    out
}

/// Emit STEM for each audio file, decode, concatenate frames.
pub fn emit_book(
    client: &CtcEmitClient,
    audio_paths: &[PathBuf],
    work_dir: &Path,
) -> Result<CtcBookEmissions, String> {
    if audio_paths.is_empty() {
        return Err("ctc align: no audio paths".into());
    }
    let opts = CtcEmitOpts::default();
    let mut header: Option<StemHeader> = None;
    let mut values = Vec::new();
    let mut frame_map = Vec::new();
    let mut file_timings = Vec::new();
    let mut cursor = 0usize;

    for audio in audio_paths {
        let stem_path = client.emit_file(audio, work_dir, &opts)?;
        let (h, em) = decode_stem_file(&stem_path)?;
        // ST align.ts: tokenizer !== null → hard fail (char models only).
        reject_subword_tokenizer(h.tokenizer_kind)?;
        let name = audio
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("audio")
            .to_string();
        if let Some(ref prev) = header {
            if prev.classes != h.classes
                || prev.blank_index != h.blank_index
                || (prev.timings.seconds_per_frame - h.timings.seconds_per_frame).abs() > 1e-12
            {
                return Err("ctc align: emission vocab/blank/spf mismatch across files".into());
            }
        } else {
            header = Some(h.clone());
        }
        let start = cursor;
        let end = cursor + em.frames;
        values.extend_from_slice(&em.values);
        frame_map.push((name, start, end));
        file_timings.push(h.timings);
        cursor = end;
    }

    let header = header.ok_or_else(|| "ctc align: no STEM decoded".to_string())?;
    let classes = header.classes;
    Ok(CtcBookEmissions {
        header: header.clone(),
        emissions: Emissions {
            values,
            frames: cursor,
            classes,
        },
        frame_map,
        file_timings,
        decoded: OnceLock::new(),
    })
}

/// Align one chapter's sentences against book emissions (matched + dropped only).
///
/// `prev_end_frame` is kept for API compatibility with the pipeline cursor; Storyteller
/// resets `lastFoundSentenceEnd` to 0 at each chapter/chunk and does **not** floor
/// `refineStartFrame` on the prior chapter's absolute end. Cross-chapter separation is
/// `excluded_ranges` / `narrowToAvailableBoundary` only. Seeding from a prior absolute
/// end (e.g. a title-page false-match at the outro) clamps every later sentence to EOF.
/// `excluded_ranges` are prior chapters' matched frame spans (+ audio masks).
pub fn align_chapter(
    chapter_id: &str,
    sentences: &[String],
    book: &CtcBookEmissions,
    prev_end_frame: i32,
    excluded_ranges: &[(usize, usize)],
) -> Result<ChapterAlignResult, String> {
    // Call after apply_emission_masks — ST decodedEmissions keys the masked reader.
    let document = book.positioned_document();
    align_sentences_ctc(
        chapter_id,
        sentences,
        &book.emissions,
        &book.header.vocab,
        book.header.blank_index,
        book.header.timings,
        &book.frame_map,
        &book.file_timings,
        prev_end_frame,
        excluded_ranges,
        document,
    )
}

/// Split slugified sentences into ≤ `max_len` chunks (Storyteller `MAX_SEARCH_LENGTH`).
pub(crate) fn chunk_sentences_for_search(
    slugs: &[String],
    max_len: usize,
) -> Vec<(usize, Vec<String>)> {
    if slugs.is_empty() {
        return Vec::new();
    }
    let total: usize = slugs.iter().map(|s| s.len()).sum();
    let chunk_count = total.div_ceil(max_len.max(1)).max(1);
    let chunk_length = total.div_ceil(chunk_count);
    let mut chunks: Vec<(usize, Vec<String>)> = vec![(0, Vec::new())];
    let mut c = 0usize;
    for (i, s) in slugs.iter().enumerate() {
        if c >= chunk_length * chunks.len() && !chunks.last().unwrap().1.is_empty() {
            chunks.push((i, Vec::new()));
        }
        chunks.last_mut().unwrap().1.push(s.clone());
        c += s.len();
    }
    chunks
}

/// Core forced-align helper (also used by unit tests with synthetic emissions).
///
/// Returns `((ranges, dropped), last_abs_end_frame, matched_span)` where
/// `last_abs_end_frame` is `-1` when no sentence matched and `matched_span` is
/// the chapter's consumed absolute frame range for subsequent `excluded_ranges`.
#[allow(clippy::too_many_arguments)]
pub fn align_sentences_ctc(
    chapter_id: &str,
    sentences: &[String],
    emissions: &Emissions,
    vocab: &HashMap<String, i32>,
    blank_id: i32,
    timings: EmissionTimings,
    frame_map: &[(String, usize, usize)],
    file_timings: &[EmissionTimings],
    prev_end_frame: i32,
    excluded_ranges: &[(usize, usize)],
    // ST decodedEmissions cache; when None, decode once for this chapter.
    cached_document: Option<&PositionedDocument>,
) -> Result<ChapterAlignResult, String> {
    if sentences.is_empty() || emissions.frames == 0 {
        return Ok((
            (Vec::new(), HashSet::new(), HashMap::new()),
            prev_end_frame,
            None,
        ));
    }

    let slugs: Vec<String> = sentences
        .iter()
        .map(|s| slugify_for_ctc(s, vocab))
        .collect();
    if slugs.iter().all(|s| s.is_empty()) {
        return Ok((
            (Vec::new(), HashSet::new(), HashMap::new()),
            prev_end_frame,
            None,
        ));
    }

    let chapter_baseline: String = slugs.concat();
    let chapter_len = chapter_baseline.len();
    let cv = char_vocab(vocab);
    // ST decodedEmissions WeakMap: one greedy decode for all chapter/chunk searches.
    let owned_document = if cached_document.is_none() {
        decode_positioned_document(emissions, &cv, blank_id)
    } else {
        None
    };
    let document = cached_document.or(owned_document.as_ref());
    // Storyteller: pre-search full chapter for evidence/matched merge into chunks.
    let chapter_bounds = document.and_then(|doc| {
        find_ctc_boundaries_in_document(&chapter_baseline, doc, emissions.frames, excluded_ranges)
    });
    let chapter_evidence = chapter_bounds.as_ref().map(|b| b.evidence_range);
    let chapter_matched = chapter_bounds.as_ref().map(|b| b.matched_range);

    let chunks = chunk_sentences_for_search(&slugs, MAX_SEARCH_LENGTH);
    let chunk_count = chunks.len();
    let mut all_ranges = Vec::new();
    let mut all_dropped = HashSet::new();
    let mut all_word_clips = HashMap::new();
    let mut last_end = prev_end_frame;
    let mut span_lo: Option<usize> = None;
    let mut span_hi: Option<usize> = None;

    for (chunk_index, (first_sentence, chunk_slugs)) in chunks.into_iter().enumerate() {
        let chunk_end = first_sentence + chunk_slugs.len();
        let chunk_sentences = &sentences[first_sentence..chunk_end.min(sentences.len())];
        let (ranges, dropped, word_clips, chunk_last, chunk_span) = align_chunk_ctc(
            chapter_id,
            first_sentence,
            chunk_sentences,
            &chunk_slugs,
            emissions,
            vocab,
            blank_id,
            timings,
            frame_map,
            file_timings,
            last_end,
            excluded_ranges,
            chapter_evidence,
            chapter_matched,
            chapter_len,
            chunk_index,
            chunk_count,
            document,
        )?;
        all_ranges.extend(ranges);
        all_dropped.extend(dropped);
        all_word_clips.extend(word_clips);
        if chunk_last >= 0 {
            last_end = chunk_last;
        }
        if let Some((s, e)) = chunk_span {
            span_lo = Some(span_lo.map_or(s, |lo| lo.min(s)));
            span_hi = Some(span_hi.map_or(e, |hi| hi.max(e)));
        }
    }

    let matched_span = match (span_lo, span_hi) {
        (Some(s), Some(e)) if e > s => Some((s, e)),
        _ => None,
    };
    Ok((
        (all_ranges, all_dropped, all_word_clips),
        last_end,
        matched_span,
    ))
}

type AlignChunkCtcOk = (
    Vec<SentenceRange>,
    DroppedSlots,
    CtcWordClips,
    i32,
    Option<(usize, usize)>,
);

#[allow(clippy::too_many_arguments)]
fn align_chunk_ctc(
    chapter_id: &str,
    sentence_index_offset: usize,
    chunk_sentences: &[String],
    chunk_slugs: &[String],
    emissions: &Emissions,
    vocab: &HashMap<String, i32>,
    blank_id: i32,
    timings: EmissionTimings,
    frame_map: &[(String, usize, usize)],
    file_timings: &[EmissionTimings],
    prev_end_frame: i32,
    excluded_ranges: &[(usize, usize)],
    chapter_evidence: Option<(usize, usize)>,
    chapter_matched: Option<(usize, usize)>,
    chapter_len: usize,
    chunk_index: usize,
    chunk_count: usize,
    document: Option<&PositionedDocument>,
) -> Result<AlignChunkCtcOk, String> {
    let empty_words = || HashMap::new();
    let mut baseline = String::new();
    let mut spans: Vec<(usize, usize)> = Vec::with_capacity(chunk_slugs.len());
    // Char offsets (ST/JS), not UTF-8 bytes — forced-align indexes baseline.chars().
    let mut char_len = 0usize;
    for s in chunk_slugs {
        let start = char_len;
        let n = s.chars().count();
        baseline.push_str(s);
        char_len += n;
        spans.push((start, char_len));
    }
    if baseline.is_empty() {
        return Ok((
            Vec::new(),
            HashSet::new(),
            empty_words(),
            prev_end_frame,
            None,
        ));
    }

    // Storyteller: search miss → skip chunk (no full-book fallback).
    let Some(doc) = document else {
        return Ok((
            Vec::new(),
            HashSet::new(),
            empty_words(),
            prev_end_frame,
            None,
        ));
    };
    let Some(b) =
        find_ctc_boundaries_in_document(&baseline, doc, emissions.frames, excluded_ranges)
    else {
        return Ok((
            Vec::new(),
            HashSet::new(),
            empty_words(),
            prev_end_frame,
            None,
        ));
    };

    let (win_start, win_end) = narrow_to_available_boundary(excluded_ranges, (b.start, b.end));
    // Storyteller: start === end → not found (empty half-open readFrames window).
    if win_start >= win_end {
        return Ok((
            Vec::new(),
            HashSet::new(),
            empty_words(),
            prev_end_frame,
            None,
        ));
    }

    let last_chunk_slug_start = chapter_len.saturating_sub(baseline.len());
    let evidence_range = {
        let head = if chunk_index == 0 {
            chapter_evidence.map_or(b.evidence_range.0, |e| e.0.min(b.evidence_range.0))
        } else {
            b.evidence_range.0
        };
        let tail = if chunk_index + 1 == chunk_count {
            chapter_evidence.map_or(b.evidence_range.1, |e| {
                e.1.saturating_sub(last_chunk_slug_start)
                    .max(b.evidence_range.1)
            })
        } else {
            b.evidence_range.1
        };
        (head, tail)
    };
    let matched_range = {
        let head = if chunk_index == 0 {
            chapter_matched.map_or(b.matched_range.0, |m| m.0.min(b.matched_range.0))
        } else {
            b.matched_range.0
        };
        let tail = if chunk_index + 1 == chunk_count {
            chapter_matched.map_or(b.matched_range.1, |m| {
                m.1.saturating_sub(last_chunk_slug_start)
                    .max(b.matched_range.1)
            })
        } else {
            b.matched_range.1
        };
        (head, tail)
    };

    let anchors: Vec<ForcedAlignAnchor> = b
        .anchors
        .into_iter()
        .filter(|a| {
            a.position > win_start
                && a.position < win_end
                && a.position - win_start >= a.offset
                && win_end - a.position >= baseline.len() - a.offset
        })
        .map(|a| ForcedAlignAnchor {
            offset: a.offset,
            position: a.position - win_start,
        })
        .collect();

    let slice = emissions_slice(emissions, win_start, win_end);

    let mut dropped = HashSet::new();
    let alignments = ctc_forced_align(
        &baseline,
        &slice,
        &anchors,
        vocab,
        blank_id,
        |label_offset, al| {
            let mut ranges = Vec::new();
            for (i, &(s0, s1)) in spans.iter().enumerate() {
                if s1 <= s0 {
                    continue;
                }
                let end_incl = s1 - 1;
                if s0 >= label_offset
                    && end_incl < label_offset + al.len()
                    && detect_dropped_labels(al, s0 - label_offset, end_incl - label_offset, 8)
                {
                    // Storyteller: only mark drop when sentence overlaps evidence range.
                    if end_incl >= evidence_range.0 && s0 < evidence_range.1 {
                        dropped.insert((chapter_id.to_string(), sentence_index_offset + i));
                    }
                    ranges.push((s0, end_incl));
                }
            }
            ranges
        },
    );

    let (b_start, b_end) = boundary_offset_frames(timings);
    let mut ranges = Vec::new();
    let mut word_clips: CtcWordClips = HashMap::new();
    // Storyteller `lastFoundSentenceEnd = 0` per chapter/chunk — window-relative only.
    // Never seed from prior-chapter absolute end (RotK title-page outro false-match
    // left prev at EOF and clamped every later sentence into the last ~230s).
    // Keep this marker in release binaries so we can confirm the image has the fix.
    let _policy = std::hint::black_box("SHELF_CTC_CURSOR_RESET_V1");
    let mut last_end_frame: i32 = -1;
    let mut last_abs_end: i32 = prev_end_frame;
    let mut first_confirmed_start: Option<usize> = None;
    let mut last_confirmed_end: Option<usize> = None;
    let mut any_confirmed = false;

    for (i, &(s0, s1)) in spans.iter().enumerate() {
        let global_i = sentence_index_offset + i;
        if dropped.contains(&(chapter_id.to_string(), global_i)) {
            continue;
        }
        if s1 <= s0 || s1 > alignments.len() {
            continue;
        }
        let end_incl = s1 - 1;
        // Matched-range gate: sentence must overlap the unpadded matched cluster.
        let overlaps_matched = end_incl >= matched_range.0 && s0 < matched_range.1;
        match sentence_audio_range(
            &alignments,
            s0,
            end_incl,
            win_start,
            frame_map,
            file_timings,
            b_start,
            b_end,
            &mut last_end_frame,
            evidence_range,
        ) {
            SentenceAudioOutcome::Matched(clips, abs_start, abs_end, sentence_span) => {
                if overlaps_matched {
                    any_confirmed = true;
                    first_confirmed_start =
                        Some(first_confirmed_start.map_or(abs_start, |s| s.min(abs_start)));
                    last_confirmed_end =
                        Some(last_confirmed_end.map_or(abs_end, |e| e.max(abs_end)));
                }
                ranges.push(
                    SentenceRange::new(global_i, chapter_id, clips).with_state(RangeState::Matched),
                );
                last_abs_end = last_end_frame + win_start as i32;
                // ST: always buildWordRanges from CTC alignments (used when granularity=word).
                if let Some(sentence) = chunk_sentences.get(i) {
                    let mut starts = ctc_word_starts_for_sentence(sentence, vocab);
                    for (a, b) in &mut starts {
                        *a += s0;
                        *b += s0;
                    }
                    let words = build_word_ranges(
                        &alignments,
                        &starts,
                        sentence_span,
                        win_start,
                        b_start,
                        b_end,
                        frame_map,
                        file_timings,
                    );
                    if !words.is_empty() {
                        word_clips.insert((chapter_id.to_string(), global_i), words);
                    }
                }
            }
            SentenceAudioOutcome::Drop => {
                dropped.insert((chapter_id.to_string(), global_i));
            }
            SentenceAudioOutcome::Skip => {}
        }
    }

    // Storyteller getSentenceRanges: reject chapter/chunk when nothing overlaps matchedRange.
    if !any_confirmed {
        return Ok((Vec::new(), dropped, empty_words(), prev_end_frame, None));
    }

    let chunk_span = match (first_confirmed_start, last_confirmed_end) {
        (Some(s), Some(e)) if e > s => Some((s, e)),
        _ => None,
    };
    Ok((ranges, dropped, word_clips, last_abs_end, chunk_span))
}

fn emissions_slice(emissions: &Emissions, from: usize, to: usize) -> Emissions {
    let classes = emissions.classes;
    let from = from.min(emissions.frames);
    let to = to.max(from).min(emissions.frames);
    Emissions {
        values: emissions.values[from * classes..to * classes].to_vec(),
        frames: to - from,
        classes,
    }
}

fn char_vocab(vocab: &HashMap<String, i32>) -> Vec<(char, i32)> {
    vocab
        .iter()
        .filter_map(|(k, &id)| {
            let mut cs = k.chars();
            let c = cs.next()?;
            if cs.next().is_some() {
                return None;
            }
            Some((c, id))
        })
        .collect()
}

/// Storyteller `findDominantRun`.
pub fn find_dominant_run(
    alignments: &[TokenAlignment],
    start_offset: usize,
    end_offset: usize,
    max_gap_frames: i32,
) -> (usize, usize) {
    let mut best = (start_offset, start_offset);
    let mut run_start = start_offset;
    for k in start_offset + 1..=end_offset + 1 {
        let gap = if k <= end_offset {
            alignments[k].start_frame - alignments[k - 1].end_frame
        } else {
            i32::MAX
        };
        if gap > max_gap_frames {
            if k - 1 - run_start > best.1 - best.0 {
                best = (run_start, k - 1);
            }
            run_start = k;
        }
    }
    best
}

/// Storyteller `extendDominantRun`.
pub fn extend_dominant_run(
    alignments: &[TokenAlignment],
    start_offset: usize,
    end_offset: usize,
    run_start: usize,
    run_end: usize,
) -> (i32, i32) {
    let run_start_frame = alignments[run_start].start_frame;
    let run_end_frame = alignments[run_end].end_frame;
    let run_len = (run_end + 1 - run_start) as f64;
    let frames_per_char = (run_end_frame - run_start_frame) as f64 / run_len;

    let mut start_frame = run_start_frame;
    if run_start > start_offset {
        let gap = run_start_frame - alignments[run_start - 1].end_frame;
        let est = ((run_start - start_offset) as f64 * frames_per_char).round() as i32;
        start_frame -= est.min((gap - 1).max(0));
    }

    let mut end_frame = run_end_frame;
    if run_end < end_offset {
        let gap = alignments[run_end + 1].start_frame - run_end_frame;
        let est = ((end_offset - run_end) as f64 * frames_per_char).round() as i32;
        end_frame += est.min((gap - 1).max(0));
    }

    (start_frame, end_frame)
}

/// Storyteller `refineStartFrame`: pad back, clamp past prior end, and floor at
/// the audio-file start that contains `start_frame` (multi-file window floor).
pub(crate) fn refine_start_frame(
    start_frame: i32,
    prev_end_frame: i32,
    pad_frames: i32,
    emissions_start: usize,
    frame_map: &[(String, usize, usize)],
) -> i32 {
    let abs = start_frame.max(0) as usize + emissions_start;
    let window_floor = frame_map
        .iter()
        .find(|(_, fs, fe)| abs >= *fs && abs < *fe)
        .map(|(_, fs, _)| *fs as i32 - emissions_start as i32)
        .unwrap_or(0);
    let mut out = start_frame - pad_frames;
    if prev_end_frame >= 0 {
        out = out.max(prev_end_frame + 1);
    }
    out.max(window_floor).max(0)
}

/// Storyteller `buildSentenceRanges` outcome for one sentence.
enum SentenceAudioOutcome {
    /// Clips + abs frame span + window-relative dominant span for `buildWordRanges`.
    Matched(Vec<AudioRange>, usize, usize, (i32, i32)),
    /// Soft skip → interpolate later (not a hard drop).
    Skip,
    /// ST: weak score inside evidence with collapsed full-span `framesPerChar` → `droppedSentences`.
    Drop,
}

#[allow(clippy::too_many_arguments)]
fn sentence_audio_range(
    alignments: &[TokenAlignment],
    start: usize,
    end_incl: usize,
    frame_offset: usize,
    frame_map: &[(String, usize, usize)],
    file_timings: &[EmissionTimings],
    boundary_start: isize,
    boundary_end: isize,
    last_end_frame: &mut i32,
    evidence_range: (usize, usize),
) -> SentenceAudioOutcome {
    if end_incl >= alignments.len() || start > end_incl {
        return SentenceAudioOutcome::Skip;
    }

    let sentence_length = end_incl + 1 - start;
    // Full-span density (ST) — used only for the weak-score → droppedSentences side-path.
    let full_frames_per_char = (alignments[end_incl].end_frame - alignments[start].start_frame)
        as f64
        / sentence_length as f64;
    let within_evidence = end_incl >= evidence_range.0 && start < evidence_range.1;

    let (run_start, run_end) =
        find_dominant_run(alignments, start, end_incl, MAX_INTRA_SENTENCE_GAP_FRAMES);
    let run_length = run_end + 1 - run_start;

    // Quality gates (Storyteller buildSentenceRanges): skip weak/collapsed → Interpolated later.
    if run_length < sentence_length / 2 {
        return SentenceAudioOutcome::Skip;
    }

    let good_chars = alignments[run_start..=run_end]
        .iter()
        .filter(|a| a.score > -4.0)
        .count();
    if (good_chars as f64) / (run_length as f64) < 0.5 {
        // ST: withinEvidence && sentenceLength > 8 && framesPerChar < 2 → droppedSentences.
        if within_evidence && sentence_length > 8 && full_frames_per_char < 2.0 {
            return SentenceAudioOutcome::Drop;
        }
        return SentenceAudioOutcome::Skip;
    }

    let run_frames_per_char = (alignments[run_end].end_frame - alignments[run_start].start_frame)
        as f64
        / run_length as f64;
    if run_length > 8 && !(2.0..=15.0).contains(&run_frames_per_char) {
        return SentenceAudioOutcome::Skip;
    }

    let (run_start_frame, run_end_frame) =
        extend_dominant_run(alignments, start, end_incl, run_start, run_end);

    let start_frame = refine_start_frame(
        run_start_frame + boundary_start as i32,
        *last_end_frame,
        SENTENCE_PAD_FRAMES,
        frame_offset,
        frame_map,
    );
    let end_frame = (run_end_frame + boundary_end as i32).max(start_frame);
    *last_end_frame = end_frame;

    let abs_start = (start_frame as usize).saturating_add(frame_offset);
    let abs_end = (end_frame as usize)
        .saturating_add(frame_offset)
        .max(abs_start + 1);
    let clips = frames_to_audio_ranges(abs_start, abs_end, frame_map, file_timings);
    SentenceAudioOutcome::Matched(clips, abs_start, abs_end, (start_frame, end_frame))
}

/// Storyteller `buildWordRanges` — window-relative word clips clamped to the
/// sentence dominant-run span. Words stay single-file (first clip only).
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_word_ranges(
    alignments: &[TokenAlignment],
    word_starts: &[(usize, usize)],
    sentence_span: (i32, i32),
    emissions_start: usize,
    boundary_start: isize,
    boundary_end: isize,
    frame_map: &[(String, usize, usize)],
    file_timings: &[EmissionTimings],
) -> Vec<AudioRange> {
    let (span_start, span_end) = sentence_span;
    let mut last_found_word_end = 0i32;
    let mut ranges = Vec::new();
    for &(start_offset, end_offset) in word_starts {
        if end_offset < start_offset || end_offset >= alignments.len() {
            continue;
        }
        let good_chars = alignments[start_offset..=end_offset]
            .iter()
            .filter(|a| a.score > -4.0)
            .count();
        let len = end_offset + 1 - start_offset;
        if (good_chars as f64) / (len as f64) < 0.5 {
            continue;
        }
        let clamp = |frame: i32| frame.clamp(span_start, span_end);
        let start_frame = refine_start_frame(
            clamp(alignments[start_offset].start_frame + boundary_start as i32),
            last_found_word_end,
            0, // ST: padFrames=0 for words
            emissions_start,
            frame_map,
        );
        let end_frame = start_frame.max(clamp(
            alignments[end_offset].end_frame + boundary_end as i32,
        ));
        last_found_word_end = end_frame;
        let abs_start = (start_frame as usize).saturating_add(emissions_start);
        let abs_end = (end_frame as usize)
            .saturating_add(emissions_start)
            .max(abs_start + 1);
        // ST: word ranges stay single-clip — keep the file the word starts in.
        let mut clips = frames_to_audio_ranges(abs_start, abs_end, frame_map, file_timings);
        clips.truncate(1);
        if let Some(clip) = clips.into_iter().next() {
            ranges.push(clip);
        }
    }
    ranges
}

fn timing_at(file_timings: &[EmissionTimings], idx: usize) -> EmissionTimings {
    file_timings.get(idx).copied().unwrap_or(EmissionTimings {
        seconds_per_chunk: 1.0,
        seconds_per_frame: 0.02,
        frames_per_chunk: 50,
        start_offset_seconds: 0.0,
        end_offset_seconds: 0.0,
    })
}

/// Storyteller `framesToAudioRanges` — one clip per crossed audio file.
fn frames_to_audio_ranges(
    abs_start: usize,
    abs_end: usize,
    frame_map: &[(String, usize, usize)],
    file_timings: &[EmissionTimings],
) -> Vec<AudioRange> {
    let global_end = abs_end.max(abs_start);
    let mut ranges = Vec::new();
    for (i, (name, fs, fe)) in frame_map.iter().enumerate() {
        if *fe <= abs_start {
            continue;
        }
        if *fs > global_end {
            break;
        }
        let start = abs_start.max(*fs);
        let end = global_end.min(*fe);
        // Span ending exactly on a file boundary contributes nothing to the next file.
        if end == start && !ranges.is_empty() {
            break;
        }
        if end > start {
            let timings = timing_at(file_timings, i);
            ranges.push(AudioRange {
                audiofile: name.clone(),
                start: frames_to_seconds(start - fs, timings),
                end: frames_to_seconds(end - fs, timings),
            });
        }
    }
    if ranges.is_empty() {
        let fallback = frame_map
            .first()
            .map(|(n, _, _)| n.clone())
            .unwrap_or_else(|| "audio".into());
        let timings = timing_at(file_timings, 0);
        ranges.push(AudioRange {
            audiofile: fallback,
            start: frames_to_seconds(abs_start, timings),
            end: frames_to_seconds(global_end, timings),
        });
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_timings() -> EmissionTimings {
        EmissionTimings {
            seconds_per_chunk: 1.0,
            seconds_per_frame: 0.02,
            frames_per_chunk: 50,
            start_offset_seconds: 0.0,
            end_offset_seconds: 0.0,
        }
    }

    /// Dense emissions that prefer the given label sequence (one frame per label + blanks).
    fn emissions_for_labels(labels: &[i32], classes: usize, blank: i32) -> Emissions {
        let mut values = Vec::new();
        for &lab in labels {
            // blank frame
            let mut row = vec![-5.0f32; classes];
            row[blank as usize] = 2.0;
            values.extend_from_slice(&row);
            // label frame
            let mut row = vec![-5.0f32; classes];
            row[lab as usize] = 2.0;
            values.extend_from_slice(&row);
        }
        // trailing blank
        let mut row = vec![-5.0f32; classes];
        row[blank as usize] = 2.0;
        values.extend_from_slice(&row);
        Emissions {
            frames: values.len() / classes,
            classes,
            values,
        }
    }

    #[test]
    fn synthetic_short_baseline_skips_when_search_misses() {
        // Vocab: blank=0, letters for "hi" / "yo" (tiny fixture).
        let mut vocab = HashMap::new();
        vocab.insert("<blank>".into(), 0);
        vocab.insert("h".into(), 1);
        vocab.insert("i".into(), 2);
        vocab.insert("y".into(), 3);
        vocab.insert("o".into(), 4);
        vocab.insert("|".into(), 5);

        let labels = [1, 2, 5, 3, 4]; // h i | y o
        let emissions = emissions_for_labels(&labels, 6, 0);
        let timings = sample_timings();
        let frame_map = vec![("narration.mp3".into(), 0, emissions.frames)];

        let sentences = vec!["Hi.".into(), "Yo!".into()];
        let file_timings = vec![timings];
        let ((ranges, dropped, _), _, span) = align_sentences_ctc(
            "ch1",
            &sentences,
            &emissions,
            &vocab,
            0,
            timings,
            &frame_map,
            &file_timings,
            -1,
            &[],
            None,
        )
        .expect("align");

        // Storyteller: RANSAC miss → skip chunk (no full-window fallback).
        assert!(dropped.is_empty(), "unexpected drops: {dropped:?}");
        assert!(
            ranges.is_empty(),
            "expected empty on search miss, got {ranges:?}"
        );
        assert!(span.is_none());
    }

    #[test]
    fn narrow_keeps_range_forward_when_region_nested() {
        // ST aligner.test.ts: nested consumption must not yield backwards gap.
        assert_eq!(
            narrow_to_available_boundary(&[(100, 1000), (500, 600)], (990, 1200)),
            (1001, 1200)
        );
    }

    #[test]
    fn narrow_handles_aligner_report_order() {
        assert_eq!(
            narrow_to_available_boundary(
                &[(912859, 1080856), (995296, 996906)],
                (1080725, 1177168),
            ),
            (1080857, 1177168)
        );
    }

    #[test]
    fn narrow_to_largest_disjoint_gap() {
        assert_eq!(
            narrow_to_available_boundary(&[(100, 200), (300, 400)], (150, 350)),
            (201, 299)
        );
    }

    #[test]
    fn narrow_unchanged_when_no_frame_available() {
        // Caller treats start === end as "chapter not found".
        assert_eq!(
            narrow_to_available_boundary(&[(100, 200)], (200, 200)),
            (200, 200)
        );
    }

    #[test]
    fn audio_exclusion_masks_map_track_seconds_to_frames() {
        let timings = sample_timings();
        let frame_map = vec![("a.mp3".into(), 0, 100), ("b.mp3".into(), 100, 200)];
        let file_timings = vec![timings, timings];
        let masks = audio_exclusion_frame_masks(
            &[AudioExclusion {
                track: "b.mp3".into(),
                start: Some(0.2),
                end: Some(0.6),
            }],
            &frame_map,
            &file_timings,
        );
        // 0.2s / 0.02 = 10 frames; 0.6s = 30 → absolute [110, 130)
        assert_eq!(masks, vec![(110, 130)]);
    }

    #[test]
    fn emission_masks_blank_argmax_leaves_unmasked() {
        let classes = 4usize;
        let blank = 0i32;
        let mut values = Vec::new();
        for _ in 0..5 {
            let mut row = vec![-1.0f32; classes];
            row[2] = 5.0;
            values.extend_from_slice(&row);
        }
        let mut em = Emissions {
            values,
            frames: 5,
            classes,
        };
        apply_emission_masks(&mut em, &[(1, 3)], blank);
        for frame in [1usize, 2] {
            let off = frame * classes;
            assert_eq!(crate::align::ctc::argmax(&em.values, off, classes), 0);
            assert_eq!(em.values[off + blank as usize], 0.0);
            assert_eq!(em.values[off + 2], MASKED_LOG_PROB);
        }
        assert_eq!(crate::align::ctc::argmax(&em.values, 0, classes), 2);
        assert_eq!(
            crate::align::ctc::argmax(&em.values, 3 * classes, classes),
            2
        );
        assert!(
            (package_audio_duration_secs(&[("a.mp3".into(), 0, 100)], &[sample_timings()]) - 2.0)
                .abs()
                < 1e-9
        );
        assert!(check_audio_emissions_pairing(
            &["a.mp3".into(), "b.mp3".into()],
            &["a.stem".into(), "b.stem".into()],
        )
        .is_ok());
        let err =
            check_audio_emissions_pairing(&["a.mp3".into(), "b.mp3".into()], &["a.stem".into()])
                .unwrap_err();
        assert!(err.contains("Missing emissions for"), "{err}");
        assert!(err.contains("b.mp3"), "{err}");
    }

    #[test]
    fn decoded_emissions_cached_until_clear() {
        // ST decodedEmissions WeakMap: greedy decode once; withMasks → clear.
        use crate::align::stem::{StemEncoding, StemHeader, TOKENIZER_NONE};

        let mut vocab = HashMap::new();
        vocab.insert("<blank>".into(), 0);
        vocab.insert("h".into(), 1);
        vocab.insert("i".into(), 2);
        vocab.insert("|".into(), 3);
        let emissions = emissions_for_labels(&[1, 2], 4, 0);
        let frames = emissions.frames;
        let header = StemHeader {
            version: 3,
            vocab,
            blank_index: 0,
            classes: 4,
            num_frames: frames,
            timings: sample_timings(),
            encoding: StemEncoding::Dense,
            tokenizer_kind: TOKENIZER_NONE,
            top_k: 0,
            frames_offset: 0,
            bytes_per_frame: 8,
        };
        let mut book = CtcBookEmissions {
            header: header.clone(),
            emissions,
            frame_map: vec![("a.mp3".into(), 0, frames)],
            file_timings: vec![header.timings],
            decoded: OnceLock::new(),
        };
        assert!(book.decoded.get().is_none());
        let first = book.positioned_document().expect("decode");
        assert!(!first.text.is_empty());
        let cached_text = first.text.clone();
        assert!(book.decoded.get().is_some());
        let ptr = book.decoded.get().unwrap() as *const _;
        let second = book.positioned_document().expect("cached");
        assert_eq!(second as *const _, ptr);
        book.clear_decoded();
        assert!(book.decoded.get().is_none());
        let third = book.positioned_document().expect("redecode");
        assert_eq!(third.text, cached_text);
        assert!(book.decoded.get().is_some());
    }

    #[test]
    fn slugify_for_ctc_concatenates_words_like_st_empty_separator() {
        // ST Aligner: slugify(..., separator=""). `|` is in MMS vocab but must
        // not appear in the search query — greedy decode strips it.
        let mut vocab = HashMap::new();
        for (c, id) in [("h", 1), ("i", 2), ("t", 3), ("e", 4), ("r", 5), ("|", 6)] {
            vocab.insert(c.into(), id);
        }
        assert_eq!(slugify_for_ctc("Hi there", &vocab), "hithere");
        assert!(!slugify_for_ctc("Hi there", &vocab).contains('|'));
    }

    #[test]
    fn slugify_for_ctc_ngrams_overlap_pipeless_decode_document() {
        // Regression: pipe separators left ~0 n-gram overlap with decode text,
        // so RANSAC false-matched RotK near EOF and SMIL collapsed.
        let mut vocab = HashMap::new();
        for (i, c) in ("abcdefghijklmnopqrstuvwxyz|").chars().enumerate() {
            vocab.insert(c.to_string(), i as i32 + 1);
        }
        let query = slugify_for_ctc("Pippin looked out from the shelter", &vocab);
        let document = "pippinlookedoutfromtheshelter"; // greedy decode shape
        assert_eq!(query, document);
        const N: usize = 10;
        let q_grams: std::collections::HashSet<_> =
            (0..=query.len() - N).map(|i| &query[i..i + N]).collect();
        let d_grams: std::collections::HashSet<_> = (0..=document.len() - N)
            .map(|i| &document[i..i + N])
            .collect();
        assert_eq!(q_grams.len(), d_grams.len());
        assert_eq!(q_grams.intersection(&d_grams).count(), q_grams.len());
    }

    #[test]
    fn slugify_for_ctc_opencc_folds_traditional_to_simplified() {
        // 書 → 书; vocab only has simplified so CTC baseline must emit 书.
        let mut vocab = HashMap::new();
        vocab.insert("书".into(), 1);
        vocab.insert("|".into(), 2);
        assert_eq!(slugify_for_ctc("書", &vocab), "书");
        assert_eq!(slugify_for_ctc_with_locale("書", &vocab, "zh"), "书");
    }

    #[test]
    fn dominant_run_skips_sparse_alignment() {
        // Ten chars: first half clustered, second half deferred across a huge gap.
        let mut alignments = Vec::new();
        for i in 0..5 {
            alignments.push(TokenAlignment {
                label: 1,
                start_frame: i * 3,
                end_frame: i * 3 + 2,
                score: -1.0,
            });
        }
        for i in 0..5 {
            alignments.push(TokenAlignment {
                label: 1,
                start_frame: 5000 + i * 3,
                end_frame: 5000 + i * 3 + 2,
                score: -1.0,
            });
        }
        let (run_start, run_end) = find_dominant_run(
            &alignments,
            0,
            alignments.len() - 1,
            MAX_INTRA_SENTENCE_GAP_FRAMES,
        );
        let run_length = run_end + 1 - run_start;
        let sentence_length = alignments.len();
        assert!(run_length < sentence_length / 2 || run_length == 5);

        // sentence_audio_range should refuse: run is only half (borderline) — make
        // the second cluster smaller so dominant run < half.
        let mut sparse = alignments[..5].to_vec();
        sparse.push(TokenAlignment {
            label: 1,
            start_frame: 8000,
            end_frame: 8001,
            score: -1.0,
        });
        sparse.push(TokenAlignment {
            label: 1,
            start_frame: 8200,
            end_frame: 8201,
            score: -1.0,
        });
        sparse.push(TokenAlignment {
            label: 1,
            start_frame: 8400,
            end_frame: 8401,
            score: -1.0,
        });
        // 5 + 3 = 8 chars; dominant run length 5 < 8/2? 5 >= 4, still passes.
        // Add more deferred singles so run 5 < half of 11.
        for i in 0..3 {
            sparse.push(TokenAlignment {
                label: 1,
                start_frame: 9000 + i * 200,
                end_frame: 9000 + i * 200 + 1,
                score: -1.0,
            });
        }
        // Now 11 chars, dominant run of 5 < 11/2=5? 5 < 5 is false. Need 5 < 5.5 → length 12.
        sparse.push(TokenAlignment {
            label: 1,
            start_frame: 10000,
            end_frame: 10001,
            score: -1.0,
        });
        let (rs, re) =
            find_dominant_run(&sparse, 0, sparse.len() - 1, MAX_INTRA_SENTENCE_GAP_FRAMES);
        let rl = re + 1 - rs;
        assert!(rl < sparse.len() / 2, "run={rl} len={}", sparse.len());

        let timings = sample_timings();
        let frame_map = vec![("a.mp3".into(), 0, 20000)];
        let mut last = -1i32;
        let out = sentence_audio_range(
            &sparse,
            0,
            sparse.len() - 1,
            0,
            &frame_map,
            &[timings],
            0,
            0,
            &mut last,
            (0, sparse.len()),
        );
        assert!(
            matches!(out, SentenceAudioOutcome::Skip),
            "sparse dominant-run must skip Matched emission"
        );
    }

    #[test]
    fn weak_score_collapsed_frames_inside_evidence_marks_drop() {
        // 10+ chars, dominant run covers all, but every score is terrible and
        // full-span framesPerChar < 2 → ST droppedSentences side-path.
        let n = 12usize;
        let alignments: Vec<TokenAlignment> = (0..n)
            .map(|i| TokenAlignment {
                label: 1,
                start_frame: i as i32, // 1 frame per char → fpc = 1
                end_frame: i as i32 + 1,
                score: -10.0,
            })
            .collect();
        let timings = sample_timings();
        let frame_map = vec![("a.mp3".into(), 0, 100)];
        let mut last = -1i32;
        let out = sentence_audio_range(
            &alignments,
            0,
            n - 1,
            0,
            &frame_map,
            &[timings],
            0,
            0,
            &mut last,
            (0, n),
        );
        assert!(
            matches!(out, SentenceAudioOutcome::Drop),
            "expected Drop for weak collapsed evidence sentence"
        );

        // Outside evidence → Skip, not Drop.
        let mut last = -1i32;
        let out = sentence_audio_range(
            &alignments,
            0,
            n - 1,
            0,
            &frame_map,
            &[timings],
            0,
            0,
            &mut last,
            (n + 10, n + 20),
        );
        assert!(matches!(out, SentenceAudioOutcome::Skip));
    }

    #[test]
    fn chunk_sentences_for_search_splits_when_over_max() {
        let slugs = vec!["aaaa".into(), "bbbb".into(), "cccc".into(), "dddd".into()];
        let chunks = chunk_sentences_for_search(&slugs, 8);
        assert!(
            chunks.len() >= 2,
            "expected multiple chunks, got {chunks:?}"
        );
        let rejoined: Vec<String> = chunks.iter().flat_map(|(_, s)| s.clone()).collect();
        assert_eq!(rejoined, slugs);
        assert_eq!(chunks[0].0, 0);
        assert!(chunks[1].0 > 0);
    }

    #[test]
    fn refine_start_frame_respects_multi_file_floor_and_prev_end() {
        // Two files: [0,100) and [100,200). Window starts at 50.
        let frame_map = vec![("a.mp3".into(), 0, 100), ("b.mp3".into(), 100, 200)];
        let emissions_start = 50usize;
        // start_frame 60 → abs 110 → file b → window_floor = 100-50 = 50
        let floored = refine_start_frame(60, -1, 25, emissions_start, &frame_map);
        assert_eq!(floored, 50, "must not pad into previous audio file");
        // Prior sentence ended at window-relative 55 → clamp to 56 beats pad.
        let after_prev = refine_start_frame(60, 55, 25, emissions_start, &frame_map);
        assert_eq!(after_prev, 56);
    }

    #[test]
    fn ctc_word_starts_concat_matches_sentence_slug_len() {
        let mut vocab = HashMap::new();
        for c in "abcdefghijklmnopqrstuvwxyz".chars() {
            vocab.insert(c.to_string(), 1);
        }
        let sentence = "Hello, world!";
        let starts = ctc_word_starts_for_sentence(sentence, &vocab);
        assert_eq!(starts.len(), 2);
        let total = starts.last().map(|(_, e)| e + 1).unwrap_or(0);
        assert_eq!(total, slugify_for_ctc(sentence, &vocab).chars().count());
    }

    #[test]
    fn build_word_ranges_clamps_to_sentence_span() {
        let timings = sample_timings();
        let frame_map = vec![("a.mp3".into(), 0, 500)];
        // 6 chars @ frames 10..16; two words [0,2] and [3,5].
        let alignments: Vec<TokenAlignment> = (0..6)
            .map(|i| TokenAlignment {
                label: 1,
                start_frame: 10 + i,
                end_frame: 11 + i,
                score: -1.0,
            })
            .collect();
        let words = vec![(0usize, 2usize), (3, 5)];
        let clips = build_word_ranges(
            &alignments,
            &words,
            (10, 16),
            0,
            0,
            0,
            &frame_map,
            &[timings],
        );
        assert_eq!(clips.len(), 2);
        assert!(clips[0].start < clips[0].end);
        assert!(clips[1].start >= clips[0].end - 1e-9 || clips[1].start >= clips[0].start);
    }

    #[test]
    fn frames_to_audio_ranges_splits_across_files() {
        // ST framesToAudioRanges: span 80..150 over [0,100)+[100,200) → two clips.
        let timings = sample_timings();
        let frame_map = vec![("a.mp3".into(), 0, 100), ("b.mp3".into(), 100, 200)];
        let file_timings = vec![timings, timings];
        let clips = frames_to_audio_ranges(80, 150, &frame_map, &file_timings);
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].audiofile, "a.mp3");
        assert!((clips[0].start - frames_to_seconds(80, timings)).abs() < 1e-9);
        assert!((clips[0].end - frames_to_seconds(100, timings)).abs() < 1e-9);
        assert_eq!(clips[1].audiofile, "b.mp3");
        assert!((clips[1].start - frames_to_seconds(0, timings)).abs() < 1e-9);
        assert!((clips[1].end - frames_to_seconds(50, timings)).abs() < 1e-9);

        // Exact file boundary end contributes nothing to the next file.
        let at_boundary = frames_to_audio_ranges(50, 100, &frame_map, &file_timings);
        assert_eq!(at_boundary.len(), 1);
        assert_eq!(at_boundary[0].audiofile, "a.mp3");

        // Different seconds_per_chunk still changes chunk math when frames cross chunks.
        let mut slow_b = timings;
        slow_b.seconds_per_chunk = 2.0;
        slow_b.frames_per_chunk = 50; // same spf, different chunk seconds
        let mixed = vec![timings, slow_b];
        let clips_mixed = frames_to_audio_ranges(80, 150, &frame_map, &mixed);
        assert_eq!(clips_mixed.len(), 2);
        // File b: 50 frames with 2.0s/chunk @ 50 fpc → exactly 2.0s (vs 1.0s with sample).
        assert!(
            (clips_mixed[1].end - 2.0).abs() < 1e-9,
            "got {}",
            clips_mixed[1].end
        );
    }

    /// Full STEM + real EPUB markup for title→…→Minas (pipeline mirror).
    #[test]
    fn rotk_full_epub_spine_through_minas_stays_early() {
        use crate::align::markup::markup_xhtml_body;
        use crate::align::stem::decode_stem_file;
        use std::io::Read;

        let stem = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../data/align-work/probe/rotk_full.stem");
        let epub = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../data/files/971031c2c30578376fca9b1786934c9686530d1ccf7f18cbf49bc3faae4d2697.epub",
        );
        if !stem.exists() || !epub.exists() {
            eprintln!("skip: missing rotk stem/epub");
            return;
        }
        let (header, emissions) = decode_stem_file(&stem).expect("stem");
        let frames = emissions.frames;
        let book = CtcBookEmissions {
            header: header.clone(),
            emissions,
            frame_map: vec![("narration.mp3".into(), 0, frames)],
            file_timings: vec![header.timings],
            decoded: OnceLock::new(),
        };
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&epub).unwrap()).unwrap();
        let mut prev_end: i32 = -1;
        let mut matched_boundaries: Vec<(usize, usize)> = Vec::new();
        for name in [
            "index_split_001.html",
            "index_split_002.html",
            "index_split_004.html",
            "index_split_006.html",
        ] {
            let mut raw = String::new();
            zip.by_name(name).unwrap().read_to_string(&mut raw).unwrap();
            let (_marked, sentences) = markup_xhtml_body(&raw, Some("ch1")).expect("markup");
            let word_count: usize = sentences.iter().map(|s| s.split_whitespace().count()).sum();
            if sentences.len() < 2 && word_count < 4 {
                eprintln!("skip short {name}");
                continue;
            }
            let ((ranges, _, _), chapter_end, span) =
                align_chapter(name, &sentences, &book, prev_end, &matched_boundaries)
                    .expect("align");
            if chapter_end >= 0 {
                prev_end = chapter_end;
            }
            if let Some(s) = span {
                matched_boundaries.push(s);
            }
            let earliest = ranges
                .iter()
                .filter_map(|r| r.audio_ranges.first().map(|a| a.start))
                .fold(f64::INFINITY, f64::min);
            eprintln!(
                "{name} sents={} ranges={} end={chapter_end} span={span:?} earliest={earliest:.1} boundaries={matched_boundaries:?}",
                sentences.len(),
                ranges.len(),
            );
            if name.contains("006") {
                assert!(
                    earliest < 1000.0,
                    "spine path EOF-latched Minas: earliest={earliest}"
                );
            }
        }
    }

    /// Full STEM: after title-page outro match, Minas must still align early.
    #[test]
    fn rotk_full_title_then_minas_aligns_early() {
        use crate::align::stem::decode_stem_file;

        let stem = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../data/align-work/probe/rotk_full.stem");
        let title_path = std::path::PathBuf::from(
            "/home/cayub/.hermes/cache/scratch/rotk-probe/index_split_001.html.txt",
        );
        let minas_path = std::path::PathBuf::from(
            "/home/cayub/.hermes/cache/scratch/rotk-probe/index_split_006.html.txt",
        );
        if !stem.exists() || !title_path.exists() || !minas_path.exists() {
            eprintln!("skip: missing rotk full fixtures");
            return;
        }
        let (header, emissions) = decode_stem_file(&stem).expect("stem");
        let frames = emissions.frames;
        let book = CtcBookEmissions {
            header: header.clone(),
            emissions,
            frame_map: vec![("narration.mp3".into(), 0, frames)],
            file_timings: vec![header.timings],
            decoded: OnceLock::new(),
        };
        let title: Vec<String> = std::fs::read_to_string(&title_path)
            .unwrap()
            .lines()
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty())
            .collect();
        let minas: Vec<String> = std::fs::read_to_string(&minas_path)
            .unwrap()
            .lines()
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty())
            .collect();
        let ((title_ranges, _, _), title_end, title_span) =
            align_chapter("title", &title, &book, -1, &[]).expect("title align");
        eprintln!(
            "title ranges={} end={title_end} span={title_span:?} first={:?}",
            title_ranges.len(),
            title_ranges
                .first()
                .map(|r| (r.audio_ranges[0].start, r.audio_ranges[0].end))
        );
        let mut excluded = Vec::new();
        if let Some(span) = title_span {
            excluded.push(span);
        }
        // Poison prev_end the way the live pipeline used to (and still passes).
        let ((ranges, _, _), _end, span) =
            align_chapter("minas", &minas, &book, title_end, &excluded).expect("minas");
        assert!(!ranges.is_empty(), "minas produced no ranges");
        let start = ranges
            .iter()
            .filter_map(|r| r.audio_ranges.first().map(|a| a.start))
            .fold(f64::INFINITY, f64::min);
        eprintln!(
            "minas ranges={} span={span:?} earliest_start={start:.1}",
            ranges.len()
        );
        assert!(
            start < 1000.0,
            "minas still EOF-latched after title exclude: earliest={start}"
        );
    }

    /// RotK: title-page false-matched the outro (~EOF). Seeding the next chapter's
    /// window-relative cursor from that absolute end clamped every sentence to EOF.
    #[test]
    fn prior_chapter_eof_prev_end_does_not_seed_window_cursor() {
        use crate::align::stem::decode_stem_file;

        let stem = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../data/align-work/probe/head90.stem");
        if !stem.exists() {
            eprintln!("skip: missing {}", stem.display());
            return;
        }
        let (header, emissions) = decode_stem_file(&stem).expect("stem");
        let sentence = "Pippin looked out from the shelter of Gandalf's cloak. He wondered if he was awake or still sleeping, still in the swift-moving dream in which he had been wrapped so long since the great ride began.";
        let file_timings = vec![header.timings];
        let ((ranges, _, _), last_end, span) = align_sentences_ctc(
            "ch",
            &[sentence.to_string()],
            &emissions,
            &header.vocab,
            header.blank_index,
            header.timings,
            &[("narration.mp3".into(), 0, emissions.frames)],
            &file_timings,
            // Poison: prior chapter "ended" past this STEM (outro false-match).
            (emissions.frames as i32) + 1000,
            &[],
            None,
        )
        .expect("align");
        assert!(!ranges.is_empty(), "expected ranges despite EOF prev_end");
        let start = ranges[0].audio_ranges[0].start;
        let end = ranges[0].audio_ranges[0].end;
        assert!(start < 80.0, "clamped to EOF-ish start={start}");
        assert!(end > 30.0, "expected real Minas window end={end}");
        assert!(last_end >= 0);
        assert!(span.is_some());
    }
}
