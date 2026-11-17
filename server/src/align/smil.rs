//! SMIL Media Overlay emission (Storyteller `createMediaOverlay`).
//!
//! Chapter seq → optional `text-range-large` (block map) → per-sentence
//! `text-range-small`. Default `TextRef::IdFragment` uses ST `#{chapterId}-s{N}`
//! (via `SentenceRange.text_id`). `TextRef::TextFragment` uses `:~:text=` maps.

use std::collections::HashMap;

use crate::align::holes::{Hole, HolePosition};
use crate::align::ranges::{RangeState, SentenceRange};
use crate::align::text_fragments::TextFragmentFactory;

/// Storyteller `STORYTELLER_VOCABULARY`.
pub const STORYTELLER_VOCABULARY: &str =
    "https://storyteller-platform.gitlab.io/storyteller/docs/vocabulary";

/// Storyteller `SMIL_PREFIX`.
pub const SMIL_PREFIX: &str =
    "storyteller: https://storyteller-platform.gitlab.io/storyteller/docs/vocabulary";

/// Storyteller `AUDIO_ONLY_TYPE`.
pub const AUDIO_ONLY_TYPE: &str = "storyteller:audio-only";

/// SMIL text reference mode (ST `--text-ref` / Aligner `textRef`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextRef {
    /// Production default: ST `#{chapterId}-s{N}` / word ids from `text_id`.
    #[default]
    IdFragment,
    /// W3C Text Fragments: `#:~:text=...` (+ block `text-range-large`).
    TextFragment,
}

/// Storyteller `stateType(state)` -- `storyteller:{matched|interpolated|unmatched|dropped}`.
pub fn state_type(state: RangeState) -> &'static str {
    match state {
        RangeState::Matched => "storyteller:matched",
        RangeState::Interpolated => "storyteller:interpolated",
        RangeState::Unmatched => "storyteller:unmatched",
        RangeState::Dropped => "storyteller:dropped",
    }
}

/// Storyteller `Epub.formatSmilDuration` -- `HH:MM:SS.mm` SMIL clock for OPF
/// `media:duration` (EPUB3 sec 9.3.5.2).
pub fn format_smil_duration(duration_secs: f64) -> String {
    let duration = duration_secs.max(0.0);
    let hours = (duration / 3600.0).floor() as u64;
    let minutes = (duration / 60.0 - hours as f64 * 60.0).floor() as u64;
    let seconds_and_millis = duration - minutes as f64 * 60.0 - hours as f64 * 3600.0;
    let fixed = format!("{seconds_and_millis:.2}");
    let (seconds, millis) = fixed.split_once('.').unwrap_or((fixed.as_str(), "0"));
    format!("{hours:02}:{minutes:02}:{seconds:0>2}.{millis}")
}

/// Sum of sentence clip lengths plus audio-only hole clips (ST `getChapterDuration`).
pub fn overlay_duration_secs(sentences: &[SentenceRange], holes: &[Hole]) -> f64 {
    let sentence_secs: f64 = sentences
        .iter()
        .flat_map(|s| s.audio_ranges.iter())
        .map(|c| (c.end - c.start).max(0.0))
        .sum();
    let hole_secs: f64 = holes.iter().map(|h| (h.end - h.start).max(0.0)).sum();
    sentence_secs + hole_secs
}

fn position_label(pos: HolePosition) -> &'static str {
    match pos {
        HolePosition::Before => "before",
        HolePosition::After => "after",
    }
}

/// Parse `clipBegin`/`clipEnd` pairs from SMIL (`N.NNNs` clock values).
pub fn parse_smil_clip_durations(smil: &str) -> Vec<f64> {
    let mut durs = Vec::new();
    let mut i = 0usize;
    while i < smil.len() {
        let rest = &smil[i..];
        let Some(b) = rest.find("clipBegin=\"") else {
            break;
        };
        let after_b = i + b + "clipBegin=\"".len();
        let Some(b_end) = smil[after_b..].find('"') else {
            break;
        };
        let begin_raw = &smil[after_b..after_b + b_end];
        let after_begin = after_b + b_end + 1;
        let Some(e_rel) = smil[after_begin..].find("clipEnd=\"") else {
            i = after_begin;
            continue;
        };
        let after_e = after_begin + e_rel + "clipEnd=\"".len();
        let Some(e_end) = smil[after_e..].find('"') else {
            break;
        };
        let end_raw = &smil[after_e..after_e + e_end];
        if let (Some(begin), Some(end)) = (parse_smil_secs(begin_raw), parse_smil_secs(end_raw)) {
            durs.push(end - begin);
        }
        i = after_e + e_end + 1;
    }
    durs
}

fn parse_smil_secs(raw: &str) -> Option<f64> {
    let s = raw.trim().trim_end_matches('s');
    s.parse().ok()
}

/// Clip-duration histogram for Combine QA (Four Lamps / RotK).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmilTimingStats {
    pub count: usize,
    pub median: f64,
    pub under_50ms: usize,
}

impl SmilTimingStats {
    pub fn from_durations(durs: &[f64]) -> Self {
        if durs.is_empty() {
            return Self {
                count: 0,
                median: 0.0,
                under_50ms: 0,
            };
        }
        let mut sorted = durs.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = sorted.len() / 2;
        let median = if sorted.len().is_multiple_of(2) {
            (sorted[mid - 1] + sorted[mid]) / 2.0
        } else {
            sorted[mid]
        };
        let under_50ms = durs.iter().filter(|&&d| d < 0.05).count();
        Self {
            count: durs.len(),
            median,
            under_50ms,
        }
    }

    /// True when timings look collapsed (RotK-class failure: ~all clips <50ms).
    pub fn looks_collapsed(&self, max_under_50ms_frac: f64) -> bool {
        if self.count == 0 {
            return true;
        }
        (self.under_50ms as f64) / (self.count as f64) > max_under_50ms_frac
    }
}

/// JS `encodeURIComponent` on a single path segment (basename only).
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | 0x27 // apostrophe
            | b'('
            | b')' => out.push(b as char),
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

fn audio_basename(audiofile: &str) -> &str {
    audiofile.rsplit('/').next().unwrap_or(audiofile)
}

fn audio_src(audio_dir_prefix: &str, audiofile: &str) -> String {
    format!(
        "{audio_dir_prefix}{}",
        encode_uri_component(audio_basename(audiofile))
    )
}

#[allow(clippy::too_many_arguments)]
fn emit_hole_par(
    body: &mut String,
    chapter_id: &str,
    text_src: &str,
    audio_dir_prefix: &str,
    sentence_id: usize,
    position: HolePosition,
    index: usize,
    hole: &Hole,
) {
    let pos = position_label(position);
    let par_id = format!("{chapter_id}-s{sentence_id}-{pos}{index}");
    let src = audio_src(audio_dir_prefix, &hole.audiofile);
    body.push_str(&format!(
        r#"        <par id="{par_id}" epub:type="{AUDIO_ONLY_TYPE}">
          <text src="{text_src}"/>
          <audio src="{src}" clipBegin="{start:.3}s" clipEnd="{end:.3}s"/>
        </par>
"#,
        start = hole.start,
        end = hole.end,
    ));
}

/// Sentence + optional block fragment maps (ST `buildFragmentMaps`).
#[derive(Debug, Clone)]
pub struct FragmentMaps {
    /// Sentence id → first sentence id of each block → block fragment. None when
    /// id-fragment (ST passes null → small-only SMIL).
    pub sentence_id_to_block_fragment: Option<HashMap<usize, String>>,
    pub sentence_id_to_fragment: HashMap<usize, String>,
}

/// Storyteller `buildFragmentMaps` (sentence granularity; word maps residual).
pub fn build_fragment_maps(
    sentences: &[SentenceRange],
    sentence_texts: &[String],
    fragment_prefix: &str,
    text_ref: TextRef,
) -> FragmentMaps {
    let mut sentence_id_to_fragment: HashMap<usize, String> = sentences
        .iter()
        .map(|r| {
            let frag = r
                .text_id
                .clone()
                .unwrap_or_else(|| format!("{fragment_prefix}-{}", r.id));
            (r.id, frag)
        })
        .collect();

    if text_ref != TextRef::TextFragment || sentence_texts.is_empty() {
        return FragmentMaps {
            sentence_id_to_block_fragment: None,
            sentence_id_to_fragment,
        };
    }

    // ST: split into blocks on sentences whose text contains `\n`.
    let mut blocks: Vec<Vec<String>> = vec![Vec::new()];
    for (i, text) in sentence_texts.iter().enumerate() {
        blocks.last_mut().unwrap().push(text.clone());
        if text.contains('\n') && i + 1 < sentence_texts.len() {
            blocks.push(Vec::new());
        }
    }

    let block_joined: Vec<String> = blocks.iter().map(|b| b.concat()).collect();
    let block_factory = TextFragmentFactory::new(&block_joined);

    let mut sentence_id_to_block_fragment = HashMap::new();
    let mut sentence_range_index = 0usize;
    for (i, block) in blocks.iter().enumerate() {
        if sentence_range_index < sentences.len() {
            sentence_id_to_block_fragment.insert(
                sentences[sentence_range_index].id,
                block_factory.find_minimal_fragment(i),
            );
        }

        let stripped: Vec<String> = block.iter().map(|s| s.replace('\n', "")).collect();
        let sentence_factory = TextFragmentFactory::new(&stripped);
        let end = (sentence_range_index + block.len()).min(sentences.len());
        let block_ranges = &sentences[sentence_range_index..end];
        for (j, range) in block_ranges.iter().enumerate() {
            sentence_id_to_fragment.insert(range.id, sentence_factory.find_minimal_fragment(j));
        }
        sentence_range_index += block.len();
    }

    FragmentMaps {
        sentence_id_to_block_fragment: Some(sentence_id_to_block_fragment),
        sentence_id_to_fragment,
    }
}

fn emit_sentence_seq(
    body: &mut String,
    chapter_id: &str,
    chapter_href: &str,
    audio_dir_prefix: &str,
    sentence: &SentenceRange,
    fragment: &str,
    holes: &[Hole],
) {
    let text_src = format!("{chapter_href}#{fragment}");
    let st = state_type(sentence.state);
    let seq_id = format!("{chapter_id}-s{}", sentence.id);
    body.push_str(&format!(
        r#"      <seq id="{seq_id}" epub:type="text-range-small {st}" epub:textref="{text_src}">
"#
    ));

    let before: Vec<&Hole> = holes
        .iter()
        .filter(|h| {
            h.anchor.sentence_id == sentence.id && h.anchor.position == HolePosition::Before
        })
        .collect();
    for (i, hole) in before.iter().enumerate() {
        emit_hole_par(
            body,
            chapter_id,
            &text_src,
            audio_dir_prefix,
            sentence.id,
            HolePosition::Before,
            i,
            hole,
        );
    }

    for (i, clip) in sentence.audio_ranges.iter().enumerate() {
        let par_id = format!("{chapter_id}-s{}-a{i}", sentence.id);
        let src = audio_src(audio_dir_prefix, &clip.audiofile);
        body.push_str(&format!(
            r#"        <par id="{par_id}" epub:type="{st}">
          <text src="{text_src}"/>
          <audio src="{src}" clipBegin="{start:.3}s" clipEnd="{end:.3}s"/>
        </par>
"#,
            start = clip.start,
            end = clip.end,
        ));
    }

    let after: Vec<&Hole> = holes
        .iter()
        .filter(|h| h.anchor.sentence_id == sentence.id && h.anchor.position == HolePosition::After)
        .collect();
    for (i, hole) in after.iter().enumerate() {
        emit_hole_par(
            body,
            chapter_id,
            &text_src,
            audio_dir_prefix,
            sentence.id,
            HolePosition::After,
            i,
            hole,
        );
    }

    body.push_str("      </seq>\n");
}

/// Build SMIL 3.0 for one chapter (ST `createMediaOverlay`).
/// `fragment_prefix` is `"sentence"` (default) or `"word"` for Foliate ID fragments.
pub fn chapter_smil(
    chapter_id: &str,
    chapter_href: &str,
    audio_dir_prefix: &str,
    sentences: &[SentenceRange],
) -> String {
    chapter_smil_with_prefix(
        chapter_id,
        chapter_href,
        audio_dir_prefix,
        sentences,
        "sentence",
        &[],
    )
}

pub fn chapter_smil_with_prefix(
    chapter_id: &str,
    chapter_href: &str,
    audio_dir_prefix: &str,
    sentences: &[SentenceRange],
    fragment_prefix: &str,
    holes: &[Hole],
) -> String {
    chapter_smil_with_text_ref(
        chapter_id,
        chapter_href,
        audio_dir_prefix,
        sentences,
        fragment_prefix,
        holes,
        TextRef::IdFragment,
        &[],
    )
}

/// Full SMIL path with optional Text Fragment maps + `text-range-large`.
#[allow(clippy::too_many_arguments)] // mirrors ST createMediaOverlay arity
pub fn chapter_smil_with_text_ref(
    chapter_id: &str,
    chapter_href: &str,
    audio_dir_prefix: &str,
    sentences: &[SentenceRange],
    fragment_prefix: &str,
    holes: &[Hole],
    text_ref: TextRef,
    sentence_texts: &[String],
) -> String {
    let maps = build_fragment_maps(sentences, sentence_texts, fragment_prefix, text_ref);

    let mut body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<smil xmlns="http://www.w3.org/ns/SMIL" xmlns:epub="http://www.idpf.org/2007/ops" version="3.0" epub:prefix="{SMIL_PREFIX}">
  <body>
    <seq id="{chapter_id}_overlay" epub:textref="{chapter_href}" epub:type="chapter">
"#
    );

    if let Some(block_map) = &maps.sentence_id_to_block_fragment {
        // ST createTextRangeLargeSequences — nest small seqs under each block.
        // Exclusive end uses next block start / len (ST last-block `length-1` is off-by-one).
        let mut block_starts: Vec<(usize, String)> = block_map
            .iter()
            .map(|(id, frag)| (*id, frag.clone()))
            .collect();
        block_starts.sort_by_key(|(id, _)| *id);

        // Map sentence id → index in `sentences` for slicing.
        let id_to_idx: HashMap<usize, usize> = sentences
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id, i))
            .collect();

        for (bi, (start_id, block_frag)) in block_starts.iter().enumerate() {
            let start_idx = id_to_idx.get(start_id).copied().unwrap_or(0);
            let end_idx = if bi + 1 < block_starts.len() {
                id_to_idx
                    .get(&block_starts[bi + 1].0)
                    .copied()
                    .unwrap_or(sentences.len())
            } else {
                sentences.len()
            };
            let block_href = format!("{chapter_href}#{block_frag}");
            body.push_str(&format!(
                r#"      <seq id="{chapter_id}-b{bi}" epub:type="text-range-large" epub:textref="{block_href}">
"#
            ));
            for sentence in &sentences[start_idx..end_idx] {
                let frag = maps
                    .sentence_id_to_fragment
                    .get(&sentence.id)
                    .map(|s| s.as_str())
                    .unwrap_or("");
                // Indent one more level under large seq — keep existing small indent for KISS.
                emit_sentence_seq(
                    &mut body,
                    chapter_id,
                    chapter_href,
                    audio_dir_prefix,
                    sentence,
                    frag,
                    holes,
                );
            }
            body.push_str("      </seq>\n");
        }
    } else {
        for sentence in sentences {
            let frag = maps
                .sentence_id_to_fragment
                .get(&sentence.id)
                .map(|s| s.as_str())
                .unwrap_or("");
            emit_sentence_seq(
                &mut body,
                chapter_id,
                chapter_href,
                audio_dir_prefix,
                sentence,
                frag,
                holes,
            );
        }
    }

    body.push_str("    </seq>\n  </body>\n</smil>\n");
    body
}

/// Storyteller `createAudioChapterOverlay` — audio-only pars for a listen chapter.
pub fn create_audio_chapter_overlay(
    chapter_id: &str,
    chapter_href: &str,
    heading_id: &str,
    audio_dir_prefix: &str,
    holes: &[Hole],
) -> String {
    let text_src = format!("{chapter_href}#{heading_id}");
    let mut body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<smil xmlns="http://www.w3.org/ns/SMIL" xmlns:epub="http://www.idpf.org/2007/ops" version="3.0" epub:prefix="{SMIL_PREFIX}">
  <body>
    <seq id="{chapter_id}_overlay" epub:textref="{chapter_href}" epub:type="chapter">
"#
    );
    for (index, hole) in holes.iter().enumerate() {
        let par_id = format!("{chapter_id}-a{index}");
        let src = audio_src(audio_dir_prefix, &hole.audiofile);
        body.push_str(&format!(
            r#"      <par id="{par_id}" epub:type="{AUDIO_ONLY_TYPE}">
        <text src="{text_src}"/>
        <audio src="{src}" clipBegin="{start:.3}s" clipEnd="{end:.3}s"/>
      </par>
"#,
            start = hole.start,
            end = hole.end,
        ));
    }
    body.push_str("    </seq>\n  </body>\n</smil>\n");
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::holes::HoleAnchor;
    use crate::align::ranges::AudioRange;

    fn sr(id: usize, start: f64, end: f64) -> SentenceRange {
        SentenceRange::new(
            id,
            "ch1",
            vec![AudioRange {
                audiofile: "Audio/ch1.mp3".into(),
                start,
                end,
            }],
        )
    }

    fn hole(sentence_id: usize, position: HolePosition, start: f64, end: f64) -> Hole {
        Hole {
            audiofile: "Audio/ch1.mp3".into(),
            start,
            end,
            anchor: HoleAnchor {
                chapter_id: "ch1".into(),
                sentence_id,
                position,
            },
            boundary: false,
            between: String::new(),
        }
    }

    #[test]
    fn emits_chapter_seq_text_range_small_prefix_and_state() {
        let sentences = vec![sr(0, 1.5, 3.25).with_state(RangeState::Matched)];
        let smil = chapter_smil("ch1", "../ch1.xhtml", "../Audio/", &sentences);
        assert!(smil.contains(
            "epub:prefix=\"storyteller: https://storyteller-platform.gitlab.io/storyteller/docs/vocabulary\""
        ));
        assert!(smil.contains("id=\"ch1_overlay\""));
        assert!(smil.contains("epub:type=\"chapter\""));
        assert!(smil.contains("epub:type=\"text-range-small storyteller:matched\""));
        assert!(smil.contains("epub:type=\"storyteller:matched\""));
        assert!(smil.contains("clipBegin=\"1.500s\""));
        assert!(smil.contains("clipEnd=\"3.250s\""));
        assert!(smil.contains("../ch1.xhtml#ch1-s0"));
        assert!(smil.contains("src=\"../Audio/ch1.mp3\""));
        assert!(!smil.contains("text-range-large"));
        assert!(!smil.contains(":~:text="));
    }

    #[test]
    fn id_fragment_emits_storyteller_chapter_s_ids() {
        let sentences = vec![sr(0, 0.0, 1.0), sr(1, 1.0, 2.0)];
        let smil = chapter_smil_with_text_ref(
            "ch1",
            "../ch1.xhtml",
            "../Audio/",
            &sentences,
            "sentence",
            &[],
            TextRef::IdFragment,
            &[],
        );
        assert!(smil.contains("#ch1-s0"));
        assert!(smil.contains("#ch1-s1"));
        assert!(!smil.contains(":~:text="));
        assert!(!smil.contains("text-range-large"));
    }

    #[test]
    fn text_fragment_emits_text_urls_and_range_large_for_multi_block() {
        let sentences = vec![sr(0, 0.0, 1.0), sr(1, 1.0, 2.0), sr(2, 2.0, 3.0)];
        // Two blocks: first sentence ends with newline → new block for the rest.
        let texts = vec![
            "First block sentence.\n".into(),
            "Second block opens. ".into(),
            "Second block continues.".into(),
        ];
        let smil = chapter_smil_with_text_ref(
            "ch1",
            "../ch1.xhtml",
            "../Audio/",
            &sentences,
            "sentence",
            &[],
            TextRef::TextFragment,
            &texts,
        );
        assert!(smil.contains("#:~:text="), "{smil}");
        assert!(smil.contains("epub:type=\"text-range-large\""), "{smil}");
        assert!(smil.contains("id=\"ch1-b0\""), "{smil}");
        assert!(smil.contains("id=\"ch1-b1\""), "{smil}");
        assert!(smil.contains("text-range-small"), "{smil}");
        // Foliate id fragments must not appear in text-fragment mode.
        assert!(!smil.contains("#sentence-"), "{smil}");
    }

    #[test]
    fn encodes_space_in_audio_basename() {
        let mut s = sr(0, 0.0, 1.0);
        s.audio_ranges[0].audiofile = "Audio/track 1.mp3".into();
        let smil = chapter_smil("ch1", "../ch1.xhtml", "../Audio/", &[s]);
        assert!(smil.contains("src=\"../Audio/track%201.mp3\""), "{smil}");
    }

    #[test]
    fn emits_audio_only_pars_before_and_after_inside_sentence_seq() {
        let sentences = vec![sr(0, 10.0, 12.0)];
        let holes = vec![
            hole(0, HolePosition::Before, 5.0, 9.0),
            hole(0, HolePosition::After, 12.5, 20.0),
        ];
        let smil = chapter_smil_with_prefix(
            "ch1",
            "../ch1.xhtml",
            "../Audio/",
            &sentences,
            "sentence",
            &holes,
        );
        assert!(smil.contains("epub:type=\"storyteller:audio-only\""));
        assert!(smil.contains("id=\"ch1-s0-before0\""));
        assert!(smil.contains("clipBegin=\"5.000s\""));
        assert!(smil.contains("clipEnd=\"9.000s\""));
        assert!(smil.contains("id=\"ch1-s0-after0\""));
        assert!(smil.contains("clipBegin=\"12.500s\""));
        assert!(smil.contains("clipEnd=\"20.000s\""));
        let before = smil.find("ch1-s0-before0").unwrap();
        let sentence = smil.find("id=\"ch1-s0-a0\"").unwrap();
        let after = smil.find("ch1-s0-after0").unwrap();
        assert!(before < sentence && sentence < after);
        let seq_open = smil.find("text-range-small").unwrap();
        let seq_close = smil.rfind("</seq>").unwrap();
        assert!(seq_open < before && after < seq_close);
    }

    #[test]
    fn overlay_duration_includes_hole_clips() {
        let sentences = vec![sr(0, 0.0, 2.0)];
        let holes = vec![hole(0, HolePosition::After, 2.0, 7.0)];
        assert!((overlay_duration_secs(&sentences, &holes) - 7.0).abs() < 1e-9);
        assert!((overlay_duration_secs(&sentences, &[]) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn format_smil_duration_matches_storyteller() {
        assert_eq!(format_smil_duration(0.0), "00:00:00.00");
        assert_eq!(format_smil_duration(3.5), "00:00:03.50");
        assert_eq!(format_smil_duration(62.04), "00:01:02.04");
        assert_eq!(format_smil_duration(3723.0), "01:02:03.00");
    }

    #[test]
    fn encode_uri_component_space_and_hash() {
        assert_eq!(encode_uri_component("a b"), "a%20b");
        assert_eq!(encode_uri_component("a#b?c"), "a%23b%3Fc");
    }

    #[test]
    fn smil_histogram_rejects_collapsed_near_zero_clips() {
        // Healthy Four Lamps-ish: median seconds, few <50ms.
        let healthy: Vec<f64> = (0..90)
            .map(|i| {
                if i == 0 {
                    0.04
                } else {
                    2.5 + (i % 5) as f64 * 0.1
                }
            })
            .collect();
        let ok = SmilTimingStats::from_durations(&healthy);
        assert_eq!(ok.count, 90);
        assert!(ok.median > 1.0, "median={}", ok.median);
        assert_eq!(ok.under_50ms, 1);
        assert!(!ok.looks_collapsed(0.25));

        // RotK-class collapse: nearly every clip under 50ms.
        let collapsed: Vec<f64> = (0..100).map(|i| if i < 99 { 0.001 } else { 1.0 }).collect();
        let bad = SmilTimingStats::from_durations(&collapsed);
        assert!(bad.looks_collapsed(0.25));
        assert!(bad.median < 0.05, "median={}", bad.median);
    }

    #[test]
    fn parse_smil_clip_durations_from_overlay() {
        let smil = r#"<par><audio clipBegin="1.000s" clipEnd="3.500s"/>
<audio clipBegin="3.500s" clipEnd="3.520s"/></par>"#;
        let durs = parse_smil_clip_durations(smil);
        assert_eq!(durs.len(), 2);
        assert!((durs[0] - 2.5).abs() < 1e-9);
        assert!((durs[1] - 0.02).abs() < 1e-9);
        let stats = SmilTimingStats::from_durations(&durs);
        assert_eq!(stats.under_50ms, 1);
        // Gate threshold 0.25: 1/2 clips under 50ms → collapsed.
        assert!(stats.looks_collapsed(0.25));
        assert!(!stats.looks_collapsed(0.6));
    }
}
