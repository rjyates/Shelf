//! CTC chapter locate (Storyteller `search.ts` `findCtcBoundaries`).
//!
//! Character n-gram RANSAC over a greedy-decoded `PositionedDocument`, then
//! confidence/coverage gates and spaced anchors for `ctc_forced_align`.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use super::ctc::{ctc_greedy_decode, Emissions, ForcedAlignAnchor};

const NGRAM_SIZE: usize = 10;
const RANSAC_ITERATIONS: usize = 500;
const MIN_INLIERS: usize = 10;
const MAX_PAIR_SEPARATION_REQUIREMENT: usize = 200;
const MIN_CONFIDENCE: f64 = 0.5;
const MIN_COVERAGE: f64 = 0.3;
const REFINE_COUNT: usize = 25;
const ANCHOR_SPACING: usize = 2000;
const BOUNDARY_PAD_FRAMES: f64 = 500.0;
const ANCHOR_CONTEXT: usize = 40;
const ANCHOR_AGREEMENT_FRACTION: f64 = 0.4;
const ANCHOR_AGREEMENT_FLOOR: f64 = 0.05;
const ANCHOR_CALIBRATION_MINIMUM: usize = 5;
const EVIDENCE_GAP_MULTIPLIER: f64 = 3.0;
const EVIDENCE_GAP_FLOOR: usize = 500;
const EVIDENCE_GAP_PERCENTILE: f64 = 0.99;
const MIN_EDGE_CLUSTER_OFFSETS: usize = 10;
const EDGE_EXTENSION_MAX_GAP: usize = 1000;
const EDGE_EXTENSION_WINDOW: usize = 250;
const EDGE_EXTENSION_PAIR_SEPARATION: usize = 50;

/// Searchable text with a position (usually emission frame) per character.
///
/// `positions.len()` must equal `text.chars().count()` (Unicode scalars),
/// matching Storyteller/JS string indexing — **not** UTF-8 byte length.
///
/// N-gram indexes are cached per document object (ST `ngramIndexes` WeakMap).
#[derive(Debug)]
pub struct PositionedDocument {
    pub text: String,
    pub positions: Vec<usize>,
    ngram_index: OnceLock<HashMap<String, Vec<usize>>>,
}

impl Clone for PositionedDocument {
    fn clone(&self) -> Self {
        // Fresh index cache — ST WeakMap keys on object identity.
        Self::new(self.text.clone(), self.positions.clone())
    }
}

impl PositionedDocument {
    pub fn new(text: String, positions: Vec<usize>) -> Self {
        Self {
            text,
            positions,
            ngram_index: OnceLock::new(),
        }
    }

    /// Storyteller `getNgramIndex` — build once, reuse across searches.
    fn ngram_index(&self) -> &HashMap<String, Vec<usize>> {
        self.ngram_index.get_or_init(|| {
            let chars = char_vec(&self.text);
            build_ngram_index(&chars, &self.positions)
        })
    }
}

fn char_vec(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn ngram_at(chars: &[char], i: usize) -> Option<String> {
    if i + NGRAM_SIZE > chars.len() {
        None
    } else {
        Some(chars[i..i + NGRAM_SIZE].iter().collect())
    }
}

#[derive(Debug, Clone, Copy)]
struct Match {
    offset: usize,
    position: usize,
}

#[derive(Debug, Clone)]
pub struct BoundaryFitOptions {
    pub min_slope: f64,
    pub max_slope: f64,
    pub inlier_tolerance: f64,
    pub excluded_ranges: Vec<(usize, usize)>,
}

impl Default for BoundaryFitOptions {
    fn default() -> Self {
        Self {
            min_slope: 2.0,
            max_slope: 15.0,
            inlier_tolerance: 2500.0,
            excluded_ranges: Vec::new(),
        }
    }
}

/// Result of CTC boundary search in absolute frame space.
#[derive(Debug, Clone)]
pub struct CtcBoundaries {
    /// Inclusive start frame (Storyteller `start`).
    pub start: usize,
    /// Half-open upper bound for `emissions_slice` / ST `readFrames(start, end)`.
    /// Storyteller names this "end" and clamps to `numFrames - 1`; do **not** add 1.
    pub end: usize,
    pub confidence: f64,
    pub anchors: Vec<ForcedAlignAnchor>,
    /// Query-offset [start, end) padded evidence cluster (Storyteller `evidenceRange`).
    pub evidence_range: (usize, usize),
    /// Query-offset [start, end) tight matched cluster (Storyteller `matchedRange`).
    pub matched_range: (usize, usize),
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    values[values.len() / 2]
}

fn build_ngram_index(chars: &[char], positions: &[usize]) -> HashMap<String, Vec<usize>> {
    let mut index: HashMap<String, Vec<usize>> = HashMap::new();
    if chars.len() < NGRAM_SIZE || positions.len() != chars.len() {
        return index;
    }
    for (i, &position) in positions
        .iter()
        .enumerate()
        .take(chars.len() - NGRAM_SIZE + 1)
    {
        let ngram = ngram_at(chars, i).expect("bounds checked");
        index.entry(ngram).or_default().push(position);
    }
    index
}

fn collect_matches(
    query_chars: &[char],
    index: &HashMap<String, Vec<usize>>,
    excluded_ranges: &[(usize, usize)],
) -> (Vec<Match>, HashMap<String, Vec<usize>>) {
    let mut query_ngram_offsets: HashMap<String, Vec<usize>> = HashMap::new();
    let mut matches = Vec::new();
    if query_chars.len() < NGRAM_SIZE {
        return (matches, query_ngram_offsets);
    }
    for i in 0..=query_chars.len() - NGRAM_SIZE {
        let query_ngram = ngram_at(query_chars, i).expect("bounds checked");
        query_ngram_offsets
            .entry(query_ngram.clone())
            .or_default()
            .push(i);
        let Some(positions) = index.get(&query_ngram) else {
            continue;
        };
        for &position in positions {
            if excluded_ranges
                .iter()
                .any(|&(start, end)| start <= position && position < end)
            {
                continue;
            }
            matches.push(Match {
                offset: i,
                position,
            });
        }
    }
    (matches, query_ngram_offsets)
}

fn fit_line(
    matches: &[Match],
    pair_separation: usize,
    options: &BoundaryFitOptions,
) -> Option<(f64, f64, Vec<Match>)> {
    // Deterministic LCG (Storyteller search.ts) for reproducible RANSAC.
    let mut seed: u32 = 0x2545_f491;
    let mut random = || {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345) & 0x7fff_ffff;
        seed as f64 / 0x8000_0000u32 as f64
    };

    let mut best: Option<(f64, f64, usize)> = None;
    for _ in 0..RANSAC_ITERATIONS {
        let a = matches[(random() * matches.len() as f64).floor() as usize];
        let b = matches[(random() * matches.len() as f64).floor() as usize];
        if a.offset.abs_diff(b.offset) < pair_separation {
            continue;
        }
        let slope = (b.position as f64 - a.position as f64) / (b.offset as f64 - a.offset as f64);
        if slope < options.min_slope || slope > options.max_slope {
            continue;
        }
        let intercept = a.position as f64 - slope * a.offset as f64;
        let mut count = 0usize;
        for m in matches {
            let predicted = intercept + slope * m.offset as f64;
            if (m.position as f64 - predicted).abs() <= options.inlier_tolerance {
                count += 1;
            }
        }
        if best.is_none_or(|(_, _, c)| count > c) {
            best = Some((slope, intercept, count));
        }
    }

    let (best_slope, best_intercept, best_count) = best?;
    if best_count < MIN_INLIERS {
        return None;
    }

    let mut inliers: Vec<Match> = matches
        .iter()
        .copied()
        .filter(|m| {
            let predicted = best_intercept + best_slope * m.offset as f64;
            (m.position as f64 - predicted).abs() <= options.inlier_tolerance
        })
        .collect();
    inliers.sort_by_key(|m| m.offset);

    let stride = (inliers.len() / 2).max(1);
    let mut slopes = Vec::new();
    for i in 0..inliers.len().saturating_sub(stride) {
        let lo = inliers[i];
        let hi = inliers[i + stride];
        if hi.offset - lo.offset < pair_separation {
            continue;
        }
        slopes.push(
            (hi.position as f64 - lo.position as f64) / (hi.offset as f64 - lo.offset as f64),
        );
    }
    let slope = if slopes.is_empty() {
        best_slope
    } else {
        median(slopes)
    };
    let intercept = if inliers.is_empty() {
        best_intercept
    } else {
        median(
            inliers
                .iter()
                .map(|m| m.position as f64 - slope * m.offset as f64)
                .collect(),
        )
    };
    Some((slope, intercept, inliers))
}

fn bigram_similarity(a: &[char], b: &[char]) -> f64 {
    if a.len() < 2 || b.len() < 2 {
        return 0.0;
    }
    let mut bigrams: HashMap<(char, char), usize> = HashMap::new();
    for w in a.windows(2) {
        *bigrams.entry((w[0], w[1])).or_default() += 1;
    }
    let mut shared = 0usize;
    for w in b.windows(2) {
        if let Some(remaining) = bigrams.get_mut(&(w[0], w[1])) {
            if *remaining > 0 {
                shared += 1;
                *remaining -= 1;
            }
        }
    }
    shared as f64 / (a.len() - 1).max(b.len() - 1) as f64
}

fn position_to_document_index(positions: &[usize], position: usize) -> usize {
    let mut low = 0usize;
    let mut high = positions.len().saturating_sub(1);
    while low < high {
        let mid = (low + high) >> 1;
        if positions[mid] < position {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    low
}

fn context_agreement(
    m: Match,
    query_chars: &[char],
    doc_chars: &[char],
    positions: &[usize],
) -> f64 {
    let index = position_to_document_index(positions, m.position);
    let q_left_start = m.offset.saturating_sub(ANCHOR_CONTEXT);
    let query_left = &query_chars[q_left_start..m.offset.min(query_chars.len())];
    let d_left_start = index.saturating_sub(ANCHOR_CONTEXT);
    let document_left = &doc_chars[d_left_start..index.min(doc_chars.len())];

    let q_right_start = (m.offset + NGRAM_SIZE).min(query_chars.len());
    let q_right_end = (q_right_start + ANCHOR_CONTEXT).min(query_chars.len());
    let query_right = &query_chars[q_right_start..q_right_end];
    let d_right_start = (index + NGRAM_SIZE).min(doc_chars.len());
    let d_right_end = (d_right_start + ANCHOR_CONTEXT).min(doc_chars.len());
    let document_right = &doc_chars[d_right_start..d_right_end];

    bigram_similarity(query_left, document_left).max(bigram_similarity(query_right, document_right))
}

fn cached_agreement(
    cache: &mut HashMap<(usize, usize), f64>,
    m: Match,
    query_chars: &[char],
    doc_chars: &[char],
    positions: &[usize],
) -> f64 {
    *cache
        .entry((m.offset, m.position))
        .or_insert_with(|| context_agreement(m, query_chars, doc_chars, positions))
}

#[allow(clippy::too_many_arguments)] // ST search.ts pickAnchors arity
fn pick_anchors(
    query_chars: &[char],
    inliers: &[Match],
    query_ngram_offsets: &HashMap<String, Vec<usize>>,
    doc_chars: &[char],
    positions: &[usize],
    index: &HashMap<String, Vec<usize>>,
    agreement_cache: &mut HashMap<(usize, usize), f64>,
    min_agreement: Option<f64>,
) -> Vec<Match> {
    let mut anchors = Vec::new();
    let mut i = 0usize;
    while i < query_chars.len() {
        let window_start = i.saturating_sub(ANCHOR_SPACING / 2);
        let window_end = (i + ANCHOR_SPACING / 2).min(query_chars.len());
        let mut candidates: Vec<Match> = inliers
            .iter()
            .copied()
            .filter(|m| window_start <= m.offset && m.offset < window_end)
            .collect();
        candidates.sort_by_key(|m| m.offset.abs_diff(i));
        for c in candidates {
            let Some(gram) = ngram_at(query_chars, c.offset) else {
                continue;
            };
            if index.get(&gram).map(|v| v.len()) != Some(1) {
                continue;
            }
            if query_ngram_offsets.get(&gram).map(|v| v.len()) != Some(1) {
                continue;
            }
            if let Some(min_a) = min_agreement {
                if cached_agreement(agreement_cache, c, query_chars, doc_chars, positions) < min_a {
                    continue;
                }
            }
            anchors.push(c);
            break;
        }
        i += ANCHOR_SPACING;
    }
    anchors
}

#[allow(clippy::too_many_arguments)] // ST search.ts selectAnchors arity
fn select_anchors(
    query_chars: &[char],
    inliers: &[Match],
    query_ngram_offsets: &HashMap<String, Vec<usize>>,
    doc_chars: &[char],
    positions: &[usize],
    index: &HashMap<String, Vec<usize>>,
    slope: f64,
    intercept: f64,
) -> Vec<ForcedAlignAnchor> {
    let mut agreement_cache: HashMap<(usize, usize), f64> = HashMap::new();

    let mut anchors = pick_anchors(
        query_chars,
        inliers,
        query_ngram_offsets,
        doc_chars,
        positions,
        index,
        &mut agreement_cache,
        None,
    );
    if anchors.len() >= ANCHOR_CALIBRATION_MINIMUM {
        let scores: Vec<f64> = anchors
            .iter()
            .map(|&a| cached_agreement(&mut agreement_cache, a, query_chars, doc_chars, positions))
            .collect();
        let threshold = (ANCHOR_AGREEMENT_FRACTION * median(scores)).max(ANCHOR_AGREEMENT_FLOOR);
        anchors = pick_anchors(
            query_chars,
            inliers,
            query_ngram_offsets,
            doc_chars,
            positions,
            index,
            &mut agreement_cache,
            Some(threshold),
        );
    }

    let mut monotonic: Vec<Match> = Vec::new();
    for a in anchors {
        loop {
            let Some(&b) = monotonic.last() else {
                monotonic.push(a);
                break;
            };
            if a.position as isize - b.position as isize >= a.offset as isize - b.offset as isize {
                monotonic.push(a);
                break;
            }
            let a_dist = (a.position as f64 - (intercept + slope * a.offset as f64)).abs();
            let b_dist = (b.position as f64 - (intercept + slope * b.offset as f64)).abs();
            if a_dist > b_dist {
                break;
            }
            monotonic.pop();
        }
    }

    monotonic
        .into_iter()
        .map(|m| ForcedAlignAnchor {
            offset: m.offset,
            position: m.position,
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct SignedMatch {
    offset: i64,
    position: i64,
}

fn fit_local_line(points: &[SignedMatch], fallback_slope: f64) -> (f64, f64) {
    let stride = (points.len() / 2).max(1);
    let mut slopes = Vec::new();
    let mut i = 0usize;
    while i + stride < points.len() {
        let lo = points[i];
        let hi = points[i + stride];
        if ((hi.offset - lo.offset).unsigned_abs() as usize) < EDGE_EXTENSION_PAIR_SEPARATION {
            i += stride;
            continue;
        }
        slopes.push((hi.position - lo.position) as f64 / (hi.offset - lo.offset) as f64);
        i += stride;
    }
    let slope = if slopes.is_empty() {
        fallback_slope
    } else {
        median(slopes)
    };
    let intercept = median(
        points
            .iter()
            .map(|m| m.position as f64 - slope * m.offset as f64)
            .collect(),
    );
    (slope, intercept)
}

fn extend_edge(
    sorted_matches: &[SignedMatch],
    sorted_inliers: &[SignedMatch],
    global_slope: f64,
    inlier_tolerance: f64,
) -> Vec<SignedMatch> {
    let Some(&last_inlier) = sorted_inliers.last() else {
        return Vec::new();
    };
    let mut window: Vec<SignedMatch> = sorted_inliers
        .iter()
        .rev()
        .take(EDGE_EXTENSION_WINDOW)
        .copied()
        .collect();
    window.reverse();
    let mut accepted = Vec::new();
    let mut last_accepted_offset = last_inlier.offset;
    let mut local = fit_local_line(&window, global_slope);

    for &m in sorted_matches {
        if m.offset <= last_inlier.offset {
            continue;
        }
        if ((m.offset - last_accepted_offset).unsigned_abs() as usize) > EDGE_EXTENSION_MAX_GAP {
            break;
        }
        let predicted = local.1 + local.0 * m.offset as f64;
        if (m.position as f64 - predicted).abs() > inlier_tolerance {
            continue;
        }
        accepted.push(m);
        window.push(m);
        if window.len() > EDGE_EXTENSION_WINDOW {
            window.remove(0);
        }
        last_accepted_offset = m.offset;
        local = fit_local_line(&window, global_slope);
    }
    accepted
}

/// Storyteller `extendInlierEdges`: grow RANSAC inliers along local slope at both ends.
fn extend_inlier_edges(
    matches: &[Match],
    sorted_inliers: &[Match],
    slope: f64,
    inlier_tolerance: f64,
) -> Vec<Match> {
    let mut by_offset: Vec<SignedMatch> = matches
        .iter()
        .map(|m| SignedMatch {
            offset: m.offset as i64,
            position: m.position as i64,
        })
        .collect();
    by_offset.sort_by_key(|m| m.offset);

    let sorted: Vec<SignedMatch> = sorted_inliers
        .iter()
        .map(|m| SignedMatch {
            offset: m.offset as i64,
            position: m.position as i64,
        })
        .collect();

    let tail = extend_edge(&by_offset, &sorted, slope, inlier_tolerance);

    let mirror = |m: SignedMatch| SignedMatch {
        offset: -m.offset,
        position: -m.position,
    };
    let mut mirrored_matches: Vec<_> = by_offset.iter().copied().map(mirror).collect();
    mirrored_matches.reverse();
    let mut mirrored_inliers: Vec<_> = sorted.iter().copied().map(mirror).collect();
    mirrored_inliers.reverse();
    let mut head: Vec<_> = extend_edge(
        &mirrored_matches,
        &mirrored_inliers,
        slope,
        inlier_tolerance,
    )
    .into_iter()
    .map(mirror)
    .collect();
    head.reverse();

    head.into_iter()
        .chain(sorted)
        .chain(tail)
        .map(|m| Match {
            offset: m.offset as usize,
            position: m.position as usize,
        })
        .collect()
}

/// Storyteller `computeEvidenceRange`.
pub fn compute_evidence_range(
    offsets: &[usize],
    query_length: usize,
) -> ((usize, usize), (usize, usize)) {
    if offsets.len() < 2 {
        return ((0, query_length), (0, query_length));
    }
    let gaps: Vec<usize> = offsets.windows(2).map(|w| w[1] - w[0]).collect();
    let mut sorted_gaps = gaps.clone();
    sorted_gaps.sort_unstable();
    let p99_idx = ((sorted_gaps.len() as f64) * EVIDENCE_GAP_PERCENTILE).floor() as usize;
    let p99 = sorted_gaps[p99_idx.min(sorted_gaps.len() - 1)];
    let threshold = ((EVIDENCE_GAP_MULTIPLIER * p99 as f64) as usize).max(EVIDENCE_GAP_FLOOR);

    let mut clusters: Vec<(usize, usize)> = Vec::new();
    let mut cluster_start = 0usize;
    for (i, &gap) in gaps.iter().enumerate() {
        if gap <= threshold {
            continue;
        }
        clusters.push((cluster_start, i));
        cluster_start = i + 1;
    }
    clusters.push((cluster_start, offsets.len() - 1));

    let mut first_cluster = 0usize;
    let mut last_cluster = clusters.len() - 1;
    while first_cluster < last_cluster
        && clusters[first_cluster].1 - clusters[first_cluster].0 + 1 < MIN_EDGE_CLUSTER_OFFSETS
    {
        first_cluster += 1;
    }
    while last_cluster > first_cluster
        && clusters[last_cluster].1 - clusters[last_cluster].0 + 1 < MIN_EDGE_CLUSTER_OFFSETS
    {
        last_cluster -= 1;
    }

    let first = offsets[clusters[first_cluster].0];
    let last = offsets[clusters[last_cluster].1] + NGRAM_SIZE;
    let matched = (first, last);
    let evidence = (
        first.saturating_sub(threshold),
        if query_length.saturating_sub(last) > threshold {
            last + threshold
        } else {
            query_length
        },
    );
    (evidence, matched)
}

/// Storyteller `findBoundariesInDocument` (RANSAC + edge extension + evidence ranges).
pub fn find_boundaries_in_document(
    query: &str,
    document: &PositionedDocument,
    options: &BoundaryFitOptions,
) -> Option<CtcBoundaries> {
    let query_chars = char_vec(query);
    let doc_chars = char_vec(&document.text);
    // Storyteller/JS: length = character count, not UTF-8 bytes.
    if doc_chars.is_empty()
        || document.positions.len() != doc_chars.len()
        || query_chars.len() < NGRAM_SIZE
    {
        return None;
    }

    let query_len = query_chars.len();
    let gram_count = query_len.saturating_sub(NGRAM_SIZE - 1);
    let pair_separation = (gram_count / 4).clamp(20, MAX_PAIR_SEPARATION_REQUIREMENT);

    // ST getNgramIndex WeakMap — reuse across chapter/chunk searches.
    let index = document.ngram_index();
    let (matches, query_ngram_offsets) =
        collect_matches(&query_chars, index, &options.excluded_ranges);
    if matches.len() < MIN_INLIERS {
        return None;
    }

    let (slope, intercept, inliers) = fit_line(&matches, pair_separation, options)?;
    let mut sorted_inliers = inliers.clone();
    sorted_inliers.sort_by_key(|m| m.offset);
    let by_offset = extend_inlier_edges(&matches, &sorted_inliers, slope, options.inlier_tolerance);

    let mut inlier_offsets: Vec<usize> = by_offset.iter().map(|m| m.offset).collect();
    inlier_offsets.sort_unstable();
    inlier_offsets.dedup();

    let (evidence_range, matched_range) = compute_evidence_range(&inlier_offsets, query_len);
    // Storyteller gates on byOffset coverage only — no early matchedRange span check.

    let head_n = REFINE_COUNT.min(by_offset.len());
    let head = &by_offset[..head_n];
    let tail = &by_offset[by_offset.len().saturating_sub(head_n)..];

    let extrapolated_start = head
        .iter()
        .map(|m| m.position as f64 - slope * m.offset as f64)
        .fold(f64::INFINITY, f64::min);
    let extrapolated_end = tail
        .iter()
        .map(|m| m.position as f64 + slope * (query_len as f64 - m.offset as f64))
        .fold(f64::NEG_INFINITY, f64::max);

    let start_pad = BOUNDARY_PAD_FRAMES.max(0.25 * (head[0].position as f64 - extrapolated_start));
    let start = extrapolated_start - start_pad;
    let end_pad = BOUNDARY_PAD_FRAMES.max(0.25 * (extrapolated_end - tail.last()?.position as f64));
    let end = extrapolated_end + end_pad;
    if end <= start {
        return None;
    }

    let matched_grams: HashSet<usize> = matches.iter().map(|m| m.offset).collect();
    let confidence = inlier_offsets.len() as f64 / matched_grams.len().max(1) as f64;
    if confidence < MIN_CONFIDENCE {
        return None;
    }

    let first = by_offset.first()?;
    let last = by_offset.last()?;
    let coverage = (last.offset - first.offset + NGRAM_SIZE) as f64 / query_len as f64;
    if coverage < MIN_COVERAGE {
        return None;
    }

    let anchors = select_anchors(
        &query_chars,
        &inliers,
        &query_ngram_offsets,
        &doc_chars,
        &document.positions,
        index,
        slope,
        intercept,
    );

    Some(CtcBoundaries {
        start: start.round().max(0.0) as usize,
        // Storyteller findBoundariesInDocument: Math.round(end) — no +1.
        // Callers treat this as half-open upper bound (ST readFrames).
        end: end.round().max(0.0) as usize,
        confidence,
        anchors,
        evidence_range,
        matched_range,
    })
}

/// Greedy-decode emissions into a searchable document (ST `ctcGreedyDecode`).
pub fn decode_positioned_document(
    emissions: &Emissions,
    vocab: &[(char, i32)],
    blank_id: i32,
) -> Option<PositionedDocument> {
    let (text, frames) = ctc_greedy_decode(emissions, vocab, blank_id);
    // One frame per Unicode scalar (ST/JS char), not per UTF-8 byte.
    if text.is_empty() || frames.len() != text.chars().count() {
        return None;
    }
    Some(PositionedDocument::new(text, frames))
}

/// RANSAC boundaries against an already-decoded document (ST `findCtcBoundaries`
/// after `decodedEmissions` WeakMap hit).
pub fn find_ctc_boundaries_in_document(
    query: &str,
    document: &PositionedDocument,
    num_frames: usize,
    excluded_ranges: &[(usize, usize)],
) -> Option<CtcBoundaries> {
    let inlier_tolerance = if query.len() > 15_000 { 5000.0 } else { 2500.0 };
    let options = BoundaryFitOptions {
        min_slope: 2.0,
        max_slope: 15.0,
        inlier_tolerance,
        excluded_ranges: excluded_ranges.to_vec(),
    };
    let mut result = find_boundaries_in_document(query, document, &options)?;
    // Storyteller findCtcBoundaries: start >= 0, end <= numFrames - 1.
    // Empty window (start == end) is rejected by the aligner, matching ST.
    if num_frames == 0 {
        return None;
    }
    result.start = result.start.min(num_frames.saturating_sub(1));
    result.end = result.end.min(num_frames.saturating_sub(1));
    Some(result)
}

/// Greedy-decode emissions → `PositionedDocument`, then RANSAC boundaries.
///
/// Prefer [`find_ctc_boundaries_in_document`] with a book-level cached decode
/// (ST `decodedEmissions` WeakMap) on the hot Align path.
pub fn find_ctc_boundaries(
    query: &str,
    emissions: &Emissions,
    vocab: &[(char, i32)],
    blank_id: i32,
    excluded_ranges: &[(usize, usize)],
) -> Option<CtcBoundaries> {
    let document = decode_positioned_document(emissions, vocab, blank_id)?;
    find_ctc_boundaries_in_document(query, &document, emissions.frames, excluded_ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positioned_document_hit_yields_boundaries_and_anchors() {
        // Unique alphabet soup so every 10-gram appears once (anchor uniqueness).
        let query: String = (0..50).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
        // Shuffle-ish: reverse every other block so it is not a pure cycle of "abcdefghij".
        let mut query = query;
        query.replace_range(10..20, "qrstuvwxyz");
        query.replace_range(20..30, "mnopqrstuv");
        query.replace_range(30..40, "wxyzabcdef");
        query.replace_range(40..50, "ghijklmnop");
        assert_eq!(query.len(), 50);
        let prefix = "ZZZZZZZZZZ";
        let suffix = "YYYYYYYYYY";
        let text = format!("{prefix}{query}{suffix}");
        let slope = 5usize;
        let positions: Vec<usize> = (0..text.len()).map(|i| i * slope + 100).collect();
        let document = PositionedDocument::new(text, positions);

        let options = BoundaryFitOptions {
            min_slope: 2.0,
            max_slope: 15.0,
            inlier_tolerance: 50.0,
            excluded_ranges: Vec::new(),
        };
        let hit = find_boundaries_in_document(&query, &document, &options)
            .expect("expected CTC boundaries");
        assert!(hit.start < hit.end, "start={} end={}", hit.start, hit.end);
        // Query starts at char offset 10 → frame ~150; pad widens the window.
        assert!(hit.start < 150 + 50);
        assert!(hit.end > 100 + prefix.len() * slope + query.len() * slope / 2);
        assert!(
            !hit.anchors.is_empty(),
            "expected at least one unique-ngram anchor"
        );
        for a in &hit.anchors {
            assert!(a.offset < query.len());
        }
        assert!(hit.matched_range.0 < hit.matched_range.1);
        assert!(hit.evidence_range.0 <= hit.matched_range.0);
        assert!(hit.evidence_range.1 >= hit.matched_range.1);
        // Storyteller SoT: end is Math.round(end), not exclusive +1.
        // A +1 would push end one frame past the extrapolated pad window.
        assert!(
            hit.end > hit.start,
            "non-empty half-open window: start={} end={}",
            hit.start,
            hit.end
        );
    }

    #[test]
    fn multibyte_chars_index_by_scalar_not_utf8_bytes() {
        // "café" = 4 scalars, 5 UTF-8 bytes. positions must be per scalar.
        let text = "abcdefghijcaféabcdefghijabcdefghijabcdefghijabcdefghij";
        let chars: Vec<char> = text.chars().collect();
        assert!(chars.len() < text.len(), "fixture must include multi-byte");
        let positions: Vec<usize> = (0..chars.len()).map(|i| i * 5 + 10).collect();
        let document = PositionedDocument::new(text.to_string(), positions);
        // Query is ASCII prefix long enough for RANSAC — must not reject on byte≠char.
        let query: String = chars[..50].iter().collect();
        let options = BoundaryFitOptions {
            min_slope: 2.0,
            max_slope: 15.0,
            inlier_tolerance: 50.0,
            excluded_ranges: Vec::new(),
        };
        let hit = find_boundaries_in_document(&query, &document, &options);
        assert!(
            hit.is_some(),
            "byte-length gate would reject; char-index must accept"
        );
    }

    #[test]
    fn ngram_index_cached_across_searches_on_same_document() {
        // ST ngramIndexes WeakMap: second search must reuse the same OnceLock entry.
        let text: String = (0..60).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
        let positions: Vec<usize> = (0..text.len()).map(|i| i * 4 + 10).collect();
        let document = PositionedDocument::new(text.clone(), positions);
        assert!(document.ngram_index.get().is_none());
        let q = &text[..40];
        let opts = BoundaryFitOptions {
            min_slope: 2.0,
            max_slope: 15.0,
            inlier_tolerance: 50.0,
            excluded_ranges: Vec::new(),
        };
        let _ = find_boundaries_in_document(q, &document, &opts);
        assert!(document.ngram_index.get().is_some());
        let ptr = document.ngram_index.get().unwrap() as *const _;
        let _ = find_boundaries_in_document(q, &document, &opts);
        assert_eq!(document.ngram_index.get().unwrap() as *const _, ptr);
    }

    #[test]
    fn find_ctc_boundaries_clamps_end_to_last_frame_index() {
        // Tiny synthetic emissions so clamp is observable.
        let frames = 40usize;
        let classes = 3usize;
        let mut values = vec![-10.0f32; frames * classes];
        // Put mass on class 1 so greedy decode yields a stable alphabet.
        for t in 0..frames {
            values[t * classes + 1] = 0.0;
        }
        let emissions = Emissions {
            values,
            frames,
            classes,
        };
        let vocab = vec![('a', 0), ('b', 1), ('c', 2)];
        // Query long enough for RANSAC; may miss — we only assert clamp when hit.
        let query = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"; // 40 b's
        if let Some(hit) = find_ctc_boundaries(query, &emissions, &vocab, 0, &[]) {
            assert!(hit.end < frames, "end={} frames={}", hit.end, frames);
            assert!(hit.start < frames);
            // Empty half-open windows are start==end (ST reject); non-empty must be >.
            if hit.start != hit.end {
                assert!(hit.start < hit.end);
            }
        }
    }

    #[test]
    fn compute_evidence_range_trims_outlier_edge_clusters() {
        // Enough dense gaps that p99 stays near the step size; one huge gap + tiny fringe.
        let mut offsets: Vec<usize> = (0..100).map(|i| i * 10).collect();
        let fringe0 = 100 * 10 + 5_000;
        offsets.push(fringe0);
        offsets.push(fringe0 + 10);
        let query_len = fringe0 + 50;
        let (evidence, matched) = compute_evidence_range(&offsets, query_len);
        assert!(matched.0 < matched.1);
        assert!(evidence.0 <= matched.0);
        assert!(evidence.1 >= matched.1);
        // Fringe past the big gap must not widen matched into the outlier cluster.
        assert!(matched.1 < fringe0, "matched={matched:?} fringe0={fringe0}");
        assert!(matched.1 <= 100 * 10 + NGRAM_SIZE);
    }

    #[test]
    fn extend_inlier_edges_grows_tail_along_slope() {
        let slope = 5.0;
        let inliers: Vec<Match> = (0..15)
            .map(|i| Match {
                offset: i * 10,
                position: 100 + i * 10 * 5,
            })
            .collect();
        // Extra matches beyond last inlier that still sit on the line.
        let mut matches = inliers.clone();
        for i in 15..25 {
            matches.push(Match {
                offset: i * 10,
                position: 100 + i * 10 * 5,
            });
        }
        // One outlier far off the line — must not be accepted.
        matches.push(Match {
            offset: 250,
            position: 100 + 250 * 5 + 50_000,
        });
        let extended = extend_inlier_edges(&matches, &inliers, slope, 5.0);
        assert!(extended.len() > inliers.len());
        assert!(extended.iter().any(|m| m.offset == 240));
        assert!(!extended
            .iter()
            .any(|m| m.offset == 250 && m.position > 10_000));
    }

    /// Live probe: 90s RotK head STEM + Minas Tirith opening (requires data/align-work/probe).
    #[test]
    fn head90_stem_boundaries_for_minas_opening() {
        use crate::align::ctc::ctc_greedy_decode;
        use crate::align::ctc_align::slugify_for_ctc;
        use crate::align::stem::decode_stem_file;

        let stem = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../data/align-work/probe/head90.stem");
        if !stem.exists() {
            eprintln!("skip: missing {}", stem.display());
            return;
        }
        let (header, emissions) = decode_stem_file(&stem).expect("stem");
        let cv: Vec<(char, i32)> = header
            .vocab
            .iter()
            .filter_map(|(k, &id)| {
                let mut cs = k.chars();
                let c = cs.next()?;
                if cs.next().is_some() {
                    None
                } else {
                    Some((c, id))
                }
            })
            .collect();
        let (doc, frames) = ctc_greedy_decode(&emissions, &cv, header.blank_index);
        eprintln!(
            "decode_len={} frames={} head={:?}",
            doc.len(),
            emissions.frames,
            &doc[..doc.len().min(80)]
        );
        assert_eq!(doc.len(), frames.len());

        let sentence = "Pippin looked out from the shelter of Gandalf's cloak. He wondered if he was awake or still sleeping, still in the swift-moving dream in which he had been wrapped so long since the great ride began.";
        let query = slugify_for_ctc(sentence, &header.vocab);
        eprintln!("query={query}");
        let hit = find_ctc_boundaries(&query, &emissions, &cv, header.blank_index, &[])
            .expect("expected boundaries on head90 for Minas opening");
        let start_s = hit.start as f64 * header.timings.seconds_per_frame;
        let end_s = hit.end as f64 * header.timings.seconds_per_frame;
        eprintln!(
            "window_sec={start_s:.2}..{end_s:.2} conf={} matched={:?}",
            hit.confidence, hit.matched_range
        );
        assert!(end_s > 30.0, "window too early: {start_s}..{end_s}");
        assert!(
            start_s < 80.0,
            "window latched past usable audio: {start_s}..{end_s}"
        );
    }

    /// Full RotK STEM: Minas opening must locate early audio (not outro/EOF).
    #[test]
    fn rotk_full_stem_boundaries_for_minas_opening() {
        use crate::align::ctc_align::slugify_for_ctc;
        use crate::align::stem::decode_stem_file;

        let stem = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../data/align-work/probe/rotk_full.stem");
        let prose = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.hermes/cache/scratch/rotk-probe/index_split_006.html.txt");
        // Prefer repo-relative scratch under CARGO_MANIFEST_DIR parent tree.
        let prose = if prose.exists() {
            prose
        } else {
            std::path::PathBuf::from(
                "/home/cayub/.hermes/cache/scratch/rotk-probe/index_split_006.html.txt",
            )
        };
        if !stem.exists() || !prose.exists() {
            eprintln!("skip: missing stem or prose fixture");
            return;
        }
        let (header, emissions) = decode_stem_file(&stem).expect("stem");
        let cv: Vec<(char, i32)> = header
            .vocab
            .iter()
            .filter_map(|(k, &id)| {
                let mut cs = k.chars();
                let c = cs.next()?;
                if cs.next().is_some() {
                    None
                } else {
                    Some((c, id))
                }
            })
            .collect();
        let raw = std::fs::read_to_string(&prose).expect("prose");
        let sentences: Vec<String> = raw.lines().map(str::to_string).collect();
        // Storyteller MAX_SEARCH_LENGTH chunk: first ~40k slug chars.
        let mut query = String::new();
        for s in &sentences {
            let slug = slugify_for_ctc(s, &header.vocab);
            if query.len() + slug.len() > 40_000 && !query.is_empty() {
                break;
            }
            query.push_str(&slug);
        }
        eprintln!("query_len={} frames={}", query.len(), emissions.frames);
        let hit = find_ctc_boundaries(&query, &emissions, &cv, header.blank_index, &[])
            .expect("expected Minas boundaries on full RotK STEM");
        let start_s = hit.start as f64 * header.timings.seconds_per_frame;
        let end_s = hit.end as f64 * header.timings.seconds_per_frame;
        eprintln!(
            "window_sec={start_s:.1}..{end_s:.1} conf={}",
            hit.confidence
        );
        assert!(
            start_s < 1000.0,
            "Minas latched to EOF/late audio: {start_s}..{end_s}"
        );
        assert!(end_s > 100.0, "window too short: {start_s}..{end_s}");
    }
}
