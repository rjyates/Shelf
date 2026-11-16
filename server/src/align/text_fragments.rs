//! Text Fragment URL factory (Storyteller `TextFragmentFactory`).
//!
//! Builds minimal `:~:text=` fragments for SMIL `epub:textref` / `<text src>` when
//! `TextRef::TextFragment` is selected. Foliate production default stays id-fragment.
//!
//! shortcut: Unicode scalar (`char`) indexing, not grapheme clusters (ST `runes2`);
//! upgrade if non-EN / ZWJ emoji spans need true grapheme boundaries.

/// Encode one Text Fragment part: `encodeURIComponent` then force `-` / `,`.
pub fn encode_text_fragment_part(part: &str) -> String {
    crate::align::smil::encode_uri_component(part)
        .replace('-', "%2d")
        .replace(',', "%2c")
}

/// Storyteller `TextFragmentFactory` — find a minimal unique `:~:text=` for a span.
pub struct TextFragmentFactory {
    /// Lowercased Unicode scalars of all spans joined.
    runes: Vec<char>,
    spans: Vec<String>,
    span_starts: Vec<usize>,
    /// First-character → positions in `runes`.
    char_positions: std::collections::HashMap<char, Vec<usize>>,
}

impl TextFragmentFactory {
    /// `cased_spans` are lowered with Unicode simple case mapping (EN-safe).
    pub fn new(cased_spans: &[impl AsRef<str>]) -> Self {
        let spans: Vec<String> = cased_spans
            .iter()
            .map(|s| s.as_ref().to_lowercase())
            .collect();
        let joined: String = spans.concat();
        let runes: Vec<char> = joined.chars().collect();

        let mut span_starts = Vec::with_capacity(spans.len());
        let mut start = 0usize;
        for span in &spans {
            span_starts.push(start);
            start += span.chars().count();
        }

        let mut char_positions: std::collections::HashMap<char, Vec<usize>> =
            std::collections::HashMap::new();
        for (i, &ch) in runes.iter().enumerate() {
            char_positions.entry(ch).or_default().push(i);
        }

        Self {
            runes,
            spans,
            span_starts,
            char_positions,
        }
    }

    /// Storyteller `findMinimalFragment` — returns `:~:text=...`.
    pub fn find_minimal_fragment(&self, span_index: usize) -> String {
        let span = self
            .spans
            .get(span_index)
            .unwrap_or_else(|| panic!("Span index {span_index} out of bounds"));
        let start_pos = self.span_starts[span_index];
        let chars: Vec<char> = span.chars().collect();
        if chars.is_empty() {
            return ":~:text=".into();
        }

        let first = chars[0];
        let mut candidates: Vec<usize> = self
            .char_positions
            .get(&first)
            .into_iter()
            .flatten()
            .copied()
            .filter(|&pos| pos < start_pos)
            .collect();

        let mut i = 1usize;
        while i < chars.len() && !candidates.is_empty() {
            let ch = chars[i];
            candidates.retain(|&cand| self.runes.get(cand + i) == Some(&ch));
            i += 1;
        }

        while i < chars.len() && is_letter_or_number(chars[i]) {
            i += 1;
        }

        let start: String = chars[..i].iter().collect();
        let mut fragment = encode_text_fragment_part(&start);

        let mut remaining: Vec<char> = chars[i..].to_vec();
        while remaining.last().is_some_and(|c| !is_letter_or_number(*c)) {
            remaining.pop();
        }

        // ST: shrink `e` until `end = remaining[e..]` occurs only as a suffix.
        let mut e = remaining.len() as isize;
        let mut end = String::new();
        let remaining_span: String = remaining.iter().collect();
        while e >= 0 {
            let e_usize = e as usize;
            let suffix: String = remaining[e_usize..].iter().collect();
            let expected_at = remaining_span.len().saturating_sub(suffix.chars().count());
            // indexOf returns byte index; for EN lowercased ASCII, bytes == chars.
            if remaining_span.find(&suffix) == Some(expected_at) {
                end = suffix;
                break;
            }
            e -= 1;
            if e >= 0 {
                end = remaining[e as usize..].iter().collect();
            }
        }

        while e >= 0
            && remaining
                .get(e as usize)
                .is_some_and(|c| is_letter_or_number(*c))
        {
            end = remaining[e as usize..].iter().collect();
            e -= 1;
        }

        if !end.is_empty() {
            fragment.push(',');
            fragment.push_str(&encode_text_fragment_part(&end));
        }

        if !candidates.is_empty() {
            let mut p = 1usize;
            while p < start_pos {
                let ch = self.runes[start_pos - p];
                candidates.retain(|&cand| cand >= p && self.runes.get(cand - p) == Some(&ch));
                p += 1;
                if candidates.is_empty() {
                    break;
                }
            }

            while p <= start_pos
                && start_pos > p
                && self
                    .runes
                    .get(start_pos - p - 1)
                    .is_some_and(|c| is_letter_or_number(*c))
            {
                p += 1;
            }

            let prefix: String = self.runes[start_pos - p..start_pos].iter().collect();
            fragment = format!("{}-,{}", encode_text_fragment_part(&prefix), fragment);
        }

        format!(":~:text={fragment}")
    }
}

fn is_letter_or_number(c: char) -> bool {
    c.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Spans mirror Echogarden segmentText (trailing space on non-final sentences).
    #[test]
    fn unique_spans_get_minimal_text_fragment() {
        let factory = TextFragmentFactory::new(&[
            "This is a short snippet of text. ",
            "It has several sentences. ",
            "None of them have the same initial or final word.",
        ]);
        assert_eq!(factory.find_minimal_fragment(0), ":~:text=this,text");
        assert_eq!(factory.find_minimal_fragment(1), ":~:text=it,sentences");
        assert_eq!(factory.find_minimal_fragment(2), ":~:text=none,word");
    }

    #[test]
    fn overlapping_start_extends_until_unique() {
        let factory = TextFragmentFactory::new(&[
            "This is a sentence with some words. ",
            "This is another sentence with more words.",
        ]);
        assert_eq!(factory.find_minimal_fragment(0), ":~:text=this,words");
        assert_eq!(
            factory.find_minimal_fragment(1),
            ":~:text=this%20is%20another,words",
        );
    }

    #[test]
    fn mid_sentence_overlap_extends_start() {
        let factory = TextFragmentFactory::new(&[
            "There is a man named John McClane. ",
            "John McClane is unequivocally a badass.",
        ]);
        assert_eq!(factory.find_minimal_fragment(0), ":~:text=there,mcclane");
        assert_eq!(
            factory.find_minimal_fragment(1),
            ":~:text=john%20mcclane%20is,badass",
        );
    }

    #[test]
    fn ambiguous_duplicate_gets_prefix_form() {
        let factory = TextFragmentFactory::new(&[
            "This is a simple sentence. ",
            "This is a slightly more complex sentence. ",
            "This is a simple sentence.",
        ]);
        assert_eq!(factory.find_minimal_fragment(0), ":~:text=this,sentence");
        assert_eq!(
            factory.find_minimal_fragment(2),
            ":~:text=sentence.%20-,this%20is%20a%20simple%20sentence.",
        );
    }

    #[test]
    fn encode_forces_hyphen_and_keeps_encoded_comma() {
        // encodeURIComponent(",") → %2C; only literal `-`/`/` need the extra replace.
        assert_eq!(encode_text_fragment_part("a-b,c"), "a%2db%2Cc");
    }
}
