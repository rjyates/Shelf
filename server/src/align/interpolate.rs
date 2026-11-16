//! Fill unmatched sentence slots between successful matches.
//!
//! Port of Storyteller `interpolateSentenceRanges` (including multi-file gaps).

use std::collections::{HashMap, HashSet};

use crate::align::ranges::{AudioRange, RangeState, SentenceRange};

type Slot = (String, usize); // (chapter_id, sentence_id)

/// Storyteller `InterpolationOptions` — weight-0 dropped slots when `drop_unread`.
#[derive(Debug, Clone, Default)]
pub struct InterpolateOptions<'a> {
    pub dropped_slots: Option<&'a HashSet<Slot>>,
    pub drop_unread: bool,
}

fn slot_weight(slot: &Slot, lengths: &HashMap<Slot, usize>, opts: &InterpolateOptions<'_>) -> f64 {
    if opts.drop_unread && opts.dropped_slots.is_some_and(|d| d.contains(slot)) {
        return 0.0;
    }
    lengths
        .get(slot)
        .copied()
        .filter(|&n| n > 0)
        .map(|n| n as f64)
        .unwrap_or(1.0)
}

fn slot_state(slot: &Slot, base: RangeState, opts: &InterpolateOptions<'_>) -> RangeState {
    if opts.dropped_slots.is_some_and(|d| d.contains(slot)) {
        RangeState::Dropped
    } else {
        base
    }
}

#[allow(clippy::too_many_arguments)] // mirrors Storyteller buildGapRanges weight push
fn push_weighted(
    out: &mut Vec<SentenceRange>,
    slots: &[Slot],
    audiofile: &str,
    start: f64,
    avail: f64,
    base_state: RangeState,
    lengths: &HashMap<Slot, usize>,
    opts: &InterpolateOptions<'_>,
) {
    let total_w: f64 = slots.iter().map(|s| slot_weight(s, lengths, opts)).sum();
    let mut cursor = start;
    for slot in slots {
        let w = slot_weight(slot, lengths, opts);
        let s = cursor;
        let e = if total_w > 0.0 {
            s + avail * w / total_w
        } else {
            s
        };
        cursor = e;
        // Zero-weight dropped slots still get a tiny stub so SMIL has a clip;
        // collapse_ranges with drop_unread removes them later.
        let end = if w == 0.0 {
            s + 0.001
        } else {
            e.max(s + 0.001)
        };
        out.push(
            SentenceRange::new(
                slot.1,
                slot.0.clone(),
                vec![AudioRange {
                    audiofile: audiofile.to_string(),
                    start: s,
                    end,
                }],
            )
            .with_state(slot_state(slot, base_state, opts)),
        );
    }
}

/// Storyteller `buildGapRanges` — same-file span, or left remainder + right head.
#[allow(clippy::too_many_arguments)] // ST buildGapRanges arity + durations/opts
fn build_gap(
    slots: &[Slot],
    left: &AudioRange,
    right: &AudioRange,
    state: RangeState,
    lengths: &HashMap<Slot, usize>,
    durations: &HashMap<String, f64>,
    chapter_split: Option<usize>,
    opts: &InterpolateOptions<'_>,
) -> Vec<SentenceRange> {
    if slots.is_empty() {
        return Vec::new();
    }

    if left.audiofile == right.audiofile {
        let mut out = Vec::new();
        let span = (right.start - left.end).max(0.0);
        push_weighted(
            &mut out,
            slots,
            &left.audiofile,
            left.end,
            span,
            state,
            lengths,
            opts,
        );
        return out;
    }

    let left_dur = durations.get(&left.audiofile).copied().unwrap_or(left.end);
    let left_avail = (left_dur - left.end).max(0.0);
    let right_avail = right.start.max(0.0);
    let total = left_avail + right_avail;
    let weights: Vec<f64> = slots
        .iter()
        .map(|s| slot_weight(s, lengths, opts))
        .collect();
    let total_w: f64 = weights.iter().sum();

    let n1 = if let Some(split) = chapter_split {
        if left_avail > 0.0 && right_avail > 0.0 {
            split.min(slots.len())
        } else {
            // fall through to time-proportional
            let left_share = if total > 0.0 {
                total_w * (left_avail / total)
            } else {
                total_w
            };
            let mut n1 = 0usize;
            let mut acc = 0.0;
            while n1 < slots.len() && acc + weights[n1] / 2.0 <= left_share {
                acc += weights[n1];
                n1 += 1;
            }
            n1
        }
    } else {
        let left_share = if total > 0.0 {
            total_w * (left_avail / total)
        } else {
            total_w
        };
        let mut n1 = 0usize;
        let mut acc = 0.0;
        while n1 < slots.len() && acc + weights[n1] / 2.0 <= left_share {
            acc += weights[n1];
            n1 += 1;
        }
        n1
    };

    let mut out = Vec::new();
    if n1 > 0 {
        push_weighted(
            &mut out,
            &slots[..n1],
            &left.audiofile,
            left.end,
            left_avail,
            state,
            lengths,
            opts,
        );
    }
    if n1 < slots.len() {
        push_weighted(
            &mut out,
            &slots[n1..],
            &right.audiofile,
            0.0,
            right_avail,
            state,
            lengths,
            opts,
        );
    }
    out
}

/// `sentence_ranges` must be matched anchors in book order (chapter then id).
/// `chapter_order` is spine order of chapter ids (needed when a whole chapter
/// has zero matches — Storyteller's TS interpolate skips those unless callers
/// keep a sparse list that still walks the spine).
/// `chapter_sentence_counts` maps chapter_id → total sentences in that chapter.
/// `sentence_lengths` weights gap slots by character length (Storyteller).
pub fn interpolate_sentence_ranges(
    sentence_ranges: &[SentenceRange],
    chapter_order: &[String],
    chapter_sentence_counts: &HashMap<String, usize>,
    audio_file_durations: &HashMap<String, f64>,
    sentence_lengths: &HashMap<Slot, usize>,
) -> Vec<SentenceRange> {
    interpolate_sentence_ranges_with(
        sentence_ranges,
        chapter_order,
        chapter_sentence_counts,
        audio_file_durations,
        sentence_lengths,
        &InterpolateOptions::default(),
    )
}

pub fn interpolate_sentence_ranges_with(
    sentence_ranges: &[SentenceRange],
    chapter_order: &[String],
    chapter_sentence_counts: &HashMap<String, usize>,
    audio_file_durations: &HashMap<String, f64>,
    sentence_lengths: &HashMap<Slot, usize>,
    opts: &InterpolateOptions<'_>,
) -> Vec<SentenceRange> {
    if sentence_ranges.is_empty() {
        return Vec::new();
    }

    let index_of = |id: &str| chapter_order.iter().position(|c| c == id);

    let mut result: Vec<SentenceRange> = Vec::new();
    let first = &sentence_ranges[0];

    // Leading unmatched chapters before the first match.
    let mut lead: Vec<Slot> = Vec::new();
    if let Some(fi) = index_of(&first.chapter_id) {
        for ch in &chapter_order[..fi] {
            let total = chapter_sentence_counts.get(ch).copied().unwrap_or(0);
            for id in 0..total {
                lead.push((ch.clone(), id));
            }
        }
    }
    for id in 0..first.id {
        lead.push((first.chapter_id.clone(), id));
    }
    if !lead.is_empty() {
        let left = AudioRange {
            audiofile: first.first().audiofile.clone(),
            start: 0.0,
            end: 0.0,
        };
        let right = first.first().clone();
        result.extend(build_gap(
            &lead,
            &left,
            &right,
            RangeState::Unmatched,
            sentence_lengths,
            audio_file_durations,
            None,
            opts,
        ));
    }
    result.push(first.clone());

    for idx in 1..sentence_ranges.len() {
        let prev = &sentence_ranges[idx - 1];
        let curr = &sentence_ranges[idx];
        let left = prev.last().clone();
        let right = curr.first().clone();

        let mut gap_slots: Vec<Slot> = Vec::new();
        let mut chapter_split: Option<usize> = None;
        if prev.chapter_id == curr.chapter_id {
            for id in (prev.id + 1)..curr.id {
                gap_slots.push((prev.chapter_id.clone(), id));
            }
        } else {
            let prev_total = chapter_sentence_counts
                .get(&prev.chapter_id)
                .copied()
                .unwrap_or(prev.id + 1);
            for id in (prev.id + 1)..prev_total {
                gap_slots.push((prev.chapter_id.clone(), id));
            }
            if let (Some(pi), Some(ci)) = (index_of(&prev.chapter_id), index_of(&curr.chapter_id)) {
                if ci > pi + 1 {
                    for ch in &chapter_order[pi + 1..ci] {
                        let total = chapter_sentence_counts.get(ch).copied().unwrap_or(0);
                        for id in 0..total {
                            gap_slots.push((ch.clone(), id));
                        }
                    }
                }
            }
            // Storyteller: slots belonging to earlier chapter stay with left file.
            chapter_split = Some(gap_slots.len());
            for id in 0..curr.id {
                gap_slots.push((curr.chapter_id.clone(), id));
            }
        }

        if !gap_slots.is_empty() {
            let state = if prev.chapter_id == curr.chapter_id {
                RangeState::Interpolated
            } else {
                RangeState::Unmatched
            };
            result.extend(build_gap(
                &gap_slots,
                &left,
                &right,
                state,
                sentence_lengths,
                audio_file_durations,
                chapter_split,
                opts,
            ));
        }
        result.push(curr.clone());
    }

    let last = sentence_ranges.last().expect("non-empty");
    let mut trail: Vec<Slot> = Vec::new();
    let last_total = chapter_sentence_counts
        .get(&last.chapter_id)
        .copied()
        .unwrap_or(last.id + 1);
    for id in (last.id + 1)..last_total {
        trail.push((last.chapter_id.clone(), id));
    }
    if let Some(li) = index_of(&last.chapter_id) {
        for ch in &chapter_order[li + 1..] {
            let total = chapter_sentence_counts.get(ch).copied().unwrap_or(0);
            for id in 0..total {
                trail.push((ch.clone(), id));
            }
        }
    }
    if !trail.is_empty() {
        let file_end = audio_file_durations
            .get(&last.last().audiofile)
            .copied()
            .unwrap_or(last.last().end);
        let left = last.last().clone();
        let right = AudioRange {
            audiofile: left.audiofile.clone(),
            start: file_end,
            end: file_end,
        };
        result.extend(build_gap(
            &trail,
            &left,
            &right,
            RangeState::Unmatched,
            sentence_lengths,
            audio_file_durations,
            None,
            opts,
        ));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matched(chapter: &str, id: usize, start: f64, end: f64) -> SentenceRange {
        SentenceRange::new(
            id,
            chapter,
            vec![AudioRange {
                audiofile: "a.mp3".into(),
                start,
                end,
            }],
        )
    }

    #[test]
    fn fills_gap_between_matches_by_length() {
        let anchors = vec![matched("c1", 0, 0.0, 1.0), matched("c1", 3, 4.0, 5.0)];
        let order = vec!["c1".into()];
        let mut counts = HashMap::new();
        counts.insert("c1".into(), 4);
        let mut lengths = HashMap::new();
        lengths.insert(("c1".into(), 1), 10);
        lengths.insert(("c1".into(), 2), 30);
        let mut durs = HashMap::new();
        durs.insert("a.mp3".into(), 10.0);

        let got = interpolate_sentence_ranges(&anchors, &order, &counts, &durs, &lengths);
        assert_eq!(got.len(), 4);
        assert_eq!(got[1].id, 1);
        assert_eq!(got[2].id, 2);
        assert_eq!(got[1].state, RangeState::Interpolated);
        let d1 = got[1].last().end - got[1].first().start;
        let d2 = got[2].last().end - got[2].first().start;
        assert!(
            d2 > d1 * 2.0,
            "longer sentence should get more time: {d1} vs {d2}"
        );
        assert!((got[2].last().end - 4.0).abs() < 1e-9);
    }

    #[test]
    fn fills_leading_and_trailing() {
        let anchors = vec![matched("c1", 1, 2.0, 3.0)];
        let order = vec!["c1".into()];
        let mut counts = HashMap::new();
        counts.insert("c1".into(), 3);
        let lengths = HashMap::new();
        let mut durs = HashMap::new();
        durs.insert("a.mp3".into(), 9.0);

        let got = interpolate_sentence_ranges(&anchors, &order, &counts, &durs, &lengths);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].id, 0);
        assert_eq!(got[2].id, 2);
        assert!((got[2].last().end - 9.0).abs() < 1e-9);
    }

    #[test]
    fn fills_skipped_middle_chapter() {
        let anchors = vec![matched("c1", 0, 0.0, 1.0), matched("c3", 0, 5.0, 6.0)];
        let order = vec!["c1".into(), "c2".into(), "c3".into()];
        let mut counts = HashMap::new();
        counts.insert("c1".into(), 1);
        counts.insert("c2".into(), 2);
        counts.insert("c3".into(), 1);
        let lengths = HashMap::new();
        let mut durs = HashMap::new();
        durs.insert("a.mp3".into(), 10.0);

        let got = interpolate_sentence_ranges(&anchors, &order, &counts, &durs, &lengths);
        assert_eq!(got.len(), 4);
        assert_eq!(got[1].chapter_id, "c2");
        assert_eq!(got[1].id, 0);
        assert_eq!(got[2].chapter_id, "c2");
        assert_eq!(got[2].id, 1);
    }

    fn matched_file(chapter: &str, id: usize, file: &str, start: f64, end: f64) -> SentenceRange {
        SentenceRange::new(
            id,
            chapter,
            vec![AudioRange {
                audiofile: file.into(),
                start,
                end,
            }],
        )
    }

    #[test]
    fn multi_file_gap_uses_left_remainder_and_right_head() {
        // c1 trailing + c2 leading across a.mp3 → b.mp3 with chapterSplit.
        let anchors = vec![
            matched_file("c1", 0, "a.mp3", 0.0, 8.0),
            matched_file("c2", 1, "b.mp3", 2.0, 3.0),
        ];
        let order = vec!["c1".into(), "c2".into()];
        let mut counts = HashMap::new();
        counts.insert("c1".into(), 2); // gap: c1#1
        counts.insert("c2".into(), 2); // gap: c2#0
        let lengths = HashMap::new();
        let mut durs = HashMap::new();
        durs.insert("a.mp3".into(), 10.0); // left avail = 2.0
        durs.insert("b.mp3".into(), 20.0); // right avail = 2.0

        let got = interpolate_sentence_ranges(&anchors, &order, &counts, &durs, &lengths);
        assert_eq!(got.len(), 4);
        let gap_c1 = &got[1];
        let gap_c2 = &got[2];
        assert_eq!(gap_c1.chapter_id, "c1");
        assert_eq!(gap_c1.id, 1);
        assert_eq!(gap_c1.first().audiofile, "a.mp3");
        assert!((gap_c1.first().start - 8.0).abs() < 1e-9);
        assert!((gap_c1.last().end - 10.0).abs() < 1e-9);
        assert_eq!(gap_c2.chapter_id, "c2");
        assert_eq!(gap_c2.id, 0);
        assert_eq!(gap_c2.first().audiofile, "b.mp3");
        assert!((gap_c2.first().start - 0.0).abs() < 1e-9);
        assert!((gap_c2.last().end - 2.0).abs() < 1e-9);
    }

    #[test]
    fn drop_unread_zeros_dropped_slot_weight() {
        use std::collections::HashSet;
        let anchors = vec![matched("c1", 0, 0.0, 1.0), matched("c1", 2, 5.0, 6.0)];
        let order = vec!["c1".into()];
        let mut counts = HashMap::new();
        counts.insert("c1".into(), 3);
        let lengths = HashMap::new();
        let mut durs = HashMap::new();
        durs.insert("a.mp3".into(), 10.0);
        let mut dropped = HashSet::new();
        dropped.insert(("c1".into(), 1usize));
        let opts = InterpolateOptions {
            dropped_slots: Some(&dropped),
            drop_unread: true,
        };
        let got =
            interpolate_sentence_ranges_with(&anchors, &order, &counts, &durs, &lengths, &opts);
        assert_eq!(got.len(), 3);
        assert_eq!(got[1].state, RangeState::Dropped);
        // Dropped middle gets tiny stub; matched ends keep their times.
        assert!((got[1].last().end - got[1].first().start - 0.001).abs() < 1e-9);
    }
}
