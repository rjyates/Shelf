//! Safe chapter cuts via silence (Storyteller `process/ranges.ts`).
//!
//! ST uses ghost-story VAD; Shelf uses ffmpeg `silencedetect` (or an injected
//! silence list for tests / callers). CTC production may not call this yet —
//! the API exists for Whisper preprocess and residual parity.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Gaps shorter than this are breaths, not sentence breaks (ST `MIN_SILENCE_SECONDS`).
pub const MIN_SILENCE_SECONDS: f64 = 0.25;

/// How far a chapter mark may move to reach silence (ST `CHAPTER_CUT_MAX_DISTANCE_SECONDS`).
pub const CHAPTER_CUT_MAX_DISTANCE_SECONDS: f64 = 10.0;

/// Audio inspected on either side of a chapter mark (ST `CHAPTER_CUT_WINDOW_SECONDS`).
pub const CHAPTER_CUT_WINDOW_SECONDS: f64 = 20.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SilenceRange {
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChapterAudioRange {
    pub filepath: String,
    pub title: Option<String>,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutReason {
    InSilence,
    Moved,
    NoSilenceNearby,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChapterCut {
    pub cut: f64,
    pub reason: CutReason,
}

/// Pick where to cut for a chapter mark given nearby silences (ST `chooseChapterCut`).
pub fn choose_chapter_cut(
    mark: f64,
    silences: &[SilenceRange],
    max_distance: f64,
    min_silence: f64,
) -> ChapterCut {
    let candidates: Vec<&SilenceRange> = silences
        .iter()
        .filter(|s| s.end - s.start >= min_silence)
        .collect();

    if candidates.iter().any(|s| s.start <= mark && mark <= s.end) {
        return ChapterCut {
            cut: mark,
            reason: CutReason::InSilence,
        };
    }

    let distance = |s: &SilenceRange| {
        if mark < s.start {
            s.start - mark
        } else {
            mark - s.end
        }
    };

    let mut nearest: Option<&SilenceRange> = None;
    for silence in &candidates {
        if nearest.is_none_or(|n| distance(silence) < distance(n)) {
            nearest = Some(silence);
        }
    }

    match nearest {
        Some(n) if distance(n) <= max_distance => ChapterCut {
            cut: (n.start + n.end) / 2.0,
            reason: CutReason::Moved,
        },
        _ => ChapterCut {
            cut: mark,
            reason: CutReason::NoSilenceNearby,
        },
    }
}

/// Move shared same-file chapter cuts onto the nearest silence (ST `moveChapterCutsToSilence`).
///
/// `find_silences(filepath, from, to)` returns absolute-second silence ranges.
pub fn move_chapter_cuts_to_silence<F: FnMut(&str, f64, f64) -> Vec<SilenceRange>>(
    ranges: &mut [ChapterAudioRange],
    mut find_silences: F,
) {
    for i in 0..ranges.len().saturating_sub(1) {
        let (left, right) = ranges.split_at_mut(i + 1);
        let range = &mut left[i];
        let next = &mut right[0];
        if range.filepath != next.filepath || (range.end - next.start).abs() > 1e-9 {
            continue;
        }
        let mark = range.end;
        let from = (mark - CHAPTER_CUT_WINDOW_SECONDS).max(range.start);
        let to = (mark + CHAPTER_CUT_WINDOW_SECONDS).min(next.end);
        let silences = find_silences(&range.filepath, from, to);
        let result = choose_chapter_cut(
            mark,
            &silences,
            CHAPTER_CUT_MAX_DISTANCE_SECONDS,
            MIN_SILENCE_SECONDS,
        );
        match result.reason {
            CutReason::InSilence | CutReason::NoSilenceNearby => {}
            CutReason::Moved => {
                range.end = result.cut;
                next.start = result.cut;
            }
        }
    }
}

/// Split a long track/chapter on the longest pause near each max_seconds boundary
/// (ST `getSafeRanges`). `find_silences` searches `[approx-120, approx]`.
pub fn get_safe_ranges<F: FnMut(&str, f64, f64) -> Vec<SilenceRange>>(
    filepath: &str,
    duration: f64,
    max_seconds: f64,
    start: f64,
    mut find_silences: F,
) -> Vec<(f64, f64)> {
    if max_seconds <= 0.0 || duration <= 0.0 {
        return vec![(start, start + duration.max(0.0))];
    }
    let mut ranges = vec![(start, duration + start)];
    // ST: `for (let i = 0; i + 1 < duration / maxSeconds; i++)`
    let mut i = 0usize;
    while (i as f64) + 1.0 < duration / max_seconds {
        let approx = start + max_seconds * (i as f64 + 1.0);
        let silences = find_silences(filepath, approx - 120.0, approx);
        if silences.is_empty() {
            break;
        }
        let best = silences
            .iter()
            .max_by(|a, b| {
                (a.end - a.start)
                    .partial_cmp(&(b.end - b.start))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .expect("non-empty");
        let last = ranges.last_mut().expect("seeded");
        last.1 = best.start;
        ranges.push((best.end, duration + start));
        i += 1;
    }
    ranges
}

/// ST `getSafeChapterRanges`: nudge chapter marks to silence, then split long chapters.
pub fn get_safe_chapter_ranges<F: FnMut(&str, f64, f64) -> Vec<SilenceRange>>(
    input: &str,
    duration: f64,
    chapters: &[ChapterAudioRange],
    max_seconds: f64,
    mut find_silences: F,
) -> Vec<ChapterAudioRange> {
    if chapters.is_empty() {
        return get_safe_ranges(input, duration, max_seconds, 0.0, &mut find_silences)
            .into_iter()
            .map(|(start, end)| ChapterAudioRange {
                filepath: input.to_string(),
                title: None,
                start,
                end,
            })
            .collect();
    }

    let mut initial: Vec<ChapterAudioRange> = chapters.to_vec();
    move_chapter_cuts_to_silence(&mut initial, &mut find_silences);

    let mut out = Vec::new();
    for range in initial {
        let chapter_duration = range.end - range.start;
        if chapter_duration <= max_seconds {
            out.push(range);
            continue;
        }
        let splits = get_safe_ranges(
            &range.filepath,
            chapter_duration,
            max_seconds,
            range.start,
            &mut find_silences,
        );
        for (start, end) in splits {
            out.push(ChapterAudioRange {
                filepath: range.filepath.clone(),
                title: range.title.clone(),
                start,
                end,
            });
        }
    }
    out
}

/// Parse ffmpeg `silencedetect` log lines into absolute ranges (add `offset` yourself).
pub fn parse_silencedetect_log(log: &str) -> Vec<SilenceRange> {
    let mut out = Vec::new();
    let mut start: Option<f64> = None;
    for line in log.lines() {
        if let Some(rest) = line.split("silence_start:").nth(1) {
            if let Ok(v) = rest.split_whitespace().next().unwrap_or("").parse() {
                start = Some(v);
            }
        } else if let Some(rest) = line.split("silence_end:").nth(1) {
            if let Some(s) = start.take() {
                if let Ok(e) = rest.split_whitespace().next().unwrap_or("").parse() {
                    out.push(SilenceRange { start: s, end: e });
                }
            }
        }
    }
    out
}

fn ffmpeg_bin() -> PathBuf {
    std::env::var_os("SHELF_AAX_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

/// Probe `[from, to]` with ffmpeg silencedetect. Empty when ffmpeg fails or window is empty.
pub fn find_silences_ffmpeg(input: &Path, from: f64, to: f64) -> Result<Vec<SilenceRange>, String> {
    if to <= from {
        return Ok(Vec::new());
    }
    let filter = format!("silencedetect=noise=-30dB:d={MIN_SILENCE_SECONDS}");
    let output = Command::new(ffmpeg_bin())
        .args([
            "-hide_banner",
            "-nostats",
            "-ss",
            &format!("{from:.3}"),
            "-to",
            &format!("{to:.3}"),
            "-i",
            input.to_str().ok_or("non-utf8 audio path")?,
            "-af",
            &filter,
            "-f",
            "null",
            "-",
        ])
        .output()
        .map_err(|e| format!("ffmpeg missing or failed: {e}"))?;
    let log = String::from_utf8_lossy(&output.stderr);
    let mut ranges = parse_silencedetect_log(&log);
    for r in &mut ranges {
        r.start += from;
        r.end += from;
    }
    Ok(ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_mark_already_in_silence() {
        assert_eq!(
            choose_chapter_cut(
                100.0,
                &[
                    SilenceRange {
                        start: 90.0,
                        end: 91.0,
                    },
                    SilenceRange {
                        start: 98.0,
                        end: 103.0,
                    },
                ],
                CHAPTER_CUT_MAX_DISTANCE_SECONDS,
                MIN_SILENCE_SECONDS,
            ),
            ChapterCut {
                cut: 100.0,
                reason: CutReason::InSilence,
            }
        );
    }

    #[test]
    fn moves_mark_into_preceding_pause() {
        let cut = choose_chapter_cut(
            2570.19,
            &[
                SilenceRange {
                    start: 2566.5,
                    end: 2569.94,
                },
                SilenceRange {
                    start: 2571.96,
                    end: 2572.79,
                },
            ],
            CHAPTER_CUT_MAX_DISTANCE_SECONDS,
            MIN_SILENCE_SECONDS,
        );
        assert_eq!(cut.reason, CutReason::Moved);
        assert!((cut.cut - (2566.5 + 2569.94) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn moves_mark_forward_into_following_pause() {
        assert_eq!(
            choose_chapter_cut(
                100.0,
                &[
                    SilenceRange {
                        start: 92.0,
                        end: 94.0,
                    },
                    SilenceRange {
                        start: 101.5,
                        end: 103.5,
                    },
                ],
                CHAPTER_CUT_MAX_DISTANCE_SECONDS,
                MIN_SILENCE_SECONDS,
            ),
            ChapterCut {
                cut: 102.5,
                reason: CutReason::Moved,
            }
        );
    }

    #[test]
    fn measures_distance_to_nearest_edge_not_midpoint() {
        assert_eq!(
            choose_chapter_cut(
                100.0,
                &[
                    SilenceRange {
                        start: 90.0,
                        end: 99.5,
                    },
                    SilenceRange {
                        start: 101.0,
                        end: 101.5,
                    },
                ],
                CHAPTER_CUT_MAX_DISTANCE_SECONDS,
                MIN_SILENCE_SECONDS,
            ),
            ChapterCut {
                cut: 94.75,
                reason: CutReason::Moved,
            }
        );
    }

    #[test]
    fn ignores_gaps_shorter_than_min_silence() {
        assert_eq!(
            choose_chapter_cut(
                100.0,
                &[
                    SilenceRange {
                        start: 99.9,
                        end: 100.1,
                    },
                    SilenceRange {
                        start: 102.0,
                        end: 104.0,
                    },
                ],
                CHAPTER_CUT_MAX_DISTANCE_SECONDS,
                MIN_SILENCE_SECONDS,
            ),
            ChapterCut {
                cut: 103.0,
                reason: CutReason::Moved,
            }
        );
    }

    #[test]
    fn keeps_mark_when_no_silence_within_reach() {
        assert_eq!(
            choose_chapter_cut(
                100.0,
                &[
                    SilenceRange {
                        start: 80.0,
                        end: 82.0,
                    },
                    SilenceRange {
                        start: 115.0,
                        end: 117.0,
                    },
                ],
                CHAPTER_CUT_MAX_DISTANCE_SECONDS,
                MIN_SILENCE_SECONDS,
            ),
            ChapterCut {
                cut: 100.0,
                reason: CutReason::NoSilenceNearby,
            }
        );
    }

    #[test]
    fn move_cuts_uses_synthetic_silences() {
        let mut ranges = vec![
            ChapterAudioRange {
                filepath: "a.m4b".into(),
                title: Some("1".into()),
                start: 0.0,
                end: 100.0,
            },
            ChapterAudioRange {
                filepath: "a.m4b".into(),
                title: Some("2".into()),
                start: 100.0,
                end: 200.0,
            },
        ];
        move_chapter_cuts_to_silence(&mut ranges, |_path, from, to| {
            assert!(from <= 100.0 && to >= 100.0);
            vec![SilenceRange {
                start: 101.0,
                end: 103.0,
            }]
        });
        assert!((ranges[0].end - 102.0).abs() < 1e-9);
        assert!((ranges[1].start - 102.0).abs() < 1e-9);
    }

    #[test]
    fn get_safe_ranges_splits_on_longest_pause() {
        let ranges = get_safe_ranges("t.mp3", 3600.0, 1800.0, 0.0, |_p, from, to| {
            // Window for first approx cut at 1800: [1680, 1800]
            assert!((from - 1680.0).abs() < 1e-6);
            assert!((to - 1800.0).abs() < 1e-6);
            vec![
                SilenceRange {
                    start: 1700.0,
                    end: 1701.0,
                },
                SilenceRange {
                    start: 1790.0,
                    end: 1795.0,
                },
            ]
        });
        assert_eq!(ranges.len(), 2);
        assert!((ranges[0].0 - 0.0).abs() < 1e-9);
        assert!((ranges[0].1 - 1790.0).abs() < 1e-9);
        assert!((ranges[1].0 - 1795.0).abs() < 1e-9);
        assert!((ranges[1].1 - 3600.0).abs() < 1e-9);
    }

    #[test]
    fn parse_silencedetect_log_pairs_start_end() {
        let log = r#"
[silencedetect @ 0x1] silence_start: 1.25
[silencedetect @ 0x1] silence_end: 2.5 | silence_duration: 1.25
[silencedetect @ 0x1] silence_start: 10.0
[silencedetect @ 0x1] silence_end: 10.5 | silence_duration: 0.5
"#;
        let got = parse_silencedetect_log(log);
        assert_eq!(
            got,
            vec![
                SilenceRange {
                    start: 1.25,
                    end: 2.5,
                },
                SilenceRange {
                    start: 10.0,
                    end: 10.5,
                },
            ]
        );
    }

    #[test]
    fn get_safe_chapter_ranges_empty_chapters_falls_back() {
        let out = get_safe_chapter_ranges("x.mp3", 100.0, &[], 200.0, |_, _, _| Vec::new());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].filepath, "x.mp3");
        assert!((out[0].start - 0.0).abs() < 1e-9);
        assert!((out[0].end - 100.0).abs() < 1e-9);
    }
}
