//! CTC align support (Storyteller `align/ctc` + `emit`).
//!
//! Storyteller generates neural emissions via `@storyteller-platform/ghost-story`
//! (MMS / ONNX), then forced-aligns EPUB text with `ctcViterbiAlign`. Shelf’s
//! Combine default remains Whisper + errorAlign. This module ports the **frame
//! timing math**, **Viterbi force-align**, **dropped-label detect**, and
//! **greedy decode** over in-memory emission frames.
//!
//! Emit feed: `ctc_emit` + Compose sidecar when `SHELF_CTC_EMIT_*` set; see `ctc_align`.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmissionTimings {
    pub seconds_per_chunk: f64,
    pub seconds_per_frame: f64,
    pub frames_per_chunk: usize,
    pub start_offset_seconds: f64,
    pub end_offset_seconds: f64,
}

/// Flat row-major emission scores: `values[t * classes + c]`.
#[derive(Debug, Clone)]
pub struct Emissions {
    pub values: Vec<f32>,
    pub frames: usize,
    pub classes: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenAlignment {
    pub label: i32,
    pub start_frame: i32,
    pub end_frame: i32,
    pub score: f64,
}

/// Storyteller `secondsToFrames`.
pub fn seconds_to_frames(seconds: f64, t: EmissionTimings) -> usize {
    if t.seconds_per_chunk <= 0.0 || t.seconds_per_frame <= 0.0 {
        return 0;
    }
    let chunks = (seconds / t.seconds_per_chunk).floor() as usize;
    let remainder = seconds - (chunks as f64) * t.seconds_per_chunk;
    chunks * t.frames_per_chunk + (remainder / t.seconds_per_frame).floor() as usize
}

/// Storyteller `framesToSeconds`.
pub fn frames_to_seconds(frames: usize, t: EmissionTimings) -> f64 {
    let chunks = frames / t.frames_per_chunk.max(1);
    let rem = frames % t.frames_per_chunk.max(1);
    (chunks as f64) * t.seconds_per_chunk + (rem as f64) * t.seconds_per_frame
}

/// Storyteller `boundaryOffsetFrames`.
pub fn boundary_offset_frames(t: EmissionTimings) -> (isize, isize) {
    let start = (t.start_offset_seconds / t.seconds_per_frame).round() as isize;
    let end = (t.end_offset_seconds / t.seconds_per_frame).round() as isize;
    (start, end)
}

pub fn argmax(values: &[f32], offset: usize, length: usize) -> usize {
    let mut best = 0usize;
    for i in 1..length {
        if values[offset + i] > values[offset + best] {
            best = i;
        }
    }
    best
}

/// Storyteller `ctcGreedyDecode` over a single in-memory emissions block.
pub fn ctc_greedy_decode(
    emissions: &Emissions,
    vocab: &[(char, i32)],
    blank_id: i32,
) -> (String, Vec<usize>) {
    let mut char_by_token = std::collections::HashMap::new();
    for &(ch, id) in vocab {
        if id != blank_id && ch != '|' {
            char_by_token.insert(id, ch);
        }
    }
    let mut chars = String::new();
    let mut frames = Vec::new();
    let mut previous = -1i32;
    for t in 0..emissions.frames {
        let best = argmax(&emissions.values, t * emissions.classes, emissions.classes) as i32;
        if best != previous {
            if let Some(ch) = char_by_token.get(&best) {
                chars.push(*ch);
                frames.push(t);
            }
        }
        previous = best;
    }
    (chars, frames)
}

/// Storyteller `ctcViterbiAlign`.
pub fn ctc_viterbi_align(
    emissions: &Emissions,
    labels: &[i32],
    blank_id: i32,
    star_id: i32,
    star_score: f32,
) -> Vec<TokenAlignment> {
    if labels.is_empty() || emissions.frames == 0 {
        return Vec::new();
    }
    let num_frames = emissions.frames;
    let classes = emissions.classes;

    let num_states = 2 * labels.len() + 1;
    let mut tokens = vec![blank_id; num_states];
    for (i, &label) in labels.iter().enumerate() {
        tokens[2 * i + 1] = label;
    }
    let mut skip_legal = vec![0u8; num_states];
    for s in 2..num_states {
        skip_legal[s] = if tokens[s] != tokens[s - 2] || tokens[s] == star_id {
            1
        } else {
            0
        };
    }

    let star_column = classes;
    let columns = classes + 1;
    let mut scores = vec![0f32; num_frames * columns];
    for t in 0..num_frames {
        let src = t * classes;
        let dst = t * columns;
        scores[dst..dst + classes].copy_from_slice(&emissions.values[src..src + classes]);
        scores[dst + star_column] = star_score;
    }
    let mut state_column = vec![0i32; num_states];
    for s in 0..num_states {
        state_column[s] = if tokens[s] == star_id {
            star_column as i32
        } else {
            tokens[s]
        };
    }

    let mut prev_scores = vec![f32::NEG_INFINITY; num_states];
    let mut curr_scores = vec![0f32; num_states];
    let mut backpointers = vec![0u8; num_frames * num_states];

    prev_scores[0] = scores[state_column[0] as usize];
    if num_states > 1 {
        prev_scores[1] = scores[state_column[1] as usize];
    }
    if labels[0] == star_id && num_states > 3 {
        prev_scores[2] = scores[state_column[2] as usize];
        prev_scores[3] = scores[state_column[3] as usize];
    }

    for t in 1..num_frames {
        let row = t * columns;
        let back_row = t * num_states;
        for s in 0..num_states {
            let mut best = prev_scores[s];
            let mut best_op = 0u8;
            if s > 0 && prev_scores[s - 1] > best {
                best = prev_scores[s - 1];
                best_op = 1;
            }
            if skip_legal[s] != 0 && s >= 2 && prev_scores[s - 2] > best {
                best = prev_scores[s - 2];
                best_op = 2;
            }
            curr_scores[s] = scores[row + state_column[s] as usize] + best;
            backpointers[back_row + s] = best_op;
        }
        std::mem::swap(&mut prev_scores, &mut curr_scores);
    }

    let mut terminal_candidates = vec![num_states - 1, num_states.saturating_sub(2)];
    if labels.last() == Some(&star_id) {
        if num_states >= 3 {
            terminal_candidates.push(num_states - 3);
        }
        if num_states >= 4 {
            terminal_candidates.push(num_states - 4);
        }
    }
    terminal_candidates.retain(|&s| s < num_states);
    let mut terminal = terminal_candidates[0];
    for &s in &terminal_candidates {
        if prev_scores[s] > prev_scores[terminal] {
            terminal = s;
        }
    }

    if prev_scores[terminal].is_infinite() && prev_scores[terminal].is_sign_negative() {
        return labels
            .iter()
            .map(|&label| TokenAlignment {
                label,
                start_frame: -1,
                end_frame: -1,
                score: f64::NEG_INFINITY,
            })
            .collect();
    }

    let mut path = vec![0i32; num_frames];
    path[num_frames - 1] = terminal as i32;
    for t in (1..num_frames).rev() {
        let s = path[t] as usize;
        let op = backpointers[t * num_states + s] as i32;
        path[t - 1] = path[t] - op;
    }

    let mut first_frames = vec![-1i32; num_states];
    let mut last_frames = vec![-1i32; num_states];
    let mut score_sums = vec![0f64; num_states];
    let mut frame_counts = vec![0i32; num_states];
    for (t, &s_i) in path.iter().enumerate() {
        let s = s_i as usize;
        if first_frames[s] == -1 {
            first_frames[s] = t as i32;
        }
        last_frames[s] = t as i32;
        score_sums[s] += f64::from(scores[t * columns + state_column[s] as usize]);
        frame_counts[s] += 1;
    }

    let mut alignments = Vec::with_capacity(labels.len());
    for (l, &label) in labels.iter().enumerate() {
        let state = 2 * l + 1;
        let start_frame = first_frames[state];
        let end_frame = last_frames[state];
        let mean_score = if frame_counts[state] > 0 {
            score_sums[state] / f64::from(frame_counts[state])
        } else {
            f64::NEG_INFINITY
        };
        alignments.push(TokenAlignment {
            label,
            start_frame,
            end_frame,
            score: mean_score,
        });
    }
    alignments
}

/// Storyteller `detectDroppedLabels`.
pub fn detect_dropped_labels(
    alignments: &[TokenAlignment],
    start_offset: usize,
    end_offset: usize,
    min_length: usize,
) -> bool {
    if end_offset < start_offset || end_offset - start_offset <= min_length {
        return false;
    }
    let Some(end_a) = alignments.get(end_offset) else {
        return false;
    };
    let Some(start_a) = alignments.get(start_offset) else {
        return false;
    };
    let frames_per_char =
        f64::from(end_a.end_frame - start_a.start_frame) / (end_offset - start_offset) as f64;
    if frames_per_char >= 2.0 {
        return false;
    }
    let span = end_offset + 1 - start_offset;
    let good = alignments[start_offset..=end_offset]
        .iter()
        .filter(|a| a.score > -4.0)
        .count();
    (good as f64) / (span as f64) < 0.5
}

/// CTC search anchor (Storyteller `Match`): char offset in baseline ↔ frame position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForcedAlignAnchor {
    pub offset: usize,
    pub position: usize,
}

const STAR_SCORE: f32 = -3.0;

fn remove_label_ranges(input: &[i32], removals: &[(usize, usize)]) -> (Vec<i32>, Vec<usize>) {
    let mut input_indices = Vec::new();
    let mut result = Vec::new();
    let mut next_removal = 0usize;
    let mut i = 0usize;
    while i < input.len() {
        let Some(&(r0, r1)) = removals.get(next_removal) else {
            input_indices.push(i);
            result.push(input[i]);
            i += 1;
            continue;
        };
        if i >= r0 && i <= r1 {
            i += 1;
            continue;
        }
        if i > r1 {
            next_removal += 1;
            continue;
        }
        input_indices.push(i);
        result.push(input[i]);
        i += 1;
    }
    (result, input_indices)
}

fn baseline_chars(baseline: &str) -> Vec<char> {
    baseline.chars().collect()
}

fn emissions_slice(emissions: &Emissions, frames_from: usize, frames_to: usize) -> Emissions {
    let classes = emissions.classes;
    let from = frames_from.min(emissions.frames);
    let to = frames_to.max(from).min(emissions.frames);
    let start = from * classes;
    let end = to * classes;
    Emissions {
        values: emissions.values[start..end].to_vec(),
        frames: to - from,
        classes,
    }
}

/// Storyteller `ctcForcedAlign`.
///
/// `drop_labels(label_offset, alignments)` returns inclusive `[start, end]` ranges
/// in **baseline** indices to drop and re-align (empty = no drop).
pub fn ctc_forced_align<F: FnMut(usize, &[TokenAlignment]) -> Vec<(usize, usize)>>(
    baseline: &str,
    emissions: &Emissions,
    anchors: &[ForcedAlignAnchor],
    vocab: &std::collections::HashMap<String, i32>,
    blank_id: i32,
    mut drop_labels: F,
) -> Vec<TokenAlignment> {
    let star = vocab.len() as i32;
    let chars = baseline_chars(baseline);

    struct Segment {
        labels: Vec<i32>,
        labels_start: usize,
        emissions: Emissions,
        emissions_start: usize,
    }

    let mut segments: Vec<Segment> = Vec::new();
    for i in 0..=anchors.len() {
        let prev = if i == 0 { None } else { anchors.get(i - 1) };
        let next = anchors.get(i);
        let frames_from = prev.map(|a| a.position).unwrap_or(0);
        let frames_to = next.map(|a| a.position).unwrap_or(emissions.frames);
        let labels_start = prev.map(|a| a.offset).unwrap_or(0);
        let labels_end = next.map(|a| a.offset).unwrap_or(chars.len());
        let labels: Vec<i32> = chars[labels_start.min(chars.len())..labels_end.min(chars.len())]
            .iter()
            .map(|c| {
                let key = c.to_string();
                vocab.get(&key).copied().unwrap_or(star)
            })
            .collect();
        segments.push(Segment {
            labels,
            labels_start,
            emissions: emissions_slice(emissions, frames_from, frames_to),
            emissions_start: frames_from,
        });
    }

    if let Some(first) = segments.first_mut() {
        first.labels.insert(0, star);
    }
    if let Some(last) = segments.last_mut() {
        last.labels.push(star);
    }

    let n_seg = segments.len();
    let mut alignments_with_removals: Vec<TokenAlignment> = Vec::new();
    let mut input_indices: Vec<usize> = Vec::new();

    for (seg_i, segment) in segments.iter().enumerate() {
        let is_first = seg_i == 0;
        let is_last = seg_i + 1 == n_seg;

        let strip_stars = |labels: &[i32]| -> Vec<i32> {
            let mut r = labels.to_vec();
            if is_first && !r.is_empty() {
                r.remove(0);
            }
            if is_last && !r.is_empty() {
                r.pop();
            }
            r
        };

        let stripped = strip_stars(&segment.labels);
        let mut indices: Vec<usize> = (0..stripped.len()).collect();

        let mut unmapped = ctc_viterbi_align(
            &segment.emissions,
            &segment.labels,
            blank_id,
            star,
            STAR_SCORE,
        );

        let stripped_al = {
            let mut a = unmapped.clone();
            if is_first && !a.is_empty() {
                a.remove(0);
            }
            if is_last && !a.is_empty() {
                a.pop();
            }
            a
        };

        let dropped: Vec<(usize, usize)> = drop_labels(segment.labels_start, &stripped_al)
            .into_iter()
            .map(|(s, e)| {
                (
                    s.saturating_sub(segment.labels_start),
                    e.saturating_sub(segment.labels_start),
                )
            })
            .collect();

        if !dropped.is_empty() {
            let (mut adjusted_labels, adjusted_indices) = remove_label_ranges(&stripped, &dropped);
            indices = adjusted_indices;
            if is_first {
                adjusted_labels.insert(0, star);
            }
            if is_last {
                adjusted_labels.push(star);
            }
            unmapped = ctc_viterbi_align(
                &segment.emissions,
                &adjusted_labels,
                blank_id,
                star,
                STAR_SCORE,
            );
        }

        let segment_alignments: Vec<TokenAlignment> = unmapped
            .into_iter()
            .map(|mut a| {
                if a.start_frame >= 0 {
                    a.start_frame += segment.emissions_start as i32;
                }
                if a.end_frame >= 0 {
                    a.end_frame += segment.emissions_start as i32;
                }
                a
            })
            .collect();

        input_indices.extend(indices.into_iter().map(|i| i + segment.labels_start));
        alignments_with_removals.extend(segment_alignments);
    }

    // Slice off leading/trailing stars inserted above.
    let core: Vec<TokenAlignment> = if alignments_with_removals.len() >= 2 {
        alignments_with_removals[1..alignments_with_removals.len() - 1].to_vec()
    } else {
        Vec::new()
    };

    let mut alignments: Vec<Option<TokenAlignment>> = vec![None; chars.len()];
    let mut last_undropped_index: isize = -1;
    let mut last_end_frame = 0i32;

    for (k, a) in core.into_iter().enumerate() {
        let Some(&i) = input_indices.get(k) else {
            break;
        };
        // Fill dropped gaps between last kept index and i.
        let fill_from = (last_undropped_index + 1).max(0) as usize;
        if i > fill_from {
            for (j, slot) in alignments.iter_mut().enumerate().take(i).skip(fill_from) {
                let label = chars
                    .get(j)
                    .map(|c| vocab.get(&c.to_string()).copied().unwrap_or(star))
                    .unwrap_or(star);
                *slot = Some(TokenAlignment {
                    label,
                    start_frame: last_end_frame,
                    end_frame: last_end_frame,
                    score: f64::NEG_INFINITY,
                });
            }
        }
        last_end_frame = a.end_frame.max(0);
        last_undropped_index = i as isize;
        if let Some(slot) = alignments.get_mut(i) {
            *slot = Some(a);
        }
    }

    // Any trailing dropped chars after the last kept index.
    let fill_from = (last_undropped_index + 1).max(0) as usize;
    for (j, slot) in alignments.iter_mut().enumerate().skip(fill_from) {
        let label = chars
            .get(j)
            .map(|c| vocab.get(&c.to_string()).copied().unwrap_or(star))
            .unwrap_or(star);
        *slot = Some(TokenAlignment {
            label,
            start_frame: last_end_frame,
            end_frame: last_end_frame,
            score: f64::NEG_INFINITY,
        });
    }

    alignments
        .into_iter()
        .enumerate()
        .map(|(j, slot)| {
            slot.unwrap_or_else(|| {
                let label = chars
                    .get(j)
                    .map(|c| vocab.get(&c.to_string()).copied().unwrap_or(star))
                    .unwrap_or(star);
                TokenAlignment {
                    label,
                    start_frame: 0,
                    end_frame: 0,
                    score: f64::NEG_INFINITY,
                }
            })
        })
        .collect()
}

/// `ctc_forced_align` with no dropped-label callback.
pub fn ctc_forced_align_simple(
    baseline: &str,
    emissions: &Emissions,
    anchors: &[ForcedAlignAnchor],
    vocab: &std::collections::HashMap<String, i32>,
    blank_id: i32,
) -> Vec<TokenAlignment> {
    ctc_forced_align(baseline, emissions, anchors, vocab, blank_id, |_, _| {
        Vec::new()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_timings() -> EmissionTimings {
        EmissionTimings {
            seconds_per_chunk: 1.0,
            seconds_per_frame: 0.02,
            frames_per_chunk: 50,
            start_offset_seconds: 0.04,
            end_offset_seconds: 0.06,
        }
    }

    #[test]
    fn round_trips_chunk_aligned_seconds() {
        let t = sample_timings();
        let frames = seconds_to_frames(2.0, t);
        assert_eq!(frames, 100);
        assert!((frames_to_seconds(frames, t) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn boundary_offsets_round_frames() {
        let (s, e) = boundary_offset_frames(sample_timings());
        assert_eq!(s, 2);
        assert_eq!(e, 3);
    }

    #[test]
    fn greedy_decode_reads_argmax_path() {
        // 3 frames, 3 classes: blank=0, a=1, b=2
        let emissions = Emissions {
            values: vec![
                0.1, 0.9, 0.0, // a
                0.9, 0.1, 0.0, // blank
                0.0, 0.1, 0.9, // b
            ],
            frames: 3,
            classes: 3,
        };
        let vocab = vec![('a', 1), ('b', 2)];
        let (text, frames) = ctc_greedy_decode(&emissions, &vocab, 0);
        assert_eq!(text, "ab");
        assert_eq!(frames, vec![0, 2]);
    }

    #[test]
    fn viterbi_aligns_simple_path() {
        // Force high scores on blank/label sequence for labels [1].
        let frames = 4;
        let classes = 3; // 0 blank, 1 label, 2 other
        let mut values = vec![0.0f32; frames * classes];
        for t in 0..frames {
            values[t * classes] = -1.0; // blank
            values[t * classes + 1] = if t == 1 || t == 2 { 0.0 } else { -5.0 };
            values[t * classes + 2] = -10.0;
        }
        let emissions = Emissions {
            values,
            frames,
            classes,
        };
        let al = ctc_viterbi_align(&emissions, &[1], 0, 99, -3.0);
        assert_eq!(al.len(), 1);
        assert_eq!(al[0].label, 1);
        assert!(al[0].start_frame >= 0);
    }

    #[test]
    fn forced_align_tiny_emissions() {
        use std::collections::HashMap;
        // Vocab: blank="-"=0, a=1, b=2. star_id = 3.
        let mut vocab = HashMap::new();
        vocab.insert("-".into(), 0);
        vocab.insert("a".into(), 1);
        vocab.insert("b".into(), 2);
        let frames = 6;
        let classes = 3;
        let mut values = vec![-10.0f32; frames * classes];
        // Prefer blank on odd frames, a then b on even peaks.
        for t in 0..frames {
            values[t * classes] = -1.0; // blank
        }
        values[classes + 1] = 0.0; // a @ frame 1
        values[2 * classes + 1] = 0.0;
        values[4 * classes + 2] = 0.0; // b @ frame 4
        values[5 * classes + 2] = 0.0;
        let emissions = Emissions {
            values,
            frames,
            classes,
        };
        let al = ctc_forced_align_simple("ab", &emissions, &[], &vocab, 0);
        assert_eq!(al.len(), 2);
        assert_eq!(al[0].label, 1);
        assert_eq!(al[1].label, 2);
        assert!(al[0].start_frame >= 0);
        assert!(al[1].start_frame >= al[0].start_frame);
    }
}
