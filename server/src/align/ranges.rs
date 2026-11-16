//! Sentence ↔ audio ranges and monotonic enforcement.
//!
//! Port of Storyteller `getSentenceRanges` helpers used after forced alignment,
//! especially `enforceMonotonicAudioRanges` (global timeline + out-of-order keep).

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeState {
    Matched,
    Interpolated,
    Unmatched,
    Dropped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioRange {
    pub audiofile: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SentenceRange {
    pub id: usize,
    pub chapter_id: String,
    pub audio_ranges: Vec<AudioRange>,
    pub state: RangeState,
    /// Absolute slug-doc char span for this match (Storyteller hyp window for word EA).
    pub slug_start: Option<usize>,
    pub slug_end: Option<usize>,
    /// Override SMIL/XHTML fragment (without `#`). None → `{prefix}-{id}`.
    pub text_id: Option<String>,
}

/// Storyteller XHTML/SMIL fragment id: `{chapterId}-s{sentenceIndex}`.
pub fn st_sentence_text_id(chapter_id: &str, sentence_id: usize) -> String {
    format!("{chapter_id}-s{sentence_id}")
}

impl SentenceRange {
    pub fn new(id: usize, chapter_id: impl Into<String>, audio_ranges: Vec<AudioRange>) -> Self {
        assert!(
            !audio_ranges.is_empty(),
            "sentence must have at least one audio range"
        );
        let chapter_id = chapter_id.into();
        let text_id = Some(st_sentence_text_id(&chapter_id, id));
        Self {
            id,
            chapter_id,
            audio_ranges,
            state: RangeState::Matched,
            slug_start: None,
            slug_end: None,
            text_id,
        }
    }

    pub fn with_state(mut self, state: RangeState) -> Self {
        self.state = state;
        self
    }

    pub fn with_slug_span(mut self, start: usize, end: usize) -> Self {
        self.slug_start = Some(start);
        self.slug_end = Some(end);
        self
    }

    pub fn with_text_id(mut self, text_id: impl Into<String>) -> Self {
        self.text_id = Some(text_id.into());
        self
    }

    pub fn with_audio_ranges(&self, audio_ranges: Vec<AudioRange>) -> Self {
        assert!(!audio_ranges.is_empty());
        Self {
            id: self.id,
            chapter_id: self.chapter_id.clone(),
            audio_ranges,
            state: self.state,
            slug_start: self.slug_start,
            slug_end: self.slug_end,
            text_id: self.text_id.clone(),
        }
    }

    pub fn first(&self) -> &AudioRange {
        &self.audio_ranges[0]
    }

    pub fn last(&self) -> &AudioRange {
        self.audio_ranges.last().expect("non-empty")
    }

    pub fn audiofiles(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for c in &self.audio_ranges {
            if !seen.iter().any(|s| s == &c.audiofile) {
                seen.push(c.audiofile.clone());
            }
        }
        seen
    }
}

/// A clip may run this far back past the previous sentence's end before it is
/// treated as out-of-order narration rather than overlap (Storyteller).
const MAX_OVERLAP_SECONDS: f64 = 30.0;

/// Keep successive sentence clips from going backwards through the audiobook
/// by a small amount. Large jumps are out-of-order narration and are kept.
///
/// Faithful port of Storyteller `enforceMonotonicAudioRanges`.
pub fn enforce_monotonic_audio_ranges(
    input: &[SentenceRange],
    files: &[String],
    durations: &HashMap<String, f64>,
) -> Vec<SentenceRange> {
    let mut file_offsets: HashMap<&str, f64> = HashMap::new();
    let mut offset = 0.0;
    for f in files {
        file_offsets.insert(f.as_str(), offset);
        offset += durations.get(f).copied().unwrap_or(0.0);
    }
    let global = |audiofile: &str, time: f64| -> f64 {
        file_offsets.get(audiofile).copied().unwrap_or(0.0) + time
    };

    let mut floor: Option<AudioRange> = None;
    input
        .iter()
        .map(|sentence| {
            let Some(prev) = floor.clone() else {
                floor = Some(sentence.last().clone());
                return sentence.clone();
            };
            let floor_end = global(&prev.audiofile, prev.end);
            let mut kept: Vec<AudioRange> = Vec::new();
            let mut cur_floor = prev;
            for range in &sentence.audio_ranges {
                let start = global(&range.audiofile, range.start);
                let end = global(&range.audiofile, range.end);
                if floor_end - start > MAX_OVERLAP_SECONDS {
                    // Narrated out of order: keep, restart floor from here.
                    kept.push(range.clone());
                    cur_floor = range.clone();
                    continue;
                }
                if end <= floor_end {
                    continue;
                }
                if start < floor_end {
                    kept.push(AudioRange {
                        audiofile: cur_floor.audiofile.clone(),
                        start: cur_floor.end,
                        end: range.end,
                    });
                } else {
                    kept.push(range.clone());
                }
                cur_floor = kept.last().expect("pushed").clone();
            }
            if kept.is_empty() {
                kept.push(AudioRange {
                    audiofile: cur_floor.audiofile.clone(),
                    start: cur_floor.end,
                    end: cur_floor.end,
                });
            }
            floor = kept.last().cloned();
            sentence.with_audio_ranges(kept)
        })
        .collect()
}

/// Nudge zero-length clips so EpubCheck is happy (Storyteller `expandEmptyAudioRanges`).
pub fn expand_empty_sentence_ranges(sentence_ranges: &[SentenceRange]) -> Vec<SentenceRange> {
    let flat: Vec<AudioRange> = sentence_ranges
        .iter()
        .flat_map(|s| s.audio_ranges.iter().cloned())
        .collect();
    let mut expanded: Vec<AudioRange> = Vec::with_capacity(flat.len());
    for range in flat {
        let nudged = if let Some(prev) = expanded.last() {
            if prev.end > range.start && prev.audiofile == range.audiofile {
                AudioRange {
                    audiofile: range.audiofile.clone(),
                    start: prev.end,
                    end: range.end,
                }
            } else {
                range
            }
        } else {
            range
        };
        let expanded_clip = if nudged.end <= nudged.start {
            AudioRange {
                end: nudged.start + 0.001,
                ..nudged
            }
        } else {
            nudged
        };
        expanded.push(expanded_clip);
    }
    let mut cursor = 0usize;
    sentence_ranges
        .iter()
        .map(|s| {
            let count = s.audio_ranges.len();
            let slice = expanded[cursor..cursor + count].to_vec();
            cursor += count;
            s.with_audio_ranges(slice)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sr(id: usize, clips: &[(&str, f64, f64)]) -> SentenceRange {
        SentenceRange::new(
            id,
            "ch",
            clips
                .iter()
                .map(|(f, s, e)| AudioRange {
                    audiofile: (*f).to_string(),
                    start: *s,
                    end: *e,
                })
                .collect(),
        )
    }

    fn files() -> Vec<String> {
        vec!["a.mp3".into(), "b.mp3".into(), "c.mp3".into()]
    }

    fn durations() -> HashMap<String, f64> {
        HashMap::from([
            ("a.mp3".into(), 1761.0),
            ("b.mp3".into(), 3600.0),
            ("c.mp3".into(), 3600.0),
        ])
    }

    #[test]
    fn sentence_range_new_sets_storyteller_chapter_s_text_id() {
        assert_eq!(st_sentence_text_id("ch1", 0), "ch1-s0");
        assert_eq!(st_sentence_text_id("ch1", 12), "ch1-s12");
        let r = SentenceRange::new(
            3,
            "chapter_x",
            vec![AudioRange {
                audiofile: "a.mp3".into(),
                start: 0.0,
                end: 1.0,
            }],
        );
        assert_eq!(r.text_id.as_deref(), Some("chapter_x-s3"));
    }

    #[test]
    fn leaves_monotonic_sequence_unchanged() {
        let input = vec![
            sr(0, &[("a.mp3", 0.0, 5.0)]),
            sr(1, &[("a.mp3", 5.0, 9.0), ("b.mp3", 0.0, 2.0)]),
            sr(2, &[("b.mp3", 2.0, 4.0)]),
        ];
        assert_eq!(
            enforce_monotonic_audio_ranges(&input, &files(), &durations()),
            input
        );
    }

    #[test]
    fn drops_earlier_file_clip_and_trims_overlap() {
        let input = vec![
            sr(0, &[("a.mp3", 1760.3, 1761.0), ("b.mp3", 0.0, 1.24)]),
            sr(1, &[("a.mp3", 1759.0, 1761.0), ("b.mp3", 0.0, 5.58)]),
            sr(2, &[("b.mp3", 5.58, 8.3)]),
        ];
        let got = enforce_monotonic_audio_ranges(&input, &files(), &durations());
        assert_eq!(got[0], input[0]);
        assert_eq!(got[1].audio_ranges.len(), 1);
        assert_eq!(got[1].audio_ranges[0].audiofile, "b.mp3");
        assert!((got[1].audio_ranges[0].start - 1.24).abs() < 1e-9);
        assert!((got[1].audio_ranges[0].end - 5.58).abs() < 1e-9);
        assert_eq!(got[2], input[2]);
    }

    #[test]
    fn parks_zero_length_when_sentence_is_behind() {
        let input = vec![
            sr(0, &[("b.mp3", 10.0, 20.0)]),
            sr(1, &[("b.mp3", 5.0, 15.0)]),
        ];
        let got = enforce_monotonic_audio_ranges(&input, &files(), &durations());
        assert_eq!(got[1].audio_ranges[0].start, 20.0);
        assert_eq!(got[1].audio_ranges[0].end, 20.0);
    }

    #[test]
    fn trims_same_file_overlap() {
        let input = vec![
            sr(0, &[("a.mp3", 0.0, 10.0)]),
            sr(1, &[("a.mp3", 8.0, 12.0)]),
        ];
        let got = enforce_monotonic_audio_ranges(&input, &files(), &durations());
        assert_eq!(got[1].audio_ranges[0].start, 10.0);
        assert_eq!(got[1].audio_ranges[0].end, 12.0);
    }

    #[test]
    fn keeps_out_of_order_narration_block() {
        // The Plague-style: later text narrated earlier in file a.
        let input = vec![
            sr(0, &[("b.mp3", 0.0, 10.0)]),
            sr(1, &[("b.mp3", 10.0, 20.0)]),
            sr(2, &[("a.mp3", 600.0, 610.0)]),
            sr(3, &[("a.mp3", 610.0, 620.0)]),
        ];
        assert_eq!(
            enforce_monotonic_audio_ranges(&input, &files(), &durations()),
            input
        );
    }

    #[test]
    fn trims_small_overlap_after_out_of_order_block() {
        let input = vec![
            sr(0, &[("b.mp3", 0.0, 10.0)]),
            sr(1, &[("a.mp3", 600.0, 610.0)]),
            sr(2, &[("a.mp3", 608.0, 615.0)]),
        ];
        let got = enforce_monotonic_audio_ranges(&input, &files(), &durations());
        assert_eq!(got[0], input[0]);
        assert_eq!(got[1], input[1]);
        assert_eq!(got[2].audio_ranges[0].audiofile, "a.mp3");
        assert!((got[2].audio_ranges[0].start - 610.0).abs() < 1e-9);
        assert!((got[2].audio_ranges[0].end - 615.0).abs() < 1e-9);
    }
}
