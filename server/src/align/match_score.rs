//! Match quality gate (Storyteller `getSentenceRanges` score > 0).
//!
//! Storyteller keeps a sentence only when errorAlign scoring stays positive.
//! Until we port errorAlign, reject n-gram hits whose hyp slug is too far from
//! the query (Levenshtein: accept iff distance < query_len / 2).

/// Unicode-aware-enough Levenshtein on chars (slug strings are ASCII-heavy).
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr = vec![0usize; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

/// Storyteller-shaped score: `floor(len/2) - dist`, keep when `> 0`.
/// Empty query never matches. No MATCH ops → treat as reject (`-1`).
pub fn match_score(query: &str, hyp: &str) -> f64 {
    if query.is_empty() {
        return -1.0;
    }
    let dist = levenshtein(query, hyp) as i64;
    let base = (query.chars().count() as i64) / 2;
    let score = base - dist;
    if score <= 0 {
        -1.0
    } else {
        score as f64 / query.chars().count() as f64
    }
}

/// Keep this n-gram (or exact) hit as a matched anchor.
pub fn accept_match(query: &str, hyp: &str) -> bool {
    match_score(query, hyp) > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_slugs_accepted() {
        let q = "there-was-a-boy-named-peter";
        assert!(accept_match(q, q));
        assert!(match_score(q, q) > 0.0);
    }

    #[test]
    fn unrelated_slugs_rejected() {
        let q = "there-was-a-boy-named-peter";
        let h = "completely-unrelated-zebra-walrus-alpha";
        assert!(!accept_match(q, h));
    }

    #[test]
    fn small_typo_still_accepted() {
        let q = "hello-world-goodbye-moon";
        let h = "hello-world-goodbye-mooon";
        assert!(accept_match(q, h));
    }

    #[test]
    fn empty_query_rejected() {
        assert!(!accept_match("", "anything"));
    }
}
