//! Token edit-alignment (Storyteller `errorAlign`).
//!
//! Ports ST `errorAlignBeamSearch` (beamSearch.ts) onto Shelf's `-`-delimited
//! slug tokens. Full ST wraps tokens as `<>` chars and may use native C++; we
//! keep atomic slug tokens for production align (tokenizer/normalizer residual).
//! [`get_unambiguous_token_span_matches`] ports the `<>`-embed graph helper;
//! beam [`backtrace_and_unambiguous`] stays frequency-based for collapse.
//!
//! `error_align_with_narrowing` recurses like ST `getSentenceRanges.ts`.

use std::collections::{HashMap, HashSet};

use crate::align::match_score::levenshtein;

const DEFAULT_BEAM_SIZE: usize = 100;
const SORT_ID_BASE: u64 = 146_527;

/// ST `errorAlign/utils.ts` — wrap each token for char-level span graph.
pub const START_DELIMITER: char = '<';
/// ST `errorAlign/utils.ts` — end of an embedded token span.
pub const END_DELIMITER: char = '>';

type IndexSet = HashSet<(i32, i32)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpType {
    Match,
    Insert,
    Delete,
    Substitute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alignment {
    pub op: OpType,
    pub ref_tok: Option<String>,
    pub hyp_tok: Option<String>,
}

fn tokens(slug: &str) -> Vec<&str> {
    if slug.is_empty() {
        Vec::new()
    } else {
        slug.split('-').filter(|t| !t.is_empty()).collect()
    }
}

/// Align reference slug to hypothesis slug (both `-`-delimited).
///
/// Storyteller default: word-level pass pins unambiguous MATCH tokens, then
/// beam-searches each gap (`alignWithWordLevelPass`).
pub fn error_align(ref_slug: &str, hyp_slug: &str) -> Vec<Alignment> {
    error_align_with_options(ref_slug, hyp_slug, true)
}

/// `word_level_pass` mirrors Storyteller `errorAlign(..., wordLevelPass)`.
pub fn error_align_with_options(
    ref_slug: &str,
    hyp_slug: &str,
    word_level_pass: bool,
) -> Vec<Alignment> {
    error_align_with_beam(ref_slug, hyp_slug, word_level_pass, DEFAULT_BEAM_SIZE)
}

fn error_align_with_beam(
    ref_slug: &str,
    hyp_slug: &str,
    word_level_pass: bool,
    beam_size: usize,
) -> Vec<Alignment> {
    let ref_t = tokens(ref_slug);
    let hyp_t = tokens(hyp_slug);
    if ref_t.is_empty() && hyp_t.is_empty() {
        return Vec::new();
    }
    if ref_t == hyp_t {
        return identical_alignments(&ref_t, &hyp_t);
    }
    if !word_level_pass {
        return align_beam_search(&ref_t, &hyp_t, beam_size);
    }
    align_with_word_level_pass(&ref_t, &hyp_t, beam_size)
}

fn identical_alignments(ref_t: &[&str], hyp_t: &[&str]) -> Vec<Alignment> {
    ref_t
        .iter()
        .zip(hyp_t.iter())
        .map(|(r, h)| Alignment {
            op: OpType::Match,
            ref_tok: Some((*r).to_string()),
            hyp_tok: Some((*h).to_string()),
        })
        .collect()
}

fn token_freq(tokens: &[&str]) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    for t in tokens {
        *m.entry((*t).to_string()).or_insert(0) += 1;
    }
    m
}

/// Storyteller `alignWithWordLevelPass` — unambiguous MATCH anchors, beam on gaps.
fn align_with_word_level_pass(ref_t: &[&str], hyp_t: &[&str], beam_size: usize) -> Vec<Alignment> {
    let mut anchors = get_unambiguous_node_matches(ref_t, hyp_t);
    // Artificial terminal match simplifies subspan extraction (ST).
    anchors.push((hyp_t.len(), ref_t.len()));

    if anchors.len() <= 1 {
        return align_beam_search(ref_t, hyp_t, beam_size);
    }

    let mut out = Vec::new();
    let mut hyp_start = 0usize;
    let mut ref_start = 0usize;
    for (i, &(hyp_end, ref_end)) in anchors.iter().enumerate() {
        let ref_empty = ref_start == ref_end;
        let hyp_empty = hyp_start == hyp_end;
        if !ref_empty && !hyp_empty {
            out.extend(align_beam_search(
                &ref_t[ref_start..ref_end],
                &hyp_t[hyp_start..hyp_end],
                beam_size,
            ));
        } else if ref_empty && !hyp_empty {
            for t in &hyp_t[hyp_start..hyp_end] {
                out.push(Alignment {
                    op: OpType::Insert,
                    ref_tok: None,
                    hyp_tok: Some((*t).to_string()),
                });
            }
        } else if hyp_empty && !ref_empty {
            for t in &ref_t[ref_start..ref_end] {
                out.push(Alignment {
                    op: OpType::Delete,
                    ref_tok: Some((*t).to_string()),
                    hyp_tok: None,
                });
            }
        }

        if i + 1 < anchors.len() {
            out.push(Alignment {
                op: OpType::Match,
                ref_tok: Some(ref_t[ref_end].to_string()),
                hyp_tok: Some(hyp_t[hyp_end].to_string()),
            });
            ref_start = ref_end + 1;
            hyp_start = hyp_end + 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// BacktraceGraph (ST backtraceGraph.ts) — multi-path unambiguous MATCH anchors
// ---------------------------------------------------------------------------

const BT_MATCH: u8 = 1;
const BT_INSERT: u8 = 2;
const BT_DELETE: u8 = 4;
const BT_SUBSTITUTE: u8 = 8;
const BT_REF_OPS: u8 = BT_MATCH | BT_SUBSTITUTE | BT_DELETE;
const BT_HYP_OPS: u8 = BT_MATCH | BT_SUBSTITUTE | BT_INSERT;

/// ST `computeLevenshteinDistanceMatrix(..., backtrace=true)` — all optimal ops per cell.
fn levenshtein_multipath_backtrace(ref_t: &[&str], hyp_t: &[&str]) -> Vec<Vec<u8>> {
    let n = hyp_t.len();
    let m = ref_t.len();
    let mut score = vec![vec![0i32; m + 1]; n + 1];
    let mut bt = vec![vec![0u8; m + 1]; n + 1];
    bt[0][0] = BT_MATCH;
    for j in 1..=m {
        score[0][j] = j as i32;
        bt[0][j] = BT_DELETE;
    }
    for i in 1..=n {
        score[i][0] = i as i32;
        bt[i][0] = BT_INSERT;
    }
    for j in 1..=m {
        for i in 1..=n {
            let diag_cost = if hyp_t[i - 1] == ref_t[j - 1] { 0 } else { 1 };
            let ins = score[i - 1][j] + 1;
            let del = score[i][j - 1] + 1;
            let diag = score[i - 1][j - 1] + diag_cost;
            let best = ins.min(del).min(diag);
            score[i][j] = best;
            let mut ops = 0u8;
            // Order matches ST posOps (MATCH, INSERT, DELETE, SUBSTITUTE).
            if diag == best && diag_cost == 0 {
                ops |= BT_MATCH;
            }
            if ins == best {
                ops |= BT_INSERT;
            }
            if del == best {
                ops |= BT_DELETE;
            }
            if diag == best && diag_cost > 0 {
                ops |= BT_SUBSTITUTE;
            }
            bt[i][j] = ops;
        }
    }
    bt
}

/// Optimal-path DAG: matrix index → parent-op bitflags (ST `BacktraceGraph.nodes`).
fn backtrace_graph_parents(bt: &[Vec<u8>]) -> HashMap<(usize, usize), u8> {
    let hyp_max = bt.len() - 1;
    let ref_max = bt[0].len() - 1;
    let mut parents: HashMap<(usize, usize), u8> = HashMap::new();
    let mut stack = vec![(hyp_max, ref_max)];
    let mut seen = HashSet::new();
    seen.insert((hyp_max, ref_max));

    while let Some((hi, ri)) = stack.pop() {
        if hi == 0 && ri == 0 {
            parents.entry((0, 0)).or_insert(0);
            continue;
        }
        let ops = bt[hi][ri];
        let mut pflags = 0u8;
        let mut cand: Vec<(usize, usize)> = Vec::new();
        if ops & BT_MATCH != 0 {
            pflags |= BT_MATCH;
            cand.push((hi - 1, ri - 1));
        }
        if ops & BT_SUBSTITUTE != 0 {
            pflags |= BT_SUBSTITUTE;
            cand.push((hi - 1, ri - 1));
        }
        if ops & BT_INSERT != 0 {
            pflags |= BT_INSERT;
            cand.push((hi - 1, ri));
        }
        if ops & BT_DELETE != 0 {
            pflags |= BT_DELETE;
            cand.push((hi, ri - 1));
        }
        parents.insert((hi, ri), pflags);
        for c in cand {
            if seen.insert(c) {
                stack.push(c);
            }
        }
    }
    parents
}

/// ST `BacktraceGraph.getUnambiguousNodeMatches` — offset (hyp, ref) token indices.
fn get_unambiguous_node_matches(ref_t: &[&str], hyp_t: &[&str]) -> Vec<(usize, usize)> {
    if ref_t.is_empty() || hyp_t.is_empty() {
        return Vec::new();
    }
    let bt = levenshtein_multipath_backtrace(ref_t, hyp_t);
    let nodes = backtrace_graph_parents(&bt);

    let mut match_indices: HashSet<(usize, usize)> = HashSet::new();
    let mut ref_count: HashMap<usize, usize> = HashMap::new();
    let mut hyp_count: HashMap<usize, usize> = HashMap::new();

    for (&(hi, ri), &pflags) in &nodes {
        // Sole parent op is MATCH.
        if pflags == BT_MATCH {
            match_indices.insert((hi, ri));
        }
        if pflags & BT_REF_OPS != 0 {
            *ref_count.entry(ri).or_insert(0) += 1;
        }
        if pflags & BT_HYP_OPS != 0 {
            *hyp_count.entry(hi).or_insert(0) += 1;
        }
    }

    let mut out: Vec<(usize, usize)> = match_indices
        .into_iter()
        .filter(|&(hi, ri)| {
            ref_count.get(&ri).copied().unwrap_or(0) == 1
                && hyp_count.get(&hi).copied().unwrap_or(0) == 1
        })
        .map(|(hi, ri)| (hi - 1, ri - 1)) // offsetIndex
        .collect();
    out.sort_by_key(|&(_h, r)| r);
    out
}

/// ST `embedTokens` — `<tok>` join for char-level span matching.
fn embed_tokens(tokens: &[&str]) -> String {
    let mut out = String::new();
    for t in tokens {
        out.push(START_DELIMITER);
        out.push_str(t);
        out.push(END_DELIMITER);
    }
    out
}

/// ST `BacktraceGraph.getUnambiguousTokenSpanMatches` — offset end-nodes of
/// unambiguous `<...>` MATCH spans on the multi-path DAG.
///
/// `ref_embedded` / `hyp_embedded` are already `embedTokens` strings (chars).
fn get_unambiguous_token_span_matches(
    ref_embedded: &str,
    hyp_embedded: &str,
) -> Vec<(usize, usize)> {
    let ref_owned: Vec<String> = ref_embedded.chars().map(|c| c.to_string()).collect();
    let hyp_owned: Vec<String> = hyp_embedded.chars().map(|c| c.to_string()).collect();
    let ref_t: Vec<&str> = ref_owned.iter().map(|s| s.as_str()).collect();
    let hyp_t: Vec<&str> = hyp_owned.iter().map(|s| s.as_str()).collect();
    if ref_t.is_empty() || hyp_t.is_empty() {
        return Vec::new();
    }

    let bt = levenshtein_multipath_backtrace(&ref_t, &hyp_t);
    let nodes = backtrace_graph_parents(&bt);

    // ST: `ref = "_" + ref` so matrix index aligns with pad[ri].
    let mut padded: Vec<char> = Vec::with_capacity(ref_embedded.chars().count() + 1);
    padded.push('_');
    padded.extend(ref_embedded.chars());

    let mut mono_match_end: HashSet<(usize, usize)> = HashSet::new();
    let mut ref_indexes: HashMap<usize, usize> = HashMap::new();
    let mut hyp_indexes: HashMap<usize, usize> = HashMap::new();

    for (&(hi, ri), &pflags) in &nodes {
        // ST: parents.has("MATCH") — not necessarily sole parent.
        if pflags & BT_MATCH == 0 {
            continue;
        }
        if padded.get(ri).copied() != Some(START_DELIMITER) {
            continue;
        }
        let mut cur_ri = ri + 1;
        let mut cur_hi = hi + 1;
        loop {
            if !nodes.contains_key(&(cur_hi, cur_ri)) {
                break;
            }
            if padded.get(cur_ri).copied() == Some(END_DELIMITER) {
                mono_match_end.insert((cur_hi, cur_ri));
                *ref_indexes.entry(cur_ri).or_insert(0) += 1;
                *hyp_indexes.entry(cur_hi).or_insert(0) += 1;
                break;
            }
            cur_ri += 1;
            cur_hi += 1;
        }
    }

    let mut out: Vec<(usize, usize)> = mono_match_end
        .into_iter()
        .filter(|&(h, r)| {
            hyp_indexes.get(&h).copied().unwrap_or(0) == 1
                && ref_indexes.get(&r).copied().unwrap_or(0) == 1
        })
        .map(|(h, r)| (h.saturating_sub(1), r.saturating_sub(1)))
        .collect();
    out.sort_by_key(|&(_h, r)| r);
    out
}

// ---------------------------------------------------------------------------
// Beam search (ST beamSearch.ts on atomic slug tokens)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct BeamPath {
    ref_index: i32,
    hyp_index: i32,
    last_hyp_index: i32,
    last_ref_index: i32,
    closed_cost: i32,
    open_cost: i32,
    at_unambiguous_match_node: bool,
    /// Completed ops (token-level segments), newest at end.
    ops: Vec<Alignment>,
    sort_id: u64,
}

impl BeamPath {
    fn new() -> Self {
        Self {
            ref_index: -1,
            hyp_index: -1,
            last_hyp_index: -1,
            last_ref_index: -1,
            closed_cost: 0,
            open_cost: 0,
            at_unambiguous_match_node: false,
            ops: Vec::new(),
            sort_id: 0,
        }
    }

    fn prune_id(&self) -> (i32, i32, i32, i32) {
        (
            self.hyp_index,
            self.ref_index,
            self.last_hyp_index,
            self.last_ref_index,
        )
    }

    fn cost(&self) -> i32 {
        let is_sub = is_substitution(
            self.hyp_index,
            self.ref_index,
            self.last_hyp_index,
            self.last_ref_index,
        );
        self.closed_cost + self.open_cost + if is_sub { self.open_cost } else { 0 }
    }

    fn norm_cost(&self) -> f64 {
        let cost = self.cost();
        if cost == 0 {
            return 0.0;
        }
        // +3 avoids div-by-zero at root (-1,-1), matching ST.
        cost as f64 / (self.ref_index + self.hyp_index + 3) as f64
    }

    fn at_end(&self, ref_max: i32, hyp_max: i32) -> bool {
        self.hyp_index == hyp_max && self.ref_index == ref_max
    }

    fn update_sort_id(&mut self, t: u64) {
        self.sort_id = self.sort_id.wrapping_mul(SORT_ID_BASE).wrapping_add(t);
    }
}

fn is_substitution(hyp_index: i32, ref_index: i32, last_hyp: i32, last_ref: i32) -> bool {
    !(ref_index == last_ref || hyp_index == last_hyp)
}

fn transition(parent: &BeamPath, ref_step: i32, hyp_step: i32) -> BeamPath {
    let mut child = parent.clone();
    child.ref_index = parent.ref_index + ref_step;
    child.hyp_index = parent.hyp_index + hyp_step;
    child.at_unambiguous_match_node = false;
    // sortId step: refStep+refStep+hypStep (ST transitionToChildNode)
    child.update_sort_id((ref_step + ref_step + hyp_step) as u64);
    child
}

fn reset_segment(path: &mut BeamPath, hyp_index: i32, ref_index: i32) {
    path.closed_cost += path.open_cost;
    if is_substitution(
        hyp_index,
        ref_index,
        path.last_hyp_index,
        path.last_ref_index,
    ) {
        path.closed_cost += path.open_cost;
    }
    path.last_hyp_index = hyp_index;
    path.last_ref_index = ref_index;
    path.open_cost = 0;
}

fn end_segment(mut path: BeamPath, op: Alignment, unambiguous: &IndexSet) -> BeamPath {
    let is_match_segment = path.open_cost == 0;
    path.at_unambiguous_match_node =
        is_match_segment && unambiguous.contains(&(path.hyp_index, path.ref_index));
    path.ops.push(op);
    let hyp = path.hyp_index;
    let r = path.ref_index;
    reset_segment(&mut path, hyp, r);
    path
}

struct BeamSrc<'a> {
    ref_t: &'a [&'a str],
    hyp_t: &'a [&'a str],
    ref_max: i32,
    hyp_max: i32,
    backtrace: IndexSet,
    unambiguous: IndexSet,
}

fn build_beam_src<'a>(ref_t: &'a [&'a str], hyp_t: &'a [&'a str]) -> BeamSrc<'a> {
    let (backtrace, unambiguous) = backtrace_and_unambiguous(ref_t, hyp_t);
    BeamSrc {
        ref_t,
        hyp_t,
        ref_max: ref_t.len() as i32 - 1,
        hyp_max: hyp_t.len() as i32 - 1,
        backtrace,
        unambiguous,
    }
}

/// Single-path DP backtrace nodes + frequency-unique MATCH indices (offset 0-based).
fn backtrace_and_unambiguous(ref_t: &[&str], hyp_t: &[&str]) -> (IndexSet, IndexSet) {
    let n = hyp_t.len();
    let m = ref_t.len();
    let mut score = vec![vec![0i32; m + 1]; n + 1];
    let mut back = vec![vec![0u8; m + 1]; n + 1];
    for j in 1..=m {
        score[0][j] = j as i32;
        back[0][j] = 2;
    }
    for i in 1..=n {
        score[i][0] = i as i32;
        back[i][0] = 1;
    }
    for i in 1..=n {
        for j in 1..=m {
            let diag_cost = if hyp_t[i - 1] == ref_t[j - 1] { 0 } else { 1 };
            let diag = score[i - 1][j - 1] + diag_cost;
            let ins = score[i - 1][j] + 1;
            let del = score[i][j - 1] + 1;
            let best = diag.min(ins).min(del);
            score[i][j] = best;
            back[i][j] = if best == diag {
                0
            } else if best == ins {
                1
            } else {
                2
            };
        }
    }

    let mut nodes = IndexSet::new();
    // Root offset (-1,-1) always on path.
    nodes.insert((-1, -1));
    let mut i = n;
    let mut j = m;
    let mut matches: Vec<(usize, usize)> = Vec::new();
    while i > 0 || j > 0 {
        // offsetIndex style: after consuming tokens i,j the node is (i-1, j-1)
        nodes.insert((i as i32 - 1, j as i32 - 1));
        if i == 0 {
            j -= 1;
            continue;
        }
        if j == 0 {
            i -= 1;
            continue;
        }
        match back[i][j] {
            0 => {
                if hyp_t[i - 1] == ref_t[j - 1] {
                    matches.push((i - 1, j - 1));
                }
                i -= 1;
                j -= 1;
            }
            1 => i -= 1,
            _ => j -= 1,
        }
    }

    let ref_freq = token_freq(ref_t);
    let hyp_freq = token_freq(hyp_t);
    let mut unambiguous = IndexSet::new();
    for (hi, ri) in matches {
        let tok = ref_t[ri];
        if ref_freq.get(tok).copied().unwrap_or(0) == 1
            && hyp_freq.get(tok).copied().unwrap_or(0) == 1
        {
            unambiguous.insert((hi as i32, ri as i32));
        }
    }
    (nodes, unambiguous)
}

fn expand<'a>(parent: &BeamPath, src: &BeamSrc<'a>) -> Vec<BeamPath> {
    let mut out = Vec::with_capacity(3);
    if let Some(p) = add_delete(parent, src) {
        out.push(p);
    }
    if let Some(p) = add_insert(parent, src) {
        out.push(p);
    }
    if let Some(p) = add_sub_or_match(parent, src) {
        out.push(p);
    }
    out
}

fn add_sub_or_match<'a>(parent: &BeamPath, src: &BeamSrc<'a>) -> Option<BeamPath> {
    if parent.ref_index >= src.ref_max || parent.hyp_index >= src.hyp_max {
        return None;
    }
    let mut child = transition(parent, 1, 1);
    let ri = child.ref_index as usize;
    let hi = child.hyp_index as usize;
    let is_match = src.ref_t[ri] == src.hyp_t[hi];
    if !is_match {
        let parent_idx = (parent.hyp_index, parent.ref_index);
        let is_backtrace = src.backtrace.contains(&parent_idx);
        // Atomic slug tokens: treat as same letter-type (cost 2), +1 off-backtrace.
        child.open_cost += 2;
        child.open_cost += if is_backtrace { 0 } else { 1 };
    }
    let op = Alignment {
        op: if is_match {
            OpType::Match
        } else {
            OpType::Substitute
        },
        ref_tok: Some(src.ref_t[ri].to_string()),
        hyp_tok: Some(src.hyp_t[hi].to_string()),
    };
    Some(end_segment(child, op, &src.unambiguous))
}

fn add_insert<'a>(parent: &BeamPath, src: &BeamSrc<'a>) -> Option<BeamPath> {
    // ST addInsert: refStep=1, hypStep=0 → consumes REF only → DELETE op
    if parent.ref_index >= src.ref_max {
        return None;
    }
    let mut child = transition(parent, 1, 0);
    let ri = child.ref_index as usize;
    let parent_idx = (parent.hyp_index, parent.ref_index);
    let is_backtrace = src.backtrace.contains(&parent_idx);
    child.open_cost += 2;
    child.open_cost += if is_backtrace { 0 } else { 1 };
    let op = Alignment {
        op: OpType::Delete,
        ref_tok: Some(src.ref_t[ri].to_string()),
        hyp_tok: None,
    };
    Some(end_segment(child, op, &src.unambiguous))
}

fn add_delete<'a>(parent: &BeamPath, src: &BeamSrc<'a>) -> Option<BeamPath> {
    if parent.hyp_index >= src.hyp_max {
        return None;
    }
    let mut child = transition(parent, 0, 1);
    let hi = child.hyp_index as usize;
    let parent_idx = (parent.hyp_index, parent.ref_index);
    let is_backtrace = src.backtrace.contains(&parent_idx);
    child.open_cost += 2;
    child.open_cost += if is_backtrace { 0 } else { 1 };
    let op = Alignment {
        op: OpType::Insert, // hyp advances only → INSERT
        ref_tok: None,
        hyp_tok: Some(src.hyp_t[hi].to_string()),
    };
    Some(end_segment(child, op, &src.unambiguous))
}

/// ST `errorAlignBeamSearch` — beam over token Path expand/prune/normCost/sortId.
fn align_beam_search(ref_t: &[&str], hyp_t: &[&str], beam_size: usize) -> Vec<Alignment> {
    if ref_t.is_empty() && hyp_t.is_empty() {
        return Vec::new();
    }
    if ref_t == hyp_t {
        return identical_alignments(ref_t, hyp_t);
    }
    // One side empty → pure insert/delete (no search).
    if ref_t.is_empty() {
        return hyp_t
            .iter()
            .map(|t| Alignment {
                op: OpType::Insert,
                ref_tok: None,
                hyp_tok: Some((*t).to_string()),
            })
            .collect();
    }
    if hyp_t.is_empty() {
        return ref_t
            .iter()
            .map(|t| Alignment {
                op: OpType::Delete,
                ref_tok: Some((*t).to_string()),
                hyp_tok: None,
            })
            .collect();
    }

    let src = build_beam_src(ref_t, hyp_t);
    let start = BeamPath::new();
    let mut beam = vec![start];
    let mut prune_map: HashMap<(i32, i32, i32, i32), i32> = HashMap::new();
    let mut ended: Vec<BeamPath> = Vec::new();

    while !beam.is_empty() {
        let mut new_beam: HashMap<(i32, i32, i32, i32), BeamPath> = HashMap::new();

        for path in &beam {
            if path.at_end(src.ref_max, src.hyp_max) {
                ended.push(path.clone());
                continue;
            }
            for new_path in expand(path, &src) {
                let new_cost = new_path.cost();
                let pid = new_path.prune_id();
                if let Some(&prev) = prune_map.get(&pid) {
                    if new_cost > prev {
                        continue;
                    }
                }
                prune_map.insert(pid, new_cost);
                match new_beam.get(&pid) {
                    Some(existing) if new_cost >= existing.cost() => {}
                    _ => {
                        new_beam.insert(pid, new_path);
                    }
                }
            }
        }

        let mut paths: Vec<BeamPath> = new_beam.into_values().collect();
        paths.sort_by(|a, b| {
            let nc = a
                .norm_cost()
                .partial_cmp(&b.norm_cost())
                .unwrap_or(std::cmp::Ordering::Equal);
            if nc != std::cmp::Ordering::Equal {
                return nc;
            }
            a.sort_id.cmp(&b.sort_id)
        });
        // Keep at most beam_size; collapse to 1 at unambiguous MATCH.
        if paths.first().is_some_and(|p| p.at_unambiguous_match_node) {
            paths.truncate(1);
            prune_map.clear();
        } else {
            paths.truncate(beam_size);
        }
        beam = paths;
    }

    ended.sort_by(|a, b| {
        let c = a.cost().cmp(&b.cost());
        if c != std::cmp::Ordering::Equal {
            return c;
        }
        a.sort_id.cmp(&b.sort_id)
    });
    ended.into_iter().next().map(|p| p.ops).unwrap_or_default()
}

/// Storyteller `getAlignmentsForSentence` — consume ops for one sentence slug.
/// Returns (ops used for this sentence, score). Score ≤ 0 means reject.
pub fn alignments_for_sentence(sentence: &str, alignments: &[Alignment]) -> (usize, f64) {
    if sentence.is_empty() {
        return (0, -1.0);
    }
    let mut score = (sentence.chars().count() as i64) / 2;
    let mut sentence_index = 0usize;
    let sentence_len = sentence.chars().count();
    let mut used = 0usize;
    let mut saw_match = false;

    for a in alignments {
        if sentence_index >= sentence_len {
            break;
        }
        used += 1;
        if a.op != OpType::Insert {
            let ref_len = a.ref_tok.as_ref().map(|s| s.chars().count()).unwrap_or(0);
            sentence_index += ref_len + usize::from(sentence_index > 0);
        }
        match a.op {
            OpType::Match => {
                saw_match = true;
            }
            OpType::Delete => {
                let n = a.ref_tok.as_ref().map(|s| s.len()).unwrap_or(0);
                score -= n as i64 + 1;
            }
            OpType::Insert if sentence_index > 0 => {
                let n = a.hyp_tok.as_ref().map(|s| s.len()).unwrap_or(0);
                score -= n as i64 + 1;
            }
            OpType::Substitute => {
                let r = a.ref_tok.as_deref().unwrap_or("");
                let h = a.hyp_tok.as_deref().unwrap_or("");
                score -= levenshtein(r, h) as i64;
            }
            OpType::Insert => {}
        }
    }

    let final_score = if saw_match {
        score as f64 / sentence_len as f64
    } else {
        -1.0
    };
    (used, final_score)
}

/// Storyteller `errorAlignWithNarrowing` — drop leading/trailing score≤0 sentences
/// and recurse on the middle slice (absolute offsets composed).
pub fn error_align_with_narrowing(
    ref_sentences: &[String],
    hyp: &str,
    narrow_start: bool,
    narrow_end: bool,
) -> (Vec<Alignment>, usize, usize) {
    if ref_sentences.is_empty() {
        return (Vec::new(), 0, 0);
    }
    let first = error_align(&ref_sentences.join("-"), hyp);

    let mut first_good = 0usize;
    if narrow_start {
        let mut align_idx = 0usize;
        for sentence in ref_sentences {
            let (used, score) = alignments_for_sentence(sentence, &first[align_idx..]);
            align_idx += used;
            if sentence.is_empty() || score <= 0.0 {
                first_good += 1;
            } else {
                break;
            }
        }
    }

    let mut last_good = ref_sentences.len();
    if narrow_end {
        // ST reverses ref tokens only (not hyp) when walking from the end.
        let reversed_first: Vec<Alignment> = first
            .iter()
            .rev()
            .map(|a| Alignment {
                op: a.op,
                ref_tok: a.ref_tok.as_ref().map(|s| s.chars().rev().collect()),
                hyp_tok: a.hyp_tok.clone(),
            })
            .collect();
        let mut bad_from_end = 0usize;
        let mut align_idx = 0usize;
        for sentence in ref_sentences.iter().rev() {
            let rev_sent: String = sentence.chars().rev().collect();
            let (used, score) = alignments_for_sentence(&rev_sent, &reversed_first[align_idx..]);
            align_idx += used;
            if sentence.is_empty() || score <= 0.0 {
                bad_from_end += 1;
            } else {
                break;
            }
        }
        last_good = ref_sentences.len() - bad_from_end;
    }

    // Already tight — ST: firstGood <= 1 && lastGood >= len - 2
    if first_good <= 1 && last_good + 2 >= ref_sentences.len() {
        return (first, 0, ref_sentences.len());
    }

    let start = first_good.saturating_sub(1);
    let end = (last_good + 1).min(ref_sentences.len());
    if start >= end {
        return (first, 0, ref_sentences.len());
    }
    let slice = &ref_sentences[start..end];
    let (alignments, inner_start, inner_end) =
        error_align_with_narrowing(slice, hyp, narrow_start, narrow_end);
    (alignments, start + inner_start, start + inner_end)
}

/// Hyp character length consumed by alignments (joined with `-`, ST style).
pub fn hyp_span_chars(alignments: &[Alignment]) -> usize {
    let parts: Vec<&str> = alignments
        .iter()
        .filter(|a| a.op != OpType::Delete)
        .filter_map(|a| a.hyp_tok.as_deref())
        .collect();
    if parts.is_empty() {
        0
    } else {
        parts.iter().map(|p| p.len()).sum::<usize>() + parts.len().saturating_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_slugs_are_all_match() {
        let a = error_align("hello-world", "hello-world");
        assert!(a.iter().all(|x| x.op == OpType::Match));
        assert_eq!(a.len(), 2);
    }

    #[test]
    fn full_match_four_tokens() {
        // ST errorAlign.test "full match" shape on slug tokens.
        let al = error_align("this-is-a-test", "this-is-a-test");
        assert_eq!(
            al.iter().map(|a| a.op).collect::<Vec<_>>(),
            vec![OpType::Match; 4]
        );
    }

    #[test]
    fn insert_match_substitute_delete_shape() {
        // ST: Inserted this is a contribution test. / This is a substitution test deleted.
        // Slug form of the same op sequence.
        let ref_s = "this-is-a-substitution-test-deleted";
        let hyp_s = "inserted-this-is-a-contribution-test";
        let al = error_align(ref_s, hyp_s);
        let ops: Vec<_> = al.iter().map(|a| a.op).collect();
        assert_eq!(
            ops,
            vec![
                OpType::Insert,
                OpType::Match,
                OpType::Match,
                OpType::Match,
                OpType::Substitute,
                OpType::Match,
                OpType::Delete,
            ],
            "{ops:?}"
        );
    }

    #[test]
    fn beam_without_word_pass_aligns_sub_insert() {
        let al = error_align_with_options("alpha-foo-bar", "alpha-foo-baz", false);
        assert!(al.iter().any(|a| a.op == OpType::Match));
        assert!(al.iter().any(|a| {
            a.op == OpType::Substitute
                || (a.op == OpType::Delete && a.ref_tok.as_deref() == Some("bar"))
                || (a.op == OpType::Insert && a.hyp_tok.as_deref() == Some("baz"))
        }));
    }

    #[test]
    fn sentence_score_positive_on_good_align() {
        let al = error_align("hello-world-goodbye", "hello-world-goodbye");
        let (used, score) = alignments_for_sentence("hello-world-goodbye", &al);
        assert_eq!(used, 3);
        assert!(score > 0.0, "{score}");
    }

    #[test]
    fn sentence_score_rejects_unrelated() {
        let al = error_align("hello-world", "zebra-walrus-alpha-beta");
        let (_used, score) = alignments_for_sentence("hello-world", &al);
        assert!(score <= 0.0, "{score}");
    }

    #[test]
    fn word_level_pass_pins_unique_tokens() {
        let al = error_align("alpha-foo-bar-omega", "alpha-foo-baz-omega");
        assert!(al
            .iter()
            .any(|a| { a.op == OpType::Match && a.ref_tok.as_deref() == Some("alpha") }));
        assert!(al
            .iter()
            .any(|a| { a.op == OpType::Match && a.ref_tok.as_deref() == Some("omega") }));
        let plain = error_align_with_options("alpha-foo-bar-omega", "alpha-foo-baz-omega", false);
        assert!(!plain.is_empty());
    }

    #[test]
    fn unambiguous_node_matches_pins_repeated_token_indices() {
        // Frequency-unique would skip both "alpha" (count=2); multi-path graph pins each.
        let ref_t = ["alpha", "foo", "alpha"];
        let hyp_t = ["alpha", "bar", "alpha"];
        assert_eq!(
            get_unambiguous_node_matches(&ref_t, &hyp_t),
            vec![(0, 0), (2, 2)]
        );
    }

    #[test]
    fn word_level_pass_anchors_repeated_tokens_via_graph() {
        // Without graph anchors, frequency-unique finds none and beams the whole span.
        let al = error_align("alpha-foo-alpha", "alpha-bar-alpha");
        let ops: Vec<_> = al.iter().map(|a| a.op).collect();
        assert_eq!(
            ops,
            vec![OpType::Match, OpType::Substitute, OpType::Match],
            "{ops:?}"
        );
    }

    #[test]
    fn unambiguous_rejects_freq_unique_overpin() {
        // ST multi-path: hyp a@2 ↔ ref a@0 is frequency-unique but not graph-unambiguous.
        let ref_t = ["a", "b"];
        let hyp_t = ["b", "b", "a", "c"];
        let matches = get_unambiguous_node_matches(&ref_t, &hyp_t);
        assert!(
            !matches.contains(&(2, 0)),
            "expected no over-pin at (2,0), got {matches:?}"
        );
    }

    #[test]
    fn embed_tokens_wraps_with_delimiters() {
        assert_eq!(embed_tokens(&["hello", "world"]), "<hello><world>");
        assert_eq!(START_DELIMITER, '<');
        assert_eq!(END_DELIMITER, '>');
    }

    #[test]
    fn unambiguous_token_span_matches_identical_embeds() {
        // `<a><b>` chars → end-of-span offsets at `>` positions 2 and 5.
        let emb = embed_tokens(&["a", "b"]);
        assert_eq!(
            get_unambiguous_token_span_matches(&emb, &emb),
            vec![(2, 2), (5, 5)]
        );
    }

    #[test]
    fn unambiguous_token_span_matches_pins_unique_spans() {
        let ref_e = embed_tokens(&["alpha", "foo", "omega"]);
        let hyp_e = embed_tokens(&["alpha", "bar", "omega"]);
        let spans = get_unambiguous_token_span_matches(&ref_e, &hyp_e);
        // End of `<alpha>` and `<omega>` should pin; middle differs.
        let alpha_end = "<alpha>".chars().count() - 1; // offset of `>`
        let omega_start = "<alpha><foo>".chars().count();
        let omega_end = omega_start + "<omega>".chars().count() - 1;
        assert!(
            spans.contains(&(alpha_end, alpha_end)),
            "alpha span missing: {spans:?}"
        );
        assert!(
            spans.iter().any(|&(h, r)| h == omega_end && r == omega_end),
            "omega span missing: {spans:?}"
        );
    }

    #[test]
    fn narrowing_keeps_good_middle_when_edges_fail() {
        // Two leading + two trailing bad → firstGood=2 triggers recursion;
        // recursive slice then tightens absolute offsets.
        let refs = vec![
            "zzzzz".into(),
            "yyyyy".into(),
            "hello-world".into(),
            "goodbye-friend".into(),
            "xxxxx".into(),
            "wwwww".into(),
        ];
        let hyp = "hello-world-goodbye-friend";
        let (al, start, end) = error_align_with_narrowing(&refs, hyp, true, true);
        assert!(start < end, "slice {start}..{end}");
        assert!(!al.is_empty());
        // After recursion, absolute slice should cover the good middle.
        assert!(
            start <= 2 && end >= 4,
            "expected middle kept via recursion, got {start}..{end}"
        );
        // Must have narrowed away from full [0, len] (recursion ran).
        assert!(
            start > 0 || end < refs.len(),
            "expected narrowed slice, got {start}..{end}"
        );
    }

    #[test]
    fn narrowing_recursion_composes_absolute_offsets() {
        // Depth ≥1: enough leading bads that the recursive call itself can trim.
        let refs = vec![
            "aaa".into(),
            "bbb".into(),
            "ccc".into(),
            "hello-world".into(),
            "goodbye-friend".into(),
            "ddd".into(),
            "eee".into(),
            "fff".into(),
        ];
        let hyp = "hello-world-goodbye-friend";
        let (_al, start, end) = error_align_with_narrowing(&refs, hyp, true, true);
        assert!(start < end);
        assert!(start >= 1, "leading bads trimmed, got start={start}");
        assert!(end <= 7, "trailing bads trimmed, got end={end}");
        assert!(start <= 3 && end >= 5, "good middle in {start}..{end}");
    }
}
