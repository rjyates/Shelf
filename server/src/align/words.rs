//! Word-level range expansion (Storyteller `granularity: word`).
//!
//! Tokenization follows `@echogarden/text-segmentation` `splitToWords`: suppressions
//! (CLDR/noun/TLD), dotted abbrevs, numbers/currency, then letter/digit runs; punctuation
//! and whitespace are separate tokens. Align/markup word lists use non-punctuation entries
//! (ST `!isPunctuation` / `slugifiedWordRanges`).

use crate::align::error_align::{alignments_for_sentence, error_align, hyp_span_chars};
use crate::align::ranges::{AudioRange, SentenceRange};
use crate::align::slugify::slugify_for_search;
use crate::align::suppressions::word_suppressions;

/// Echogarden-shaped word/punct token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordToken {
    pub text: String,
    pub is_punctuation: bool,
}

fn is_word_char(c: char) -> bool {
    c.is_alphabetic() || c.is_numeric() || matches!(c, '_' | '\u{0300}'..='\u{036F}')
}

fn is_letter_or_mark(c: char) -> bool {
    c.is_alphabetic() || matches!(c, '\u{0300}'..='\u{036F}')
}

fn currency_or_percent(c: char) -> bool {
    matches!(
        c,
        '%' | '$'
            | '¥'
            | '€'
            | '£'
            | '₩'
            | '₭'
            | '₽'
            | '₫'
            | '฿'
            | '¢'
            | '₮'
            | '؋'
            | '₦'
            | '₱'
            | '₴'
            | '₪'
    )
}

fn try_suppression_at(text: &str, i: usize, suppressions: &[String]) -> Option<usize> {
    let rest = &text[i..];
    for s in suppressions {
        let mut matched = 0usize;
        let mut ok = true;
        for (a, b) in rest.chars().zip(s.chars()) {
            if a.to_lowercase().next() != b.to_lowercase().next() {
                ok = false;
                break;
            }
            matched += a.len_utf8();
        }
        if !ok || s.chars().count() > rest.chars().count() || matched == 0 {
            continue;
        }
        let end = i + matched;
        if text[end..].chars().next().is_some_and(is_word_char) {
            continue;
        }
        return Some(end);
    }
    None
}

/// Dotted abbrev: `U.S.A.` / `Dr.` / `M.` (letter (`.` letter)+ `.`?).
fn try_dotted_abbrev_at(text: &str, i: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if i >= text.len() {
        return None;
    }
    let chars: Vec<(usize, char)> = text[i..].char_indices().map(|(o, c)| (i + o, c)).collect();
    if chars.is_empty() || !chars[0].1.is_alphabetic() {
        return None;
    }
    let mut k = 1usize;
    let mut saw_dot_letter = false;
    while k + 1 < chars.len() {
        if chars[k].1 == '.' && chars[k + 1].1.is_alphabetic() {
            saw_dot_letter = true;
            k += 2;
            continue;
        }
        break;
    }
    if !saw_dot_letter {
        return None;
    }
    if k < chars.len() && chars[k].1 == '.' {
        k += 1;
    }
    let end = if k < chars.len() {
        chars[k].0
    } else {
        text.len()
    };
    let next = text[end..].chars().next();
    if next.is_some_and(|c| is_letter_or_mark(c) || c.is_ascii_digit()) {
        return None;
    }
    let _ = bytes;
    Some(end)
}

/// Number / currency / percent run (lite Echogarden number patterns).
fn try_number_at(text: &str, i: usize) -> Option<usize> {
    let chars: Vec<(usize, char)> = text[i..].char_indices().map(|(o, c)| (i + o, c)).collect();
    if chars.is_empty() {
        return None;
    }
    let mut k = 0usize;
    if currency_or_percent(chars[0].1) {
        k = 1;
        if k >= chars.len() || !chars[k].1.is_ascii_digit() {
            return None;
        }
    } else if matches!(chars[0].1, '+' | '-') {
        if chars.len() < 2 || !chars[1].1.is_ascii_digit() {
            return None;
        }
        k = 1;
    } else if !chars[0].1.is_ascii_digit() {
        return None;
    }
    while k < chars.len() {
        let c = chars[k].1;
        if c.is_ascii_digit() {
            k += 1;
            continue;
        }
        if matches!(c, '.' | ',' | '٬' | '_')
            && k + 1 < chars.len()
            && chars[k + 1].1.is_ascii_digit()
        {
            k += 2;
            while k < chars.len() && chars[k].1.is_ascii_digit() {
                k += 1;
            }
            continue;
        }
        break;
    }
    if k < chars.len() && currency_or_percent(chars[k].1) {
        k += 1;
    } else if k < chars.len() && chars[k].1.is_alphabetic() {
        // 234ms / 2.4a — digits then letters
        while k < chars.len() && (chars[k].1.is_alphabetic() || chars[k].1 == '_') {
            k += 1;
        }
    }
    let end = if k < chars.len() {
        chars[k].0
    } else {
        text.len()
    };
    if end == i {
        return None;
    }
    // Must have consumed at least one digit.
    if !text[i..end].chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(end)
}

fn try_basic_word_at(text: &str, i: usize) -> Option<usize> {
    let chars: Vec<(usize, char)> = text[i..].char_indices().map(|(o, c)| (i + o, c)).collect();
    if chars.is_empty() || !is_word_char(chars[0].1) {
        return None;
    }
    let mut k = 1usize;
    while k < chars.len() {
        let c = chars[k].1;
        if is_word_char(c) {
            k += 1;
            continue;
        }
        // Inner apostrophe: letter/digit ' letter
        if matches!(c, '\'' | '\u{2019}' | '\u{2018}')
            && k + 1 < chars.len()
            && is_letter_or_mark(chars[k + 1].1)
        {
            k += 2;
            continue;
        }
        // Hyphenated: word - word
        if c == '-' && k + 1 < chars.len() && is_word_char(chars[k + 1].1) {
            k += 2;
            while k < chars.len() && is_word_char(chars[k].1) {
                k += 1;
            }
            continue;
        }
        break;
    }
    Some(if k < chars.len() {
        chars[k].0
    } else {
        text.len()
    })
}

fn push_punct_gap(out: &mut Vec<WordToken>, text: &str, start: usize, end: usize) {
    if start >= end {
        return;
    }
    // Echogarden: spaces coalesce into one punct token; other chars are singleton tokens.
    let slice = &text[start..end];
    let mut buf = String::new();
    let flush_spaces = |buf: &mut String, out: &mut Vec<WordToken>| {
        if !buf.is_empty() {
            out.push(WordToken {
                text: std::mem::take(buf),
                is_punctuation: true,
            });
        }
    };
    for c in slice.chars() {
        if c == ' ' {
            buf.push(c);
        } else {
            flush_spaces(&mut buf, out);
            out.push(WordToken {
                text: c.to_string(),
                is_punctuation: true,
            });
        }
    }
    flush_spaces(&mut buf, out);
}

/// Full Echogarden-shaped token stream (words + punctuation/whitespace).
pub fn tokenize_words(text: &str, locale: &str) -> Vec<WordToken> {
    let lang = locale
        .split(['-', '_'])
        .next()
        .unwrap_or("en")
        .to_ascii_lowercase();
    let suppressions = word_suppressions(&lang);
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < text.len() {
        // Ensure we are on a char boundary.
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let start = i;
        let end = try_suppression_at(text, i, &suppressions)
            .or_else(|| try_dotted_abbrev_at(text, i))
            .or_else(|| try_number_at(text, i))
            .or_else(|| try_basic_word_at(text, i));
        if let Some(end) = end {
            if end > start {
                // Gap before this word → punctuation tokens.
                // (Caller tracks last_end; we scan linearly so gap is empty here —
                // punctuation is whatever we skip when no word match.)
                out.push(WordToken {
                    text: text[start..end].to_string(),
                    is_punctuation: false,
                });
                i = end;
                continue;
            }
        }
        // No word: consume one char as punct (or coalesce spaces below via gap helper).
        let ch = text[i..].chars().next().unwrap();
        let next = i + ch.len_utf8();
        // Look ahead: gather a run until next word start so spaces coalesce.
        let mut j = next;
        while j < text.len() {
            if !text.is_char_boundary(j) {
                j += 1;
                continue;
            }
            if try_suppression_at(text, j, &suppressions).is_some()
                || try_dotted_abbrev_at(text, j).is_some()
                || try_number_at(text, j).is_some()
                || try_basic_word_at(text, j).is_some()
            {
                break;
            }
            let c = text[j..].chars().next().unwrap();
            j += c.len_utf8();
        }
        push_punct_gap(&mut out, text, i, j);
        i = j;
    }
    out
}

/// Non-punctuation word strings (ST markup / `slugifiedWordRanges` filter).
pub fn split_words(sentence: &str) -> Vec<String> {
    split_words_locale(sentence, "en")
}

pub fn split_words_locale(sentence: &str, locale: &str) -> Vec<String> {
    tokenize_words(sentence, locale)
        .into_iter()
        .filter(|t| !t.is_punctuation)
        .map(|t| t.text)
        .collect()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Nest word spans inside each sentence span. When `chapter_id` is set, use
/// ST `{chapterId}-s{i}` / `{chapterId}-s{i}-w{j}` + `epub:type=storyteller:word-span`;
/// else Foliate `sentence-N` / `word-N`. Returns updated XHTML and per-sentence words.
pub fn inject_word_spans(
    xhtml: &str,
    sentences: &[String],
    start_id: usize,
    chapter_id: Option<&str>,
) -> (String, Vec<Vec<String>>, usize) {
    inject_word_spans_locale(xhtml, sentences, start_id, chapter_id, "en")
}

pub fn inject_word_spans_locale(
    xhtml: &str,
    sentences: &[String],
    start_id: usize,
    chapter_id: Option<&str>,
    locale: &str,
) -> (String, Vec<Vec<String>>, usize) {
    let mut out = xhtml.to_string();
    let mut next_id = start_id;
    let mut words_out = Vec::new();
    for (sid, sentence) in sentences.iter().enumerate() {
        let tokens = tokenize_words(sentence, locale);
        let words: Vec<String> = tokens
            .iter()
            .filter(|t| !t.is_punctuation)
            .map(|t| t.text.clone())
            .collect();
        words_out.push(words.clone());
        if words.is_empty() {
            continue;
        }
        let mut inner = String::new();
        let mut wj = 0usize;
        for t in &tokens {
            if t.is_punctuation {
                inner.push_str(&xml_escape(&t.text));
                continue;
            }
            let (id, epub_type) = match chapter_id {
                Some(cid) => (
                    format!("{cid}-s{sid}-w{wj}"),
                    r#" epub:type="storyteller:word-span""#,
                ),
                None => (
                    format!("word-{next_id}"),
                    r#" epub:type="storyteller:word-span""#,
                ),
            };
            inner.push_str(&format!(
                r#"<span id="{id}" class="shelf-mo-word"{epub_type}>{}</span>"#,
                xml_escape(&t.text)
            ));
            next_id += 1;
            wj += 1;
        }
        let open = match chapter_id {
            Some(cid) => format!(r#"id="{cid}-s{sid}""#),
            None => format!(r#"id="sentence-{sid}""#),
        };
        if let Some(pos) = out.find(&open) {
            if let Some(gt) = out[pos..].find('>') {
                let inner_start = pos + gt + 1;
                if let Some(close_rel) = out[inner_start..].find("</span>") {
                    let inner_end = inner_start + close_rel;
                    out.replace_range(inner_start..inner_end, &inner);
                }
            }
        }
    }
    (out, words_out, next_id)
}

type TimesFn<'a> = dyn Fn(usize, usize) -> Option<(f64, f64, String)> + 'a;

/// Expand sentence ranges into one range per word. Word ids are sequential
/// across the book (0..N) so SMIL can target `#word-{id}`.
pub fn expand_to_word_ranges(
    sentences: &[SentenceRange],
    words_per_sentence: &[(String, usize, Vec<String>)],
) -> Vec<SentenceRange> {
    expand_to_word_ranges_with(sentences, words_per_sentence, None, None)
}

/// Storyteller word path: when `slug_doc` + `times_for_span` are provided, align
/// each sentence's words against its stored hyp slug span.
pub fn expand_to_word_ranges_with(
    sentences: &[SentenceRange],
    words_per_sentence: &[(String, usize, Vec<String>)],
    slug_doc: Option<&str>,
    times_for_span: Option<&TimesFn<'_>>,
) -> Vec<SentenceRange> {
    let mut out = Vec::new();
    let mut word_id = 0usize;
    let index: std::collections::HashMap<(String, usize), &Vec<String>> = words_per_sentence
        .iter()
        .map(|(ch, id, words)| ((ch.clone(), *id), words))
        .collect();

    for sent in sentences {
        let words = index
            .get(&(sent.chapter_id.clone(), sent.id))
            .map(|w| w.as_slice())
            .unwrap_or(&[]);
        if words.is_empty() {
            out.push(
                SentenceRange::new(word_id, sent.chapter_id.clone(), sent.audio_ranges.clone())
                    .with_state(sent.state),
            );
            word_id += 1;
            continue;
        }

        let st_word_id = |j: usize| format!("{}-s{}-w{j}", sent.chapter_id, sent.id);

        if let (Some(doc), Some(times), Some(ss), Some(se)) =
            (slug_doc, times_for_span, sent.slug_start, sent.slug_end)
        {
            if se > ss && se <= doc.len() {
                if let Some(ranges) =
                    expand_words_via_error_align(sent, words, &doc[ss..se], ss, times)
                {
                    for (j, r) in ranges.into_iter().enumerate() {
                        out.push(
                            SentenceRange::new(word_id, sent.chapter_id.clone(), r.audio_ranges)
                                .with_state(sent.state)
                                .with_text_id(st_word_id(j)),
                        );
                        word_id += 1;
                    }
                    continue;
                }
            }
        }

        // Fallback: character-weight split (interpolated / missing hyp span).
        let first = sent.first();
        let last = sent.last();
        let audiofile = first.audiofile.clone();
        let start = first.start;
        let end = last.end;
        let span = (end - start).max(0.0);
        let total_w: f64 = words.iter().map(|w| w.chars().count().max(1) as f64).sum();
        let mut cursor = start;
        for (i, word) in words.iter().enumerate() {
            let w = word.chars().count().max(1) as f64;
            let s = cursor;
            let e = if i + 1 == words.len() {
                end
            } else if total_w > 0.0 {
                s + span * w / total_w
            } else {
                s
            };
            cursor = e;
            out.push(
                SentenceRange::new(
                    word_id,
                    sent.chapter_id.clone(),
                    vec![AudioRange {
                        audiofile: audiofile.clone(),
                        start: s,
                        end: e.max(s + 0.001),
                    }],
                )
                .with_state(sent.state)
                .with_text_id(st_word_id(i)),
            );
            word_id += 1;
        }
    }
    out
}

/// CTC Word path: prefer ST `buildWordRanges` clips when count matches words;
/// otherwise fall back to proportional split (interpolated / partial sentences).
pub fn expand_to_word_ranges_ctc(
    sentences: &[SentenceRange],
    words_per_sentence: &[(String, usize, Vec<String>)],
    ctc_words: &std::collections::HashMap<(String, usize), Vec<AudioRange>>,
) -> Vec<SentenceRange> {
    let index: std::collections::HashMap<(String, usize), &Vec<String>> = words_per_sentence
        .iter()
        .map(|(ch, id, words)| ((ch.clone(), *id), words))
        .collect();
    let mut need_fallback = Vec::new();
    let mut out = Vec::new();
    let mut word_id = 0usize;
    for sent in sentences {
        let words = index
            .get(&(sent.chapter_id.clone(), sent.id))
            .map(|w| w.as_slice())
            .unwrap_or(&[]);
        let st_word_id = |j: usize| format!("{}-s{}-w{j}", sent.chapter_id, sent.id);
        if let Some(clips) = ctc_words.get(&(sent.chapter_id.clone(), sent.id)) {
            if !words.is_empty() && clips.len() == words.len() {
                for (j, clip) in clips.iter().enumerate() {
                    out.push(
                        SentenceRange::new(word_id, sent.chapter_id.clone(), vec![clip.clone()])
                            .with_state(sent.state)
                            .with_text_id(st_word_id(j)),
                    );
                    word_id += 1;
                }
                continue;
            }
        }
        need_fallback.push(sent.clone());
    }
    if need_fallback.is_empty() {
        return out;
    }
    // Proportional for sentences without a 1:1 CTC word clip set.
    let mut proportional =
        expand_to_word_ranges_with(&need_fallback, words_per_sentence, None, None);
    // Rebase ids after CTC-prefixed words.
    for r in &mut proportional {
        r.id = word_id;
        word_id += 1;
    }
    out.extend(proportional);
    out
}

fn expand_words_via_error_align(
    sent: &SentenceRange,
    words: &[String],
    hyp_slice: &str,
    abs_base: usize,
    times: &TimesFn<'_>,
) -> Option<Vec<SentenceRange>> {
    let word_slugs: Vec<String> = words.iter().map(|w| slugify_for_search(w)).collect();
    if word_slugs.iter().all(|s| s.is_empty()) {
        return None;
    }
    let ref_joined = word_slugs.join("-");
    let alignments = error_align(&ref_joined, hyp_slice);
    let mut align_idx = 0usize;
    let mut hyp_local = 0usize;
    let mut out = Vec::new();
    for slug in &word_slugs {
        if slug.is_empty() {
            continue;
        }
        if align_idx >= alignments.len() {
            return None;
        }
        let (used, _score) = alignments_for_sentence(slug, &alignments[align_idx..]);
        if used == 0 {
            return None;
        }
        let span_ops = &alignments[align_idx..align_idx + used];
        let hyp_chars = hyp_span_chars(span_ops);
        let abs_start = abs_base + hyp_local;
        let abs_end = abs_start + hyp_chars;
        let (start_t, end_t, audiofile) = if hyp_chars > 0 {
            times(abs_start, abs_end).unwrap_or_else(|| {
                (
                    sent.first().start,
                    sent.last().end,
                    sent.first().audiofile.clone(),
                )
            })
        } else {
            // DELETE-only word: park a tiny stub at current cursor time.
            let t = times(abs_start, abs_start.saturating_add(1))
                .map(|(s, _, f)| (s, s + 0.001, f))
                .unwrap_or_else(|| {
                    let s = sent.first().start;
                    (s, s + 0.001, sent.first().audiofile.clone())
                });
            t
        };
        out.push(SentenceRange::new(
            0, // reassigned by caller
            sent.chapter_id.clone(),
            vec![AudioRange {
                audiofile,
                start: start_t,
                end: end_t.max(start_t + 0.001),
            }],
        ));
        align_idx += used;
        hyp_local += hyp_chars;
        if hyp_slice.as_bytes().get(hyp_local) == Some(&b'-') {
            hyp_local += 1;
        }
    }
    if out.len()
        == words
            .iter()
            .filter(|w| !slugify_for_search(w).is_empty())
            .count()
    {
        Some(out)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::ranges::AudioRange;

    #[test]
    fn splits_proportionally_by_length() {
        let sent = SentenceRange::new(
            0,
            "c1",
            vec![AudioRange {
                audiofile: "a.mp3".into(),
                start: 0.0,
                end: 4.0,
            }],
        );
        let words = vec![("c1".into(), 0usize, vec!["Hi".into(), "there!!!".into()])];
        let got = expand_to_word_ranges(&[sent], &words);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].id, 0);
        assert_eq!(got[1].id, 1);
        let d0 = got[0].last().end - got[0].first().start;
        let d1 = got[1].last().end - got[1].first().start;
        assert!(d1 > d0, "longer word gets more time: {d0} vs {d1}");
        assert!((got[1].last().end - 4.0).abs() < 1e-9);
    }

    #[test]
    fn aligns_words_against_hyp_slug_span() {
        let sent = SentenceRange::new(
            0,
            "c1",
            vec![AudioRange {
                audiofile: "a.mp3".into(),
                start: 0.0,
                end: 2.0,
            }],
        )
        .with_slug_span(0, 11);
        let words = vec![("c1".into(), 0usize, vec!["Hello".into(), "world".into()])];
        let doc = "hello-world";
        let times = |start: usize, end: usize| -> Option<(f64, f64, String)> {
            Some((start as f64 * 0.1, end as f64 * 0.1, "a.mp3".into()))
        };
        let got = expand_to_word_ranges_with(&[sent], &words, Some(doc), Some(&times));
        assert_eq!(got.len(), 2);
        assert!((got[0].first().start - 0.0).abs() < 1e-9);
        assert!((got[0].last().end - 0.5).abs() < 1e-9);
        assert!((got[1].first().start - 0.6).abs() < 1e-9);
        assert!((got[1].last().end - 1.1).abs() < 1e-9);
    }

    #[test]
    fn inject_word_spans_uses_storyteller_ids_when_chapter_set() {
        let xhtml = r#"<p><span id="ch1-s0" class="shelf-mo" epub:type="storyteller:sentence-span">Hello world</span></p>"#;
        let (out, words, next) =
            inject_word_spans(xhtml, &["Hello world".to_string()], 0, Some("ch1"));
        assert_eq!(words, vec![vec!["Hello".to_string(), "world".to_string()]]);
        assert_eq!(next, 2);
        assert!(out.contains(r#"id="ch1-s0-w0""#), "{out}");
        assert!(out.contains(r#"id="ch1-s0-w1""#), "{out}");
        assert!(
            out.contains(r#"epub:type="storyteller:word-span""#),
            "{out}"
        );
        assert!(!out.contains(r#"id="word-0""#), "{out}");
    }

    #[test]
    fn inject_word_spans_falls_back_to_word_n() {
        let xhtml = r#"<p><span id="sentence-0" class="shelf-mo">Hi</span></p>"#;
        let (out, _, _) = inject_word_spans(xhtml, &["Hi".to_string()], 7, None);
        assert!(out.contains(r#"id="word-7""#), "{out}");
        assert!(
            out.contains(r#"epub:type="storyteller:word-span""#),
            "{out}"
        );
    }

    #[test]
    fn expand_ctc_prefers_aligned_clips_when_counts_match() {
        let sent = SentenceRange::new(
            0,
            "c1",
            vec![AudioRange {
                audiofile: "a.mp3".into(),
                start: 0.0,
                end: 4.0,
            }],
        );
        let words = vec![("c1".into(), 0usize, vec!["Hi".into(), "there".into()])];
        let mut ctc = std::collections::HashMap::new();
        ctc.insert(
            ("c1".into(), 0usize),
            vec![
                AudioRange {
                    audiofile: "a.mp3".into(),
                    start: 0.0,
                    end: 1.0,
                },
                AudioRange {
                    audiofile: "a.mp3".into(),
                    start: 1.0,
                    end: 2.5,
                },
            ],
        );
        let got = expand_to_word_ranges_ctc(&[sent], &words, &ctc);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].text_id.as_deref(), Some("c1-s0-w0"));
        assert!((got[0].last().end - 1.0).abs() < 1e-9);
        assert!((got[1].last().end - 2.5).abs() < 1e-9);
    }

    #[test]
    fn echogarden_words_strip_punctuation() {
        let w = split_words("Hello, world!");
        assert_eq!(w, vec!["Hello", "world"]);
    }

    #[test]
    fn echogarden_noun_and_number_tokens() {
        let w = split_words("I like C# and C++ — price $60 or 3.14 today.");
        assert!(w.contains(&"C#".to_string()), "{w:?}");
        assert!(w.contains(&"C++".to_string()), "{w:?}");
        assert!(
            w.contains(&"$60".to_string()) || w.iter().any(|x| x.contains("60")),
            "{w:?}"
        );
        assert!(w.contains(&"3.14".to_string()), "{w:?}");
    }

    #[test]
    fn inject_keeps_punct_between_word_spans() {
        let xhtml = r#"<p><span id="ch1-s0" class="shelf-mo">Hello, world!</span></p>"#;
        let (out, words, _) =
            inject_word_spans(xhtml, &["Hello, world!".to_string()], 0, Some("ch1"));
        assert_eq!(words, vec![vec!["Hello".to_string(), "world".to_string()]]);
        assert!(out.contains(">Hello</span>"), "{out}");
        assert!(out.contains(","), "{out}");
        assert!(out.contains(">world</span>"), "{out}");
        assert!(out.contains("!"), "{out}");
    }

    #[test]
    fn mr_abbrev_is_single_word_token() {
        let toks = tokenize_words("Mr. Smith", "en");
        let words: Vec<_> = toks
            .iter()
            .filter(|t| !t.is_punctuation)
            .map(|t| t.text.as_str())
            .collect();
        assert_eq!(words, vec!["Mr.", "Smith"], "{toks:?}");
    }
}
