//! Storyteller Align Report v4 (`report-v4.ts` + lite `reportBuilder`).

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::align::holes::{Hole, HolePosition};
use crate::align::ranges::{RangeState, SentenceRange};
use crate::align::readaloud::MarkedChapter;

pub const REPORT_VERSION: u32 = 4;

pub const ALIGN_REPORT_BASENAME: &str = "storyteller-align-report.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportAligner {
    Ctc,
    Classic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpineStatus {
    Aligned,
    NotFound,
    TooShort,
    Excluded,
    AudioOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SpanState {
    Aligned,
    Interpolated,
    Unmatched,
    Dropped,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpanAudio {
    pub track: i32,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSpan {
    pub spine: usize,
    pub first_sentence: usize,
    pub last_sentence: usize,
    pub state: SpanState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<SpanAudio>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportHole {
    pub spine: usize,
    pub sentence: usize,
    pub position: ReportHolePosition,
    pub audio: SpanAudio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportHolePosition {
    Before,
    After,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportClassification {
    pub zone: String,
    pub verdict: String,
    pub decided_by: Option<serde_json::Value>,
    pub signals: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SentenceContext {
    pub prev_sentence: Option<String>,
    pub matched_sentence: String,
    pub next_sentence: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSpineItem {
    pub index: usize,
    pub href: String,
    pub title: Option<String>,
    pub status: SpineStatus,
    pub classification: Option<ReportClassification>,
    pub decision: Option<String>,
    pub sentence_count: usize,
    pub matched_sentence_count: usize,
    pub interpolated_sentence_count: usize,
    pub dropped_sentence_count: usize,
    pub matched_seconds: f64,
    pub interpolated_seconds: f64,
    pub loose_seconds: f64,
    pub score: Option<f64>,
    pub first_match: Option<SentenceContext>,
    pub last_match: Option<SentenceContext>,
    pub preview: Option<String>,
}

/// Spine row before assemble fills counts/seconds/score.
#[derive(Debug, Clone)]
pub struct SpineItemInput {
    pub index: usize,
    pub href: String,
    pub title: Option<String>,
    pub status: SpineStatus,
    pub classification: Option<ReportClassification>,
    pub decision: Option<String>,
    pub sentence_count: usize,
    pub first_match: Option<SentenceContext>,
    pub last_match: Option<SentenceContext>,
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTrackSource {
    pub file: String,
    pub start: f64,
    pub end: f64,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcludedRange {
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTrack {
    pub index: usize,
    pub name: String,
    pub duration: f64,
    pub offset: f64,
    pub matched_seconds: f64,
    pub interpolated_seconds: f64,
    pub loose_seconds: f64,
    pub excluded: Vec<ExcludedRange>,
    pub source: Option<ReportTrackSource>,
}

#[derive(Debug, Clone)]
pub struct TrackInput {
    pub index: usize,
    pub name: String,
    pub duration: f64,
    pub offset: f64,
    pub source: Option<ReportTrackSource>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpineStatusCounts {
    pub aligned: usize,
    #[serde(rename = "not-found")]
    pub not_found: usize,
    #[serde(rename = "too-short")]
    pub too_short: usize,
    pub excluded: usize,
    #[serde(rename = "audio-only")]
    pub audio_only: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTotals {
    pub sentences: usize,
    pub matched_sentences: usize,
    pub interpolated_sentences: usize,
    pub unmatched_sentences: usize,
    pub dropped_sentences: usize,
    pub seconds: f64,
    pub matched_seconds: f64,
    pub interpolated_seconds: f64,
    pub loose_seconds: f64,
    pub spine: SpineStatusCounts,
    pub tracks: usize,
    pub unaligned_tracks: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportV4 {
    pub version: u32,
    pub aligner: ReportAligner,
    pub auto_exclude: bool,
    pub spine: Vec<ReportSpineItem>,
    pub tracks: Vec<ReportTrack>,
    pub spans: Vec<ReportSpan>,
    pub holes: Vec<ReportHole>,
    pub totals: ReportTotals,
}

pub fn empty_totals() -> ReportTotals {
    ReportTotals {
        sentences: 0,
        matched_sentences: 0,
        interpolated_sentences: 0,
        unmatched_sentences: 0,
        dropped_sentences: 0,
        seconds: 0.0,
        matched_seconds: 0.0,
        interpolated_seconds: 0.0,
        loose_seconds: 0.0,
        spine: SpineStatusCounts {
            aligned: 0,
            not_found: 0,
            too_short: 0,
            excluded: 0,
            audio_only: 0,
        },
        tracks: 0,
        unaligned_tracks: 0,
    }
}

pub fn empty_report(aligner: ReportAligner, auto_exclude: bool) -> ReportV4 {
    ReportV4 {
        version: REPORT_VERSION,
        aligner,
        auto_exclude,
        spine: Vec::new(),
        tracks: Vec::new(),
        spans: Vec::new(),
        holes: Vec::new(),
        totals: empty_totals(),
    }
}

pub fn round_seconds(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn round_score(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn span_state(state: RangeState) -> SpanState {
    match state {
        RangeState::Matched => SpanState::Aligned,
        RangeState::Interpolated => SpanState::Interpolated,
        RangeState::Unmatched => SpanState::Unmatched,
        RangeState::Dropped => SpanState::Dropped,
    }
}

fn span_sentences(first: usize, last: usize) -> usize {
    last.saturating_sub(first).saturating_add(1)
}

fn span_seconds(audio: Option<&SpanAudio>) -> f64 {
    audio.map(|a| a.end - a.start).unwrap_or(0.0)
}

/// ST `buildChapterSpans` — run consecutive same-state sentences into spans.
pub fn build_chapter_spans(
    spine: usize,
    sentence_count: usize,
    ranges: &[SentenceRange],
    track_index: &dyn Fn(&str) -> i32,
    sentence_score: &dyn Fn(usize) -> Option<f64>,
) -> Vec<ReportSpan> {
    let by_id: HashMap<usize, &SentenceRange> = ranges.iter().map(|r| (r.id, r)).collect();

    struct SentenceState {
        state: SpanState,
        range: Option<SentenceRange>,
    }

    let mut states: Vec<SentenceState> = Vec::with_capacity(sentence_count);
    for id in 0..sentence_count {
        let range = by_id.get(&id).copied();
        let state = range
            .map(|r| span_state(r.state))
            .unwrap_or(SpanState::Unmatched);
        let timed = matches!(state, SpanState::Aligned | SpanState::Interpolated);
        states.push(SentenceState {
            state,
            range: if timed { range.cloned() } else { None },
        });
    }

    let mut spans = Vec::new();
    let mut run_start = 0usize;
    while run_start < states.len() {
        let head_state = states[run_start].state;
        let head_range = states[run_start].range.clone();
        let track = head_range
            .as_ref()
            .map(|r| track_index(&r.first().audiofile))
            .unwrap_or(-1);
        let mut run_end = run_start;
        while run_end + 1 < states.len() {
            let next = &states[run_end + 1];
            if next.state != head_state {
                break;
            }
            if let Some(ref nr) = next.range {
                if track_index(&nr.first().audiofile) != track {
                    break;
                }
            }
            run_end += 1;
        }

        if let Some(head) = head_range.as_ref() {
            let mut start = head.first().start;
            let mut end = head.first().end;
            let mut score_sum = 0.0;
            let mut score_count = 0usize;
            for (id, state) in states.iter().enumerate().take(run_end + 1).skip(run_start) {
                let range = state.range.as_ref().unwrap();
                for clip in &range.audio_ranges {
                    if track_index(&clip.audiofile) != track {
                        continue;
                    }
                    start = start.min(clip.start);
                    end = end.max(clip.end);
                }
                if let Some(score) = sentence_score(id) {
                    score_sum += score;
                    score_count += 1;
                }
            }
            let audio = SpanAudio {
                track,
                start: round_seconds(start),
                end: round_seconds(end),
            };
            if head_state == SpanState::Aligned {
                spans.push(ReportSpan {
                    spine,
                    first_sentence: run_start,
                    last_sentence: run_end,
                    state: SpanState::Aligned,
                    audio: Some(audio),
                    score: if score_count > 0 {
                        Some(round_score(score_sum / score_count as f64))
                    } else {
                        None
                    },
                });
            } else {
                spans.push(ReportSpan {
                    spine,
                    first_sentence: run_start,
                    last_sentence: run_end,
                    state: SpanState::Interpolated,
                    audio: Some(audio),
                    score: None,
                });
            }
        } else {
            spans.push(ReportSpan {
                spine,
                first_sentence: run_start,
                last_sentence: run_end,
                state: if head_state == SpanState::Dropped {
                    SpanState::Dropped
                } else {
                    SpanState::Unmatched
                },
                audio: None,
                score: None,
            });
        }
        run_start = run_end + 1;
    }
    spans
}

pub struct AssembleReportInput {
    pub aligner: ReportAligner,
    pub auto_exclude: bool,
    pub spine: Vec<SpineItemInput>,
    pub tracks: Vec<TrackInput>,
    pub spans: Vec<ReportSpan>,
    pub holes: Vec<ReportHole>,
    pub excluded_audio: HashMap<usize, Vec<ExcludedRange>>,
}

/// ST `assembleReport`.
pub fn assemble_report(input: AssembleReportInput) -> ReportV4 {
    let mut spine: Vec<ReportSpineItem> = input
        .spine
        .into_iter()
        .map(|item| ReportSpineItem {
            index: item.index,
            href: item.href,
            title: item.title,
            status: item.status,
            classification: item.classification,
            decision: item.decision,
            sentence_count: item.sentence_count,
            matched_sentence_count: 0,
            interpolated_sentence_count: 0,
            dropped_sentence_count: 0,
            matched_seconds: 0.0,
            interpolated_seconds: 0.0,
            loose_seconds: 0.0,
            score: None,
            first_match: item.first_match,
            last_match: item.last_match,
            preview: item.preview,
        })
        .collect();
    spine.sort_by_key(|s| s.index);

    let mut tracks: Vec<ReportTrack> = input
        .tracks
        .into_iter()
        .map(|t| ReportTrack {
            index: t.index,
            name: t.name,
            duration: t.duration,
            offset: t.offset,
            matched_seconds: 0.0,
            interpolated_seconds: 0.0,
            loose_seconds: 0.0,
            excluded: input
                .excluded_audio
                .get(&t.index)
                .cloned()
                .unwrap_or_default(),
            source: t.source,
        })
        .collect();

    let mut score_sums: HashMap<usize, (f64, usize)> = HashMap::new();
    let mut spans = input.spans;
    spans.sort_by(|a, b| {
        a.spine
            .cmp(&b.spine)
            .then(a.first_sentence.cmp(&b.first_sentence))
    });

    for span in &spans {
        let sentences = span_sentences(span.first_sentence, span.last_sentence);
        let seconds = span_seconds(span.audio.as_ref());
        let item = spine.iter_mut().find(|s| s.index == span.spine);
        let track = span
            .audio
            .as_ref()
            .and_then(|a| tracks.get_mut(a.track as usize));
        match span.state {
            SpanState::Aligned => {
                if let Some(item) = item {
                    item.matched_sentence_count += sentences;
                    item.matched_seconds += seconds;
                    if let Some(score) = span.score {
                        let entry = score_sums.entry(span.spine).or_insert((0.0, 0));
                        entry.0 += score * sentences as f64;
                        entry.1 += sentences;
                    }
                }
                if let Some(track) = track {
                    track.matched_seconds += seconds;
                }
            }
            SpanState::Interpolated => {
                if let Some(item) = item {
                    item.interpolated_sentence_count += sentences;
                    item.interpolated_seconds += seconds;
                }
                if let Some(track) = track {
                    track.interpolated_seconds += seconds;
                }
            }
            SpanState::Dropped => {
                if let Some(item) = item {
                    item.dropped_sentence_count += sentences;
                }
            }
            SpanState::Unmatched => {}
        }
    }

    let mut holes = input.holes;
    holes.sort_by(|a, b| {
        a.audio.track.cmp(&b.audio.track).then(
            a.audio
                .start
                .partial_cmp(&b.audio.start)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    for hole in &holes {
        let seconds = span_seconds(Some(&hole.audio));
        if let Some(item) = spine.iter_mut().find(|s| s.index == hole.spine) {
            item.loose_seconds += seconds;
        }
        if let Some(track) = tracks.get_mut(hole.audio.track as usize) {
            track.loose_seconds += seconds;
        }
    }

    let mut totals = empty_totals();
    for item in &mut spine {
        if let Some((sum, count)) = score_sums.get(&item.index) {
            if *count > 0 {
                item.score = Some(round_score(sum / *count as f64));
            }
        }
        item.matched_seconds = round_seconds(item.matched_seconds);
        item.interpolated_seconds = round_seconds(item.interpolated_seconds);
        item.loose_seconds = round_seconds(item.loose_seconds);
        if item.status == SpineStatus::Aligned
            && item.matched_sentence_count == 0
            && item.sentence_count > 0
        {
            item.status = SpineStatus::NotFound;
        }
        match item.status {
            SpineStatus::Aligned => totals.spine.aligned += 1,
            SpineStatus::NotFound => totals.spine.not_found += 1,
            SpineStatus::TooShort => totals.spine.too_short += 1,
            SpineStatus::Excluded => totals.spine.excluded += 1,
            SpineStatus::AudioOnly => totals.spine.audio_only += 1,
        }
        totals.sentences += item.sentence_count;
        totals.matched_sentences += item.matched_sentence_count;
        totals.interpolated_sentences += item.interpolated_sentence_count;
        totals.dropped_sentences += item.dropped_sentence_count;
    }
    totals.unmatched_sentences = totals
        .sentences
        .saturating_sub(totals.matched_sentences)
        .saturating_sub(totals.interpolated_sentences)
        .saturating_sub(totals.dropped_sentences);

    for track in &mut tracks {
        track.matched_seconds = round_seconds(track.matched_seconds);
        track.interpolated_seconds = round_seconds(track.interpolated_seconds);
        track.loose_seconds = round_seconds(track.loose_seconds);
        totals.seconds += track.duration;
        totals.matched_seconds += track.matched_seconds;
        totals.interpolated_seconds += track.interpolated_seconds;
        totals.loose_seconds += track.loose_seconds;
        totals.tracks += 1;
        if track.matched_seconds == 0.0 && track.interpolated_seconds == 0.0 {
            totals.unaligned_tracks += 1;
        }
    }
    totals.seconds = round_seconds(totals.seconds);
    totals.matched_seconds = round_seconds(totals.matched_seconds);
    totals.interpolated_seconds = round_seconds(totals.interpolated_seconds);
    totals.loose_seconds = round_seconds(totals.loose_seconds);

    ReportV4 {
        version: REPORT_VERSION,
        aligner: input.aligner,
        auto_exclude: input.auto_exclude,
        spine,
        tracks,
        spans,
        holes,
        totals,
    }
}

/// Sentence context for report UI (ST `buildAlignerReport` first/last match).
fn sentence_context_at(texts: &[String], id: usize) -> SentenceContext {
    SentenceContext {
        prev_sentence: id.checked_sub(1).and_then(|i| texts.get(i).cloned()),
        matched_sentence: texts.get(id).cloned().unwrap_or_default(),
        next_sentence: texts.get(id + 1).cloned(),
    }
}

/// Preview snippet for report UI — first non-empty sentence text, truncated.
fn chapter_preview(texts: &[String]) -> Option<String> {
    let raw = texts
        .iter()
        .find(|t| t.chars().any(|c| !c.is_whitespace()))?;
    let trimmed: String = raw.chars().take(160).collect();
    Some(trimmed)
}

/// ST `buildAlignerReport` from packaged MarkedChapters — first/last match,
/// preview, audio-only spine rows + holes (report UI polish).
pub fn build_report_from_chapters(
    aligner: ReportAligner,
    auto_exclude: bool,
    chapters: &[MarkedChapter],
    audio_files: &[std::path::PathBuf],
    package_audio_duration_secs: Option<f64>,
) -> ReportV4 {
    let track_names: Vec<String> = audio_files
        .iter()
        .map(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("audio")
                .to_string()
        })
        .collect();
    let track_index_map: HashMap<String, i32> = track_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i as i32))
        .collect();
    let track_index = |audiofile: &str| -> i32 {
        let base = Path::new(audiofile)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(audiofile);
        track_index_map.get(base).copied().unwrap_or(-1)
    };

    let mut spine = Vec::new();
    let mut spans = Vec::new();
    let mut report_holes = Vec::new();

    for (i, ch) in chapters.iter().enumerate() {
        let audio_only = ch.audio_heading.is_some();
        let status = if audio_only {
            SpineStatus::AudioOnly
        } else {
            SpineStatus::Aligned
        };
        let sentence_count = if audio_only {
            0
        } else {
            ch.sentences
                .iter()
                .map(|s| s.id)
                .max()
                .map(|m| m + 1)
                .unwrap_or(ch.sentences.len())
        };

        let mut first_match = None;
        let mut last_match = None;
        if sentence_count > 0 {
            let chapter_spans =
                build_chapter_spans(i, sentence_count, &ch.sentences, &track_index, &|_| None);
            let aligned: Vec<_> = chapter_spans
                .iter()
                .filter(|s| s.state == SpanState::Aligned)
                .collect();
            if let (Some(first), Some(last)) = (aligned.first(), aligned.last()) {
                if !ch.sentence_texts.is_empty() {
                    first_match = Some(sentence_context_at(
                        &ch.sentence_texts,
                        first.first_sentence,
                    ));
                    last_match = Some(sentence_context_at(&ch.sentence_texts, last.last_sentence));
                }
            }
            spans.extend(chapter_spans);
        }

        // ST: audio-chapter holes attach as after-sentence-0 on the audio-only row.
        for hole in &ch.holes {
            let mut rh = hole_to_report(i, hole, &track_index);
            if audio_only {
                rh.sentence = 0;
                rh.position = ReportHolePosition::After;
            }
            report_holes.push(rh);
        }

        spine.push(SpineItemInput {
            index: i,
            href: ch.href.clone(),
            title: ch.title.clone(),
            status,
            classification: None,
            decision: None,
            sentence_count,
            first_match,
            last_match,
            preview: if audio_only {
                ch.title.clone()
            } else {
                chapter_preview(&ch.sentence_texts)
            },
        });
    }

    let n = track_names.len().max(1) as f64;
    let per = package_audio_duration_secs.unwrap_or(0.0) / n;
    let mut offset = 0.0;
    let mut tracks = Vec::new();
    for (index, name) in track_names.into_iter().enumerate() {
        tracks.push(TrackInput {
            index,
            name,
            duration: round_seconds(per),
            offset: round_seconds(offset),
            source: None,
        });
        offset += per;
    }

    assemble_report(AssembleReportInput {
        aligner,
        auto_exclude,
        spine,
        tracks,
        spans,
        holes: report_holes,
        excluded_audio: HashMap::new(),
    })
}

fn hole_to_report(spine: usize, hole: &Hole, track_index: &dyn Fn(&str) -> i32) -> ReportHole {
    ReportHole {
        spine,
        sentence: hole.anchor.sentence_id,
        position: match hole.anchor.position {
            HolePosition::Before => ReportHolePosition::Before,
            HolePosition::After => ReportHolePosition::After,
        },
        audio: SpanAudio {
            track: track_index(&hole.audiofile),
            start: round_seconds(hole.start),
            end: round_seconds(hole.end),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::ranges::AudioRange;

    fn range(id: usize, start: f64, end: f64, audiofile: &str, state: RangeState) -> SentenceRange {
        SentenceRange::new(
            id,
            "ch",
            vec![AudioRange {
                audiofile: audiofile.into(),
                start,
                end,
            }],
        )
        .with_state(state)
    }

    fn track_index(file: &str) -> i32 {
        match file {
            "a.mp3" => 0,
            "b.mp3" => 1,
            _ => -1,
        }
    }

    #[test]
    fn build_chapter_spans_tiles_runs() {
        let ranges = vec![
            range(0, 10.0, 12.4, "a.mp3", RangeState::Unmatched),
            range(1, 12.4, 14.0, "a.mp3", RangeState::Matched),
            range(2, 14.0, 15.9, "a.mp3", RangeState::Matched),
            range(3, 15.9, 18.0, "a.mp3", RangeState::Interpolated),
            range(4, 18.0, 21.3, "a.mp3", RangeState::Interpolated),
            range(5, 21.3, 23.0, "a.mp3", RangeState::Matched),
            range(6, 23.0, 30.0, "a.mp3", RangeState::Unmatched),
        ];
        let spans = build_chapter_spans(4, 7, &ranges, &track_index, &|id| match id {
            1 => Some(90.0),
            2 => Some(92.0),
            _ => Some(88.0),
        });
        assert_eq!(spans.len(), 5);
        assert_eq!(spans[0].state, SpanState::Unmatched);
        assert!(spans[0].audio.is_none());
        assert_eq!(spans[1].first_sentence, 1);
        assert_eq!(spans[1].last_sentence, 2);
        assert_eq!(spans[1].state, SpanState::Aligned);
        let a = spans[1].audio.as_ref().unwrap();
        assert_eq!(a.track, 0);
        assert!((a.start - 12.4).abs() < 1e-9);
        assert!((a.end - 15.9).abs() < 1e-9);
        assert_eq!(spans[1].score, Some(91.0));
        assert_eq!(spans[2].state, SpanState::Interpolated);
        assert_eq!(spans[2].first_sentence, 3);
        assert_eq!(spans[2].last_sentence, 4);
        assert_eq!(spans[3].first_sentence, 5);
        assert_eq!(spans[3].score, Some(88.0));
        assert_eq!(spans[4].state, SpanState::Unmatched);
    }

    #[test]
    fn build_chapter_spans_splits_at_track_boundary() {
        let ranges = vec![
            range(0, 100.0, 110.0, "a.mp3", RangeState::Matched),
            range(1, 110.0, 120.0, "a.mp3", RangeState::Matched),
            range(2, 0.0, 5.0, "b.mp3", RangeState::Matched),
            range(3, 5.0, 9.0, "b.mp3", RangeState::Matched),
        ];
        let spans = build_chapter_spans(0, 4, &ranges, &track_index, &|_| None);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].first_sentence, 0);
        assert_eq!(spans[0].last_sentence, 1);
        assert_eq!(spans[0].audio.as_ref().unwrap().track, 0);
        assert_eq!(spans[1].first_sentence, 2);
        assert_eq!(spans[1].last_sentence, 3);
        assert_eq!(spans[1].audio.as_ref().unwrap().track, 1);
    }

    #[test]
    fn build_chapter_spans_keeps_dropped_apart() {
        let ranges = vec![
            range(0, 0.0, 4.0, "a.mp3", RangeState::Matched),
            range(1, 4.0, 4.5, "a.mp3", RangeState::Dropped),
            range(2, 4.5, 8.0, "a.mp3", RangeState::Matched),
            range(3, 8.0, 20.0, "a.mp3", RangeState::Unmatched),
        ];
        let spans = build_chapter_spans(1, 4, &ranges, &track_index, &|_| None);
        assert_eq!(spans[0].state, SpanState::Aligned);
        assert_eq!(spans[1].state, SpanState::Dropped);
        assert!(spans[1].audio.is_none());
        assert_eq!(spans[2].state, SpanState::Aligned);
        assert_eq!(spans[3].state, SpanState::Unmatched);
    }

    #[test]
    fn build_chapter_spans_missing_ranges_unmatched() {
        let spans = build_chapter_spans(2, 3, &[], &track_index, &|_| None);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].first_sentence, 0);
        assert_eq!(spans[0].last_sentence, 2);
        assert_eq!(spans[0].state, SpanState::Unmatched);
    }

    #[test]
    fn assemble_report_rolls_up_totals() {
        let report = assemble_report(AssembleReportInput {
            aligner: ReportAligner::Ctc,
            auto_exclude: true,
            spine: vec![
                SpineItemInput {
                    index: 1,
                    href: "ch1.xhtml".into(),
                    title: Some("One".into()),
                    status: SpineStatus::Aligned,
                    classification: None,
                    decision: None,
                    sentence_count: 4,
                    first_match: None,
                    last_match: None,
                    preview: None,
                },
                SpineItemInput {
                    index: 0,
                    href: "nav.xhtml".into(),
                    title: None,
                    status: SpineStatus::Excluded,
                    classification: Some(ReportClassification {
                        zone: "front".into(),
                        verdict: "exclude".into(),
                        decided_by: Some(serde_json::json!({
                            "source": "nav",
                            "value": "nav",
                            "verdict": "exclude"
                        })),
                        signals: vec![serde_json::json!({
                            "source": "nav",
                            "value": "nav",
                            "verdict": "exclude"
                        })],
                    }),
                    decision: None,
                    sentence_count: 0,
                    first_match: None,
                    last_match: None,
                    preview: None,
                },
                SpineItemInput {
                    index: 2,
                    href: "ch2.xhtml".into(),
                    title: None,
                    status: SpineStatus::Aligned,
                    classification: None,
                    decision: None,
                    sentence_count: 2,
                    first_match: None,
                    last_match: None,
                    preview: None,
                },
            ],
            tracks: vec![
                TrackInput {
                    index: 0,
                    name: "a.mp3".into(),
                    duration: 100.0,
                    offset: 0.0,
                    source: None,
                },
                TrackInput {
                    index: 1,
                    name: "b.mp3".into(),
                    duration: 50.0,
                    offset: 100.0,
                    source: None,
                },
            ],
            spans: vec![
                ReportSpan {
                    spine: 1,
                    first_sentence: 0,
                    last_sentence: 1,
                    state: SpanState::Aligned,
                    audio: Some(SpanAudio {
                        track: 0,
                        start: 0.0,
                        end: 30.0,
                    }),
                    score: Some(80.0),
                },
                ReportSpan {
                    spine: 1,
                    first_sentence: 2,
                    last_sentence: 2,
                    state: SpanState::Interpolated,
                    audio: Some(SpanAudio {
                        track: 0,
                        start: 30.0,
                        end: 40.0,
                    }),
                    score: None,
                },
                ReportSpan {
                    spine: 1,
                    first_sentence: 3,
                    last_sentence: 3,
                    state: SpanState::Aligned,
                    audio: Some(SpanAudio {
                        track: 0,
                        start: 40.0,
                        end: 60.0,
                    }),
                    score: Some(100.0),
                },
                ReportSpan {
                    spine: 2,
                    first_sentence: 0,
                    last_sentence: 1,
                    state: SpanState::Unmatched,
                    audio: None,
                    score: None,
                },
            ],
            holes: Vec::new(),
            excluded_audio: HashMap::new(),
        });

        assert_eq!(report.version, REPORT_VERSION);
        assert_eq!(report.spine[0].index, 0);
        assert_eq!(report.spine[0].status, SpineStatus::Excluded);
        assert_eq!(report.spine[1].status, SpineStatus::Aligned);
        assert_eq!(report.spine[1].score, Some(86.7));
        assert_eq!(report.spine[2].status, SpineStatus::NotFound);
        assert_eq!(report.spine[1].matched_sentence_count, 3);
        assert_eq!(report.spine[1].interpolated_sentence_count, 1);
        assert!((report.spine[1].matched_seconds - 50.0).abs() < 1e-9);
        assert!((report.tracks[0].matched_seconds - 50.0).abs() < 1e-9);
        assert!((report.tracks[0].interpolated_seconds - 10.0).abs() < 1e-9);
        assert_eq!(report.totals.sentences, 6);
        assert_eq!(report.totals.matched_sentences, 3);
        assert_eq!(report.totals.interpolated_sentences, 1);
        assert_eq!(report.totals.unmatched_sentences, 2);
        assert_eq!(report.totals.dropped_sentences, 0);
        assert!((report.totals.seconds - 150.0).abs() < 1e-9);
        assert!((report.totals.matched_seconds - 50.0).abs() < 1e-9);
        assert!((report.totals.interpolated_seconds - 10.0).abs() < 1e-9);
        assert_eq!(report.totals.spine.aligned, 1);
        assert_eq!(report.totals.spine.not_found, 1);
        assert_eq!(report.totals.spine.excluded, 1);
        assert_eq!(report.totals.tracks, 2);
        assert_eq!(report.totals.unaligned_tracks, 1);
    }

    #[test]
    fn build_report_fills_first_last_preview_and_audio_only() {
        use crate::align::holes::{Hole, HoleAnchor, HolePosition};
        use crate::align::readaloud::MarkedChapter;
        use crate::align::smil::TextRef;
        use std::path::PathBuf;

        let aligned = MarkedChapter {
            chapter_id: "c1".into(),
            href: "ch1.xhtml".into(),
            xhtml: "<html/>".into(),
            smil_href: "MediaOverlays/c1.smil".into(),
            sentences: vec![
                range(0, 0.0, 1.0, "a.mp3", RangeState::Matched),
                range(1, 1.0, 2.0, "a.mp3", RangeState::Matched),
                range(2, 2.0, 3.0, "a.mp3", RangeState::Interpolated),
            ],
            holes: Vec::new(),
            fragment_prefix: "sentence".into(),
            text_ref: TextRef::IdFragment,
            sentence_texts: vec!["One.".into(), "Two.".into(), "Three.".into()],
            audio_heading: None,
            spine_insert: None,
            title: Some("Chapter One".into()),
        };
        let listen = MarkedChapter {
            chapter_id: "listen".into(),
            href: "listen.xhtml".into(),
            xhtml: "<html/>".into(),
            smil_href: "MediaOverlays/listen.smil".into(),
            sentences: Vec::new(),
            holes: vec![Hole {
                audiofile: "a.mp3".into(),
                start: 10.0,
                end: 20.0,
                anchor: HoleAnchor {
                    chapter_id: "listen".into(),
                    sentence_id: 99,
                    position: HolePosition::Before,
                },
                boundary: false,
                between: String::new(),
            }],
            fragment_prefix: "sentence".into(),
            text_ref: TextRef::IdFragment,
            sentence_texts: Vec::new(),
            audio_heading: Some("h-listen".into()),
            spine_insert: None,
            title: Some("Listen".into()),
        };
        let report = build_report_from_chapters(
            ReportAligner::Ctc,
            true,
            &[aligned, listen],
            &[PathBuf::from("a.mp3")],
            Some(30.0),
        );
        assert_eq!(report.spine.len(), 2);
        assert_eq!(report.spine[0].status, SpineStatus::Aligned);
        let first = report.spine[0].first_match.as_ref().unwrap();
        assert_eq!(first.matched_sentence, "One.");
        assert_eq!(first.next_sentence.as_deref(), Some("Two."));
        let last = report.spine[0].last_match.as_ref().unwrap();
        assert_eq!(last.matched_sentence, "Two.");
        assert_eq!(last.prev_sentence.as_deref(), Some("One."));
        assert_eq!(report.spine[0].preview.as_deref(), Some("One."));

        assert_eq!(report.spine[1].status, SpineStatus::AudioOnly);
        assert_eq!(report.spine[1].preview.as_deref(), Some("Listen"));
        assert_eq!(report.totals.spine.audio_only, 1);
        assert_eq!(report.holes.len(), 1);
        assert_eq!(report.holes[0].spine, 1);
        assert_eq!(report.holes[0].sentence, 0);
        assert_eq!(report.holes[0].position, ReportHolePosition::After);
    }
}
