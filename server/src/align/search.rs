//! Chapter/sentence locate via n-gram boundary voting.
//!
//! Port of Storyteller `libraries/align/src/align/search.ts` `findBoundaries`
//! (word-delimited path used by the transcription aligner).

use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Boundaries {
    pub start: usize,
    pub end: usize,
}

/// Precomputed n-gram → word-start positions for one slugified document.
pub struct NgramIndex {
    index: HashMap<String, Vec<usize>>,
    ngram_size: usize,
}

impl NgramIndex {
    pub fn build(text: &str, ngram_size: usize) -> Self {
        let mut index: HashMap<String, Vec<usize>> = HashMap::new();
        for (ngram, pos) in ngrams(text, ngram_size) {
            index.entry(ngram).or_default().push(pos);
        }
        Self { index, ngram_size }
    }
}

/// Locate `query` inside `document`. Both strings should already be slugified
/// with `-` as the gram delimiter (see [`super::slugify::slugify_for_search`]).
pub fn find_boundaries(query: &str, document: &str) -> Option<Boundaries> {
    let index = NgramIndex::build(document, 5);
    find_boundaries_with_index(query, document, &index, 0, 1000)
}

/// Like [`find_boundaries`], reusing a prebuilt index. `min_start` is a character
/// offset floor in `document` (Storyteller advances through the transcript).
pub fn find_boundaries_with_index(
    query: &str,
    document: &str,
    index: &NgramIndex,
    min_start: usize,
    bin_size: usize,
) -> Option<Boundaries> {
    if query.is_empty() {
        return None;
    }
    if min_start > 0 {
        let mut start = min_start.min(document.len());
        // Align to a `-`-delimited word so a mid-token floor cannot break n-grams.
        if start > 0 && start < document.len() && !document[start..].starts_with('-') {
            if let Some(rel) = document[start..].find('-') {
                start += rel + 1;
            }
        } else if start < document.len() && document[start..].starts_with('-') {
            start += 1;
        }
        let suffix = &document[start..];
        if suffix.is_empty() {
            return None;
        }
        let suffix_index = NgramIndex::build(suffix, index.ngram_size);
        let hit = find_boundaries_with_index(query, suffix, &suffix_index, 0, bin_size)?;
        return Some(Boundaries {
            start: hit.start + start,
            end: hit.end + start,
        });
    }
    // Queries shorter than the n-gram width cannot vote; exact slug match instead
    // (otherwise every short sentence becomes an interpolate-only gap).
    if grams(query).len() < index.ngram_size {
        return find_exact_slug(query, document);
    }
    let (start_votes, end_votes) = collect_boundary_votes(query, document, index)?;
    let best_start = choose_best_from_bins(&bin_boundary_votes(&start_votes, bin_size), -1)?;
    let best_end = choose_best_from_bins(&bin_boundary_votes(&end_votes, bin_size), 1)?;
    Some(Boundaries {
        start: offset_from_word_index(best_start, document),
        end: offset_from_word_index(best_end, document),
    })
}

fn find_exact_slug(query: &str, document: &str) -> Option<Boundaries> {
    let start = document.find(query)?;
    Some(Boundaries {
        start,
        end: start + query.len(),
    })
}

fn grams(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split('-').collect()
    }
}

fn ngrams(text: &str, ngram_size: usize) -> Vec<(String, usize)> {
    let grams = grams(text);
    if ngram_size == 0 || grams.len() < ngram_size {
        return Vec::new();
    }
    (0..=grams.len() - ngram_size)
        .map(|i| (grams[i..i + ngram_size].join("-"), i))
        .collect()
}

fn collect_boundary_votes(
    query: &str,
    document: &str,
    index: &NgramIndex,
) -> Option<(Vec<i64>, Vec<i64>)> {
    let query_grams = grams(query);
    let mut skipped = 0usize;
    let mut total = 0usize;
    let mut start_votes = Vec::new();
    let mut end_votes = Vec::new();
    let _ = document;
    for (ngram, start) in ngrams(query, index.ngram_size) {
        total += 1;
        let Some(doc_starts) = index.index.get(&ngram) else {
            skipped += 1;
            continue;
        };
        for &document_start in doc_starts {
            start_votes.push(document_start as i64 - start as i64);
            end_votes.push(document_start as i64 + (query_grams.len() as i64 - start as i64));
        }
    }
    if total == 0 || skipped > total / 2 || start_votes.is_empty() {
        return None;
    }
    Some((start_votes, end_votes))
}

fn bin_boundary_votes(votes: &[i64], bin_size: usize) -> BTreeMap<i64, Vec<i64>> {
    let mut bins: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    let Some(&start) = votes.iter().min() else {
        return bins;
    };
    let bin_size = bin_size.max(1) as i64;
    for &vote in votes {
        let bin_index = (vote - start).div_euclid(bin_size);
        bins.entry(bin_index).or_default().push(vote);
    }
    bins
}

fn choose_best_from_bins(bins: &BTreeMap<i64, Vec<i64>>, dir: i8) -> Option<i64> {
    let total: usize = bins.values().map(|b| b.len()).sum();
    if total == 0 {
        return None;
    }
    let best = bins.values().max_by_key(|b| b.len())?;
    if (best.len() as f64) / (total as f64) < 0.2 {
        return None;
    }
    if dir > 0 {
        best.iter().copied().max()
    } else {
        best.iter().copied().min()
    }
}

/// Storyteller: for each word before `word_index`, add `len + 1` (delimiter).
fn offset_from_word_index(word_index: i64, document: &str) -> usize {
    let words = grams(document);
    let limit = (word_index.max(0) as usize).min(words.len());
    let mut offset = 0usize;
    for word in words.iter().take(limit) {
        offset += word.len() + 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::slugify::slugify_for_search;

    #[test]
    fn finds_phrase_inside_longer_document() {
        let document =
            slugify_for_search("Once upon a time there was a boy named Peter and he loved to fly");
        let query = slugify_for_search("there was a boy named Peter");
        let hit = find_boundaries(&query, &document).expect("boundaries");
        assert!(hit.start < hit.end);
        assert!(hit.end <= document.len() + 1);
    }

    #[test]
    fn returns_none_when_query_missing() {
        let document = slugify_for_search("alpha beta gamma delta epsilon");
        let query = slugify_for_search("completely unrelated zebra walrus");
        assert!(find_boundaries(&query, &document).is_none());
    }

    #[test]
    fn short_query_uses_exact_slug_match() {
        // ngramSize=5 cannot vote; exact slug substring still locates the phrase.
        let document = slugify_for_search("once upon a time there was a boy named peter");
        let query = slugify_for_search("there was a boy");
        let hit = find_boundaries(&query, &document).expect("exact short match");
        assert!(document[hit.start..hit.end].contains("there-was-a-boy"));
    }

    #[test]
    fn min_start_skips_earlier_duplicate_match() {
        let document = slugify_for_search(
            "there was a boy named peter then later there was a boy named peter again",
        );
        let query = slugify_for_search("there was a boy named peter");
        let index = NgramIndex::build(&document, 5);
        let first = find_boundaries_with_index(&query, &document, &index, 0, 1000).expect("first");
        // Floor just after the first match's start so the second copy remains.
        let floor = (first.start + 1).min(document.len());
        let second = find_boundaries_with_index(&query, &document, &index, floor, 1000)
            .unwrap_or_else(|| {
                panic!("second missing; doc={document} first={first:?} floor={floor}")
            });
        assert!(second.start >= floor);
        assert!(second.start > first.start);
    }
}
