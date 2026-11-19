//! Book Orbit–shaped title match + candidate relevance + match tiers.
//! Portions adapted from bookorbit/bookorbit `title-match.ts`, `candidate-relevance.ts`,
//! and `metadata-match.ts` (AGPL-3.0). See `/NOTICE`.

use super::providers::Candidate;
use serde_json::{json, Value};
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;

const ISBN_MATCH_SCORE: i32 = 100;
const AUTHOR_MATCH_SCORE: i32 = 3;
const AUTHOR_TOKEN_MATCH_SCORE: i32 = 1;
const MIN_RELEVANCE_SCORE: i32 = 3;

const TITLE_EXACT: f32 = 10.0;
const TITLE_PREFIX: f32 = 8.0;
const TITLE_SUBSTRING: f32 = 7.0;
const TITLE_TOKEN_OVERLAP: f32 = 6.0;
const TITLE_LEVENSHTEIN: f32 = 4.0;
const MIN_LEVENSHTEIN_SIMILARITY: f32 = 0.6;
const MIN_TOKEN_OVERLAP_RATIO: f32 = 0.5;

const STOPWORDS: &[&str] = &[
    "the", "an", "and", "or", "of", "in", "on", "at", "to", "for", "with", "from", "by", "is",
    "it", "as", "be", "are", "was", "were", "this", "that", "these", "those", "into", "over",
    "under", "its", "his", "her", "their", "our", "your", "my", "a",
];

/// Strip edition notes and a trailing "by Author" the way BO's match UI does.
pub fn title_core(title: &str) -> String {
    let mut t = title.to_string();
    while let Some(start) = t.find('(') {
        if let Some(end) = t[start..].find(')') {
            t.replace_range(start..start + end + 1, " ");
        } else {
            break;
        }
    }
    if let Some(idx) = t.to_lowercase().rfind(" by ") {
        t.truncate(idx);
    }
    if let Some((head, tail)) = t.split_once(':') {
        if tail.trim().len() > 12 {
            t = head.to_string();
        }
    }
    normalize_title_text(&t)
}

/// BO `foldTextForComparison` + `normalizeTitleText`.
///
/// shortcut: strips all Mn marks after NFKD (BO keeps Devanagari/kana marks); upgrade if CJK
/// orthography matching becomes a reported miss.
pub fn normalize_title_text(value: &str) -> String {
    let folded: String = value
        .nfkd()
        .filter(|ch| !unicode_normalization::char::is_combining_mark(*ch))
        .collect::<String>()
        .to_lowercase();
    let mut out = String::with_capacity(folded.len());
    let mut prev_space = true;
    for ch in folded.chars() {
        if ch.is_alphanumeric() {
            out.push(ch);
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim().to_string()
}

fn tokenize_title_text(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .filter(|t| t.len() > 1 || (t.len() == 1 && t.chars().all(|c| c.is_ascii_digit())))
        .map(str::to_string)
        .collect()
}

fn relevance_tokens(value: &str) -> Vec<String> {
    tokenize_title_text(value)
        .into_iter()
        .filter(|t| t.len() > 1)
        .collect()
}

fn significant_tokens(tokens: &[String]) -> Vec<String> {
    let significant: Vec<String> = tokens
        .iter()
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .cloned()
        .collect();
    if significant.is_empty() {
        tokens.to_vec()
    } else {
        significant
    }
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let mut from = 0;
    while from + needle.len() <= haystack.len() {
        if let Some(rel) = haystack[from..].find(needle) {
            let index = from + rel;
            let starts = index == 0 || haystack.as_bytes().get(index - 1) == Some(&b' ');
            let end = index + needle.len();
            let ends = end == haystack.len() || haystack.as_bytes().get(end) == Some(&b' ');
            if starts && ends {
                return true;
            }
            from = index + 1;
        } else {
            break;
        }
    }
    false
}

fn starts_with_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() || !haystack.starts_with(needle) {
        return false;
    }
    haystack.len() == needle.len() || haystack.as_bytes().get(needle.len()) == Some(&b' ')
}

fn levenshtein(a: &str, b: &str) -> usize {
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
    let mut cur = vec![0; m + 1];
    for i in 1..=n {
        cur[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m]
}

fn levenshtein_similarity(a: &str, b: &str) -> f32 {
    let max_len = a.chars().count().max(b.chars().count());
    if max_len == 0 {
        return 1.0;
    }
    1.0 - (levenshtein(a, b) as f32 / max_len as f32)
}

/// BO `scoreTitleMatch` (exact / prefix / substring / token overlap / Levenshtein).
pub fn score_title_match(query: &str, candidate: &str) -> f32 {
    let q = normalize_title_text(query);
    let c = normalize_title_text(candidate);
    if q.is_empty() || c.is_empty() {
        return 0.0;
    }
    if c == q {
        return TITLE_EXACT;
    }
    if starts_with_word(&c, &q) || starts_with_word(&q, &c) {
        return TITLE_PREFIX;
    }
    if contains_word(&c, &q) || contains_word(&q, &c) {
        return TITLE_SUBSTRING;
    }
    let query_tokens = significant_tokens(&relevance_tokens(&q));
    let candidate_tokens: HashSet<String> = relevance_tokens(&c).into_iter().collect();
    let overlap_ratio = if query_tokens.is_empty() {
        0.0
    } else {
        query_tokens
            .iter()
            .filter(|t| candidate_tokens.contains(*t))
            .count() as f32
            / query_tokens.len() as f32
    };
    let token_score = if overlap_ratio > MIN_TOKEN_OVERLAP_RATIO {
        overlap_ratio * TITLE_TOKEN_OVERLAP
    } else {
        0.0
    };
    let similarity = levenshtein_similarity(&q, &c);
    let levenshtein_score = if similarity >= MIN_LEVENSHTEIN_SIMILARITY {
        similarity * TITLE_LEVENSHTEIN
    } else {
        0.0
    };
    token_score.max(levenshtein_score)
}

fn bare_isbn(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'x' || *c == 'X')
        .collect::<String>()
        .to_uppercase()
}

fn score_author(query: &str, authors: &[String]) -> i32 {
    let q = normalize_title_text(query);
    if q.is_empty() {
        return 0;
    }
    for author in authors {
        let a = normalize_title_text(author);
        if a.is_empty() {
            continue;
        }
        if a.contains(&q) || q.contains(&a) {
            return AUTHOR_MATCH_SCORE;
        }
    }
    let q_last = q.split_whitespace().last().unwrap_or("");
    if !q_last.is_empty()
        && authors.iter().any(|a| {
            normalize_title_text(a)
                .split_whitespace()
                .last()
                .map(|l| l == q_last)
                .unwrap_or(false)
        })
    {
        return AUTHOR_TOKEN_MATCH_SCORE;
    }
    let left = vec![query.to_string()];
    if share_significant_token(&left, authors) {
        return AUTHOR_TOKEN_MATCH_SCORE;
    }
    0
}

fn share_significant_token(left: &[String], right: &[String]) -> bool {
    let left_tokens: HashSet<String> = left
        .iter()
        .flat_map(|v| tokenize_title_text(&normalize_title_text(v)))
        .filter(|t| t.len() > 2)
        .collect();
    if left_tokens.is_empty() {
        return false;
    }
    right.iter().any(|v| {
        tokenize_title_text(&normalize_title_text(v))
            .into_iter()
            .any(|t| left_tokens.contains(&t))
    })
}

fn skip_guide_title(title: &str) -> bool {
    let t = title.to_lowercase();
    [
        "summary",
        "study guide",
        "analysis",
        "guide to",
        "workbook",
        "review of",
        "chapter summary",
        "literature guide",
    ]
    .iter()
    .any(|p| t.contains(p))
}

/// Score one candidate the way BO `scoreCandidate` does (ISBN / title / author).
pub fn score_candidate(
    candidate: &Candidate,
    title: &str,
    author: &str,
    isbn: Option<&str>,
) -> i32 {
    if let Some(isbn) = isbn.map(str::trim).filter(|s| !s.is_empty()) {
        let want = bare_isbn(isbn);
        if !want.is_empty() {
            for got in [candidate.isbn13.as_deref(), candidate.isbn10.as_deref()]
                .into_iter()
                .flatten()
            {
                if bare_isbn(got) == want {
                    return ISBN_MATCH_SCORE;
                }
            }
        }
    }
    let matchable = candidate
        .title
        .as_deref()
        .or(candidate.subtitle.as_deref())
        .unwrap_or("");
    let mut score = 0i32;
    if !title.trim().is_empty() && !matchable.is_empty() {
        score = score_title_match(title, matchable).round() as i32;
    }
    if (title.trim().is_empty() || score > 0) && !author.trim().is_empty() {
        score += score_author(author, &candidate.authors);
    }
    score
}

/// BO `filterAndRank`: drop guides and weak hits, sort best-first, cap at `limit`.
pub fn filter_and_rank(
    candidates: Vec<Candidate>,
    title: &str,
    author: &str,
    isbn: Option<&str>,
    limit: usize,
) -> Vec<Candidate> {
    if title.trim().is_empty()
        && author.trim().is_empty()
        && isbn.map(str::trim).unwrap_or("").is_empty()
    {
        return candidates.into_iter().take(limit).collect();
    }
    let mut ranked: Vec<(i32, Candidate)> = candidates
        .into_iter()
        .filter(|c| {
            let t = c.title.as_deref().unwrap_or("");
            t.is_empty() || !skip_guide_title(t)
        })
        .map(|c| (score_candidate(&c, title, author, isbn), c))
        .filter(|(score, _)| *score >= MIN_RELEVANCE_SCORE)
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0));
    ranked.into_iter().take(limit).map(|(_, c)| c).collect()
}

/// Subject the match UI / assessMatch scores against (BO `MatchSubject`).
#[derive(Clone, Debug, Default)]
pub struct MatchSubject {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub isbn13: Option<String>,
    pub isbn10: Option<String>,
    pub series_name: Option<String>,
    pub series_index: Option<String>,
    /// provider key → external id already stored on the book
    pub provider_ids: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct MatchAssessment {
    pub tier: &'static str,
    pub score: f64,
    pub linked: bool,
    pub isbn_match: bool,
    pub title_match: &'static str,
    pub author_match: bool,
    pub series_match: bool,
    pub filled_fields: i32,
    pub sparse: bool,
}

impl MatchAssessment {
    pub fn to_json(&self) -> Value {
        json!({
            "tier": self.tier,
            "score": self.score,
            "linked": self.linked,
            "isbn_match": self.isbn_match,
            "title_match": self.title_match,
            "author_match": self.author_match,
            "series_match": self.series_match,
            "filled_fields": self.filled_fields,
            "sparse": self.sparse,
        })
    }
}

fn filled_fields(candidate: &Candidate) -> i32 {
    let mut n = 0;
    if candidate
        .title
        .as_deref()
        .filter(|s| !s.is_empty())
        .is_some()
    {
        n += 1;
    }
    if candidate
        .subtitle
        .as_deref()
        .filter(|s| !s.is_empty())
        .is_some()
    {
        n += 1;
    }
    if !candidate.authors.is_empty() {
        n += 1;
    }
    if candidate
        .description
        .as_deref()
        .filter(|s| !s.is_empty())
        .is_some()
    {
        n += 1;
    }
    if candidate
        .publisher
        .as_deref()
        .filter(|s| !s.is_empty())
        .is_some()
    {
        n += 1;
    }
    if candidate.published_date.is_some() || candidate.published_year.is_some() {
        n += 1;
    }
    if candidate
        .language
        .as_deref()
        .filter(|s| !s.is_empty())
        .is_some()
    {
        n += 1;
    }
    if candidate.page_count.is_some() {
        n += 1;
    }
    if candidate
        .series_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .is_some()
    {
        n += 1;
    }
    if candidate.isbn13.is_some() || candidate.isbn10.is_some() {
        n += 1;
    }
    if !candidate.genres.is_empty() {
        n += 1;
    }
    if candidate.community_rating.is_some() {
        n += 1;
    }
    if candidate
        .cover_url
        .as_deref()
        .filter(|s| !s.is_empty())
        .is_some()
    {
        n += 1;
    }
    n
}

fn last_name(name: &str) -> String {
    normalize_title_text(name)
        .split_whitespace()
        .last()
        .unwrap_or("")
        .to_string()
}

/// BO `assessMatch` — strong (linked/ISBN), possible (same title+author), else weak.
pub fn assess_match(candidate: &Candidate, subject: &MatchSubject) -> MatchAssessment {
    let linked = subject
        .provider_ids
        .iter()
        .any(|(k, id)| k == &candidate.provider && !id.is_empty() && id == &candidate.external_id);

    let subject_isbns: HashSet<String> = [subject.isbn13.as_deref(), subject.isbn10.as_deref()]
        .into_iter()
        .flatten()
        .map(bare_isbn)
        .filter(|s| !s.is_empty())
        .collect();
    let isbn_match = [candidate.isbn13.as_deref(), candidate.isbn10.as_deref()]
        .into_iter()
        .flatten()
        .map(bare_isbn)
        .any(|isbn| !isbn.is_empty() && subject_isbns.contains(&isbn));

    let book_title = title_core(subject.title.as_deref().unwrap_or(""));
    let result_title = title_core(
        candidate
            .title
            .as_deref()
            .or(candidate.subtitle.as_deref())
            .unwrap_or(""),
    );
    let title_match = if !book_title.is_empty() && result_title == book_title {
        "same"
    } else if !book_title.is_empty()
        && !result_title.is_empty()
        && (result_title.starts_with(&book_title) || book_title.starts_with(&result_title))
    {
        "longer"
    } else {
        "different"
    };

    let book_authors: Vec<String> = subject
        .authors
        .iter()
        .map(|a| normalize_title_text(a))
        .filter(|a| !a.is_empty())
        .collect();
    let book_last: HashSet<String> = subject
        .authors
        .iter()
        .map(|a| last_name(a))
        .filter(|s| !s.is_empty())
        .collect();
    let author_match = candidate.authors.iter().any(|author| {
        let n = normalize_title_text(author);
        book_authors.iter().any(|b| b == &n) || book_last.contains(&last_name(author))
    });

    let series_match = match (
        candidate.series_name.as_deref(),
        subject.series_name.as_deref(),
    ) {
        (Some(c), Some(s))
            if !c.is_empty()
                && !s.is_empty()
                && normalize_title_text(c) == normalize_title_text(s)
                && candidate.series_index.as_deref().unwrap_or("")
                    == subject.series_index.as_deref().unwrap_or("") =>
        {
            true
        }
        _ => false,
    };

    let filled = filled_fields(candidate);
    let tier = if linked || isbn_match {
        "strong"
    } else if title_match == "same" && author_match {
        "possible"
    } else {
        "weak"
    };
    let score = (if linked { 60.0 } else { 0.0 })
        + (if isbn_match { 45.0 } else { 0.0 })
        + match title_match {
            "same" => 25.0,
            "longer" => 6.0,
            _ => 0.0,
        }
        + (if author_match { 20.0 } else { 0.0 })
        + (if series_match { 10.0 } else { 0.0 })
        + (filled as f64) * 0.6;

    MatchAssessment {
        tier,
        score,
        linked,
        isbn_match,
        title_match,
        author_match,
        series_match,
        filled_fields: filled,
        sparse: filled <= 4,
    }
}

/// BO `rankMatches` + `groupMatches`.
pub fn rank_and_group(
    candidates: Vec<Candidate>,
    subject: &MatchSubject,
    provider_order: &[String],
) -> Value {
    let order: std::collections::HashMap<&str, usize> = provider_order
        .iter()
        .enumerate()
        .map(|(i, k)| (k.as_str(), i))
        .collect();
    let mut ranked: Vec<(Candidate, MatchAssessment, usize)> = candidates
        .into_iter()
        .enumerate()
        .map(|(index, c)| {
            let a = assess_match(&c, subject);
            (c, a, index)
        })
        .collect();
    fn tier_ord(t: &str) -> i32 {
        match t {
            "strong" => 0,
            "possible" => 1,
            _ => 2,
        }
    }
    ranked.sort_by(|a, b| {
        tier_ord(a.1.tier)
            .cmp(&tier_ord(b.1.tier))
            .then(
                b.1.score
                    .partial_cmp(&a.1.score)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(
                order
                    .get(a.0.provider.as_str())
                    .copied()
                    .unwrap_or(usize::MAX)
                    .cmp(
                        &order
                            .get(b.0.provider.as_str())
                            .copied()
                            .unwrap_or(usize::MAX),
                    ),
            )
            .then(a.2.cmp(&b.2))
    });

    let mut strong = Vec::new();
    let mut possible = Vec::new();
    let mut weak = Vec::new();
    let mut flat = Vec::new();
    for (c, a, _) in ranked {
        let entry = json!({
            "candidate": super::providers::candidate_json(&c),
            "match": a.to_json(),
        });
        match a.tier {
            "strong" => strong.push(entry.clone()),
            "possible" => possible.push(entry.clone()),
            _ => weak.push(entry.clone()),
        }
        flat.push(entry);
    }
    json!({
        "results": flat,
        "groups": {
            "strong": strong,
            "possible": possible,
            "weak": weak,
        }
    })
}

/// Prefer a real book language over OL's arbitrary first code (e.g. `cat` before `eng`).
pub fn prefer_language_code(codes: &[String]) -> Option<String> {
    if codes.is_empty() {
        return None;
    }
    const PREFERRED: &[&str] = &[
        "eng", "en", "enm", "en-us", "en-gb", "spa", "es", "fre", "fr", "ger", "de", "ita", "it",
        "por", "pt", "dut", "nl", "rus", "ru", "jpn", "ja", "chi", "zh", "kor", "ko", "ara", "ar",
        "pol", "pl", "swe", "sv", "nor", "no", "dan", "da", "fin", "fi", "cze", "cs", "hun", "hu",
        "rum", "ro", "tur", "tr", "gre", "el", "heb", "he", "lat", "la",
    ];
    for pref in PREFERRED {
        if let Some(hit) = codes.iter().find(|c| c.eq_ignore_ascii_case(pref)) {
            return Some(hit.clone());
        }
    }
    codes
        .iter()
        .find(|c| {
            let n = c.len();
            (2..=3).contains(&n) && c.chars().all(|ch| ch.is_ascii_alphabetic())
        })
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(title: &str, authors: &[&str]) -> Candidate {
        Candidate {
            provider: "openLibrary".into(),
            external_id: "x".into(),
            title: Some(title.into()),
            authors: authors.iter().map(|s| (*s).to_string()).collect(),
            ..Candidate::default()
        }
    }

    #[test]
    fn ranks_exact_title_author_above_weak() {
        let title = "The Return of the King";
        let author = "J. R. R. Tolkien";
        let list = vec![
            cand("Italian Cooking Basics", &["Someone"]),
            cand("The Return of the King", &["J.R.R. Tolkien"]),
            cand("The Lord of the Rings", &["J.R.R. Tolkien"]),
        ];
        let ranked = filter_and_rank(list, title, author, None, 5);
        assert_eq!(ranked[0].title.as_deref(), Some("The Return of the King"));
    }

    #[test]
    fn title_core_strips_long_subtitle() {
        let core =
            title_core("The Return of the King: Being the Third Part of the Lord of the Rings");
        assert!(core.starts_with("the return of the king"));
        assert!(!core.contains("being"));
    }

    #[test]
    fn fold_drops_latin_accents() {
        assert_eq!(
            normalize_title_text("Sōseki"),
            normalize_title_text("Soseki")
        );
    }

    #[test]
    fn levenshtein_typo_still_scores() {
        let score = score_title_match("Return of the King", "Return of the Kin");
        assert!(score >= MIN_LEVENSHTEIN_SIMILARITY * TITLE_LEVENSHTEIN * 0.9);
    }

    #[test]
    fn ubik_does_not_match_rubik() {
        assert_eq!(score_title_match("Ubik", "Rubik Cube Solutions"), 0.0);
    }

    #[test]
    fn assess_isbn_is_strong() {
        let mut c = cand("Anything", &["X"]);
        c.isbn13 = Some("9780261102378".into());
        let subject = MatchSubject {
            title: Some("Other".into()),
            authors: vec!["Y".into()],
            isbn13: Some("978-0-261-10237-8".into()),
            ..MatchSubject::default()
        };
        let a = assess_match(&c, &subject);
        assert_eq!(a.tier, "strong");
        assert!(a.isbn_match);
    }

    #[test]
    fn assess_same_title_author_is_possible() {
        let c = cand("The Return of the King", &["J. R. R. Tolkien"]);
        let subject = MatchSubject {
            title: Some("The Return of the King".into()),
            authors: vec!["J.R.R. Tolkien".into()],
            ..MatchSubject::default()
        };
        assert_eq!(assess_match(&c, &subject).tier, "possible");
    }

    #[test]
    fn prefers_eng_over_cat() {
        let codes = vec!["cat".into(), "eng".into(), "fre".into()];
        assert_eq!(prefer_language_code(&codes).as_deref(), Some("eng"));
    }
}
