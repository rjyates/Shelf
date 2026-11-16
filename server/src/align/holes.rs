//! Audio holes between matched sentences (Storyteller `holes.ts` `collapseRanges`).

use crate::align::ranges::{AudioRange, RangeState, SentenceRange};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangeBehaviour {
    pub gap_seconds: Option<f64>,
    pub audio_chapter_seconds: Option<f64>,
    pub drop_unread: bool,
}

impl RangeBehaviour {
    pub const LEGACY: Self = Self {
        gap_seconds: None,
        audio_chapter_seconds: None,
        drop_unread: false,
    };

    pub const DEFAULT: Self = Self {
        gap_seconds: Some(5.0),
        audio_chapter_seconds: Some(300.0),
        drop_unread: false,
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoleAnchor {
    pub chapter_id: String,
    pub sentence_id: usize,
    pub position: HolePosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolePosition {
    Before,
    After,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hole {
    pub audiofile: String,
    pub start: f64,
    pub end: f64,
    pub anchor: HoleAnchor,
    pub boundary: bool,
    pub between: String,
}

const DROPPED_WIDTH: f64 = 0.001;

fn slot_key(chapter_id: &str, id: usize) -> String {
    format!("{chapter_id}\0{id}")
}

fn between(prev: Option<&SentenceRange>, next: Option<&SentenceRange>) -> String {
    let left = prev
        .map(|r| slot_key(&r.chapter_id, r.id))
        .unwrap_or_default();
    let right = next
        .map(|r| slot_key(&r.chapter_id, r.id))
        .unwrap_or_default();
    format!("{left}|{right}")
}

fn anchor_after(range: &SentenceRange) -> HoleAnchor {
    HoleAnchor {
        chapter_id: range.chapter_id.clone(),
        sentence_id: range.id,
        position: HolePosition::After,
    }
}

fn anchor_before(range: &SentenceRange) -> HoleAnchor {
    HoleAnchor {
        chapter_id: range.chapter_id.clone(),
        sentence_id: range.id,
        position: HolePosition::Before,
    }
}

/// Collapse tiny gaps and emit holes for large ones (Storyteller `collapseRanges`).
pub fn collapse_ranges(
    ranges: &[SentenceRange],
    audiofiles: &[String],
    durations: &std::collections::HashMap<String, f64>,
    behaviour: RangeBehaviour,
) -> (Vec<SentenceRange>, Vec<Hole>) {
    let gap = behaviour.gap_seconds;
    let mut holes: Vec<Hole> = Vec::new();
    let mut out: Vec<Vec<AudioRange>> = ranges.iter().map(|r| r.audio_ranges.clone()).collect();

    let flow: Vec<usize> = ranges
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            if behaviour.drop_unread && r.state == RangeState::Dropped {
                None
            } else {
                Some(i)
            }
        })
        .collect();

    struct Clip {
        owner: usize,
        clip_idx: usize,
    }
    let clips: Vec<Clip> = flow
        .iter()
        .flat_map(|&owner| (0..out[owner].len()).map(move |clip_idx| Clip { owner, clip_idx }))
        .collect();

    let duration = |file: &str| durations.get(file).copied().unwrap_or(0.0);
    let is_hole = |seconds: f64| gap.is_some_and(|g| seconds > g);

    for (c, clip_ref) in clips.iter().enumerate() {
        let owner = clip_ref.owner;
        let range = &ranges[owner];
        let clip = out[owner][clip_ref.clip_idx].clone();
        let prev = c.checked_sub(1).map(|i| &clips[i]);
        let next = clips.get(c + 1);
        let prev_range = prev.and_then(|p| {
            if p.owner != owner {
                Some(&ranges[p.owner])
            } else {
                None
            }
        });
        let next_range = next.and_then(|n| {
            if n.owner != owner {
                Some(&ranges[n.owner])
            } else {
                None
            }
        });
        let same_sentence_before = prev.is_some_and(|p| p.owner == owner);
        let same_sentence_after = next.is_some_and(|n| n.owner == owner);

        if prev.is_none()
            || out[prev.unwrap().owner][prev.unwrap().clip_idx].audiofile != clip.audiofile
        {
            if !same_sentence_before && is_hole(clip.start) {
                holes.push(Hole {
                    audiofile: clip.audiofile.clone(),
                    start: 0.0,
                    end: clip.start,
                    anchor: anchor_before(range),
                    boundary: prev_range.is_none_or(|p| p.chapter_id != range.chapter_id),
                    between: between(prev_range, Some(range)),
                });
            } else {
                out[owner][clip_ref.clip_idx].start = 0.0;
            }
        }

        let clip = out[owner][clip_ref.clip_idx].clone();
        let next_same_file = next.and_then(|n| {
            let nclip = &out[n.owner][n.clip_idx];
            if nclip.audiofile == clip.audiofile {
                Some(nclip.start)
            } else {
                None
            }
        });
        match next_same_file {
            None => {
                let end = duration(&clip.audiofile);
                if !same_sentence_after && is_hole(end - clip.end) {
                    holes.push(Hole {
                        audiofile: clip.audiofile.clone(),
                        start: clip.end,
                        end,
                        anchor: anchor_after(range),
                        boundary: next_range.is_none_or(|n| n.chapter_id != range.chapter_id),
                        between: between(Some(range), next_range),
                    });
                } else {
                    out[owner][clip_ref.clip_idx].end = end;
                }
            }
            Some(next_start) => {
                // Out-of-order narration (e.g. title-page false-match at EOF
                // followed by real chapter at t≈0): never shrink end backward to
                // next_start — that inverted every later clip into EOF micros.
                if next_start < clip.end {
                    continue;
                }
                if !same_sentence_after && is_hole(next_start - clip.end) {
                    holes.push(Hole {
                        audiofile: clip.audiofile.clone(),
                        start: clip.end,
                        end: next_start,
                        anchor: anchor_after(range),
                        boundary: next_range.is_some_and(|n| n.chapter_id != range.chapter_id),
                        between: between(Some(range), next_range),
                    });
                } else {
                    out[owner][clip_ref.clip_idx].end = next_start;
                }
            }
        }
    }

    if behaviour.drop_unread {
        place_dropped(&mut out, ranges, &flow, &mut holes);
    }

    let referenced: std::collections::HashSet<String> =
        ranges.iter().flat_map(|r| r.audiofiles()).collect();
    let flow_ranges: Vec<&SentenceRange> = flow.iter().map(|&i| &ranges[i]).collect();
    for (index, audiofile) in audiofiles.iter().enumerate() {
        if referenced.contains(audiofile) || duration(audiofile) <= 0.0 {
            continue;
        }
        if gap.is_none() {
            continue;
        }
        let before = &audiofiles[..index];
        let prev = flow_ranges
            .iter()
            .rev()
            .find(|r| before.iter().any(|f| f == &r.last().audiofile))
            .copied();
        let next = if let Some(p) = prev {
            let idx = flow_ranges.iter().position(|r| std::ptr::eq(*r, p));
            idx.and_then(|i| flow_ranges.get(i + 1).copied())
        } else {
            flow_ranges.first().copied()
        };
        let anchor = match prev {
            Some(p) => anchor_after(p),
            None => match next {
                Some(n) => anchor_before(n),
                None => continue,
            },
        };
        holes.push(Hole {
            audiofile: audiofile.clone(),
            start: 0.0,
            end: duration(audiofile),
            anchor,
            boundary: true,
            between: between(prev, next),
        });
    }

    let result_ranges: Vec<SentenceRange> = ranges
        .iter()
        .enumerate()
        .map(|(i, r)| r.with_audio_ranges(out[i].clone()))
        .collect();
    let holes = sort_holes(holes, audiofiles);
    (result_ranges, holes)
}

fn place_dropped(
    out: &mut [Vec<AudioRange>],
    ranges: &[SentenceRange],
    flow: &[usize],
    holes: &mut [Hole],
) {
    let flow_set: std::collections::HashSet<usize> = flow.iter().copied().collect();
    let mut i = 0usize;
    while i < out.len() {
        if flow_set.contains(&i) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < out.len() && !flow_set.contains(&j) {
            j += 1;
        }
        let prev_index = flow.iter().rev().copied().find(|&f| f < i);
        let next_index = flow.iter().copied().find(|&f| f > j.saturating_sub(1));
        let file = ranges[i].first().audiofile.clone();
        let next_clip_info =
            next_index.and_then(|ni| out[ni].first().map(|c| (c.audiofile.clone(), c.start)));
        let prev_clip_info =
            prev_index.and_then(|pi| out[pi].last().map(|c| (c.audiofile.clone(), c.end, pi)));

        let host = if let Some((nf, ns)) = &next_clip_info {
            if nf == &file {
                Some((file.clone(), *ns, None::<usize>))
            } else if let Some((pf, pe, pi)) = &prev_clip_info {
                if pf == &file {
                    Some((file.clone(), *pe, Some(*pi)))
                } else {
                    None
                }
            } else {
                None
            }
        } else if let Some((pf, pe, pi)) = &prev_clip_info {
            if pf == &file {
                Some((file.clone(), *pe, Some(*pi)))
            } else {
                None
            }
        } else {
            None
        };

        if let Some((host_file, host_end, shrink_pi)) = host {
            let width = (j - i) as f64 * DROPPED_WIDTH;
            if let Some(pi) = shrink_pi {
                if let Some(clip) = out[pi].last_mut() {
                    clip.end = clip.start.max(host_end - width);
                }
            } else if let Some((pf, _, pi)) = prev_clip_info {
                if pf == host_file {
                    if let Some(clip) = out[pi].last_mut() {
                        clip.end = clip.start.max(clip.end.min(host_end - width));
                    }
                }
            }
            for hole in holes.iter_mut() {
                if hole.audiofile == host_file && (hole.end - host_end).abs() < 1e-12 {
                    hole.end = hole.start.max(host_end - width);
                }
            }
            let mut cursor = host_end - width;
            for slot in out.iter_mut().take(j).skip(i) {
                *slot = vec![AudioRange {
                    audiofile: host_file.clone(),
                    start: cursor,
                    end: cursor + DROPPED_WIDTH,
                }];
                cursor += DROPPED_WIDTH;
            }
        }
        i = j;
    }
}

fn sort_holes(mut holes: Vec<Hole>, audiofiles: &[String]) -> Vec<Hole> {
    let order: std::collections::HashMap<&str, usize> = audiofiles
        .iter()
        .enumerate()
        .map(|(i, f)| (f.as_str(), i))
        .collect();
    holes.sort_by(|a, b| {
        let oa = order.get(a.audiofile.as_str()).copied().unwrap_or(0);
        let ob = order.get(b.audiofile.as_str()).copied().unwrap_or(0);
        oa.cmp(&ob).then(a.start.partial_cmp(&b.start).unwrap())
    });
    holes
}

/// Storyteller `ExistingChapter` — excluded spine item reusable as an audio chapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingChapter {
    pub id: String,
    pub title: Option<String>,
}

/// Storyteller `AudioChapter` — a run of long holes that can become a listen chapter.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioChapter {
    pub anchor: HoleAnchor,
    pub holes: Vec<Hole>,
    pub title: Option<String>,
    pub anchor_title: Option<String>,
    pub existing: Option<ExistingChapter>,
    /// Sum of hole lengths (derived; ST does not store this on the type).
    pub duration_secs: f64,
}

/// Storyteller `SpineEntryLike` for `audioChapterSpine`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpineEntryLike {
    pub index: usize,
    pub chapter_id: String,
    pub title: Option<String>,
    /// `"aligned"` or `"excluded"`.
    pub status: String,
}

/// Storyteller `audioChapterSpine(pending)`.
#[derive(Debug, Clone)]
pub struct AudioChapterSpine {
    items: Vec<SpineEntryLike>,
    indices: std::collections::HashMap<String, usize>,
    titles: std::collections::HashMap<String, Option<String>>,
}

impl AudioChapterSpine {
    pub fn from_pending(pending: impl IntoIterator<Item = SpineEntryLike>) -> Self {
        let mut items: Vec<SpineEntryLike> = pending.into_iter().collect();
        items.sort_by_key(|e| e.index);
        let indices = items
            .iter()
            .map(|e| (e.chapter_id.clone(), e.index))
            .collect();
        let titles = items
            .iter()
            .map(|e| (e.chapter_id.clone(), e.title.clone()))
            .collect();
        Self {
            items,
            indices,
            titles,
        }
    }

    pub fn chapter_title(&self, chapter_id: &str) -> Option<String> {
        self.titles.get(chapter_id).cloned().flatten()
    }

    pub fn excluded_between(&self, prev: Option<&str>, next: Option<&str>) -> Vec<ExistingChapter> {
        let from = match prev {
            None => Some(isize::MIN),
            Some(id) => self.indices.get(id).map(|&i| i as isize),
        };
        let to = match next {
            None => Some(isize::MAX),
            Some(id) => self.indices.get(id).map(|&i| i as isize),
        };
        let (Some(from), Some(to)) = (from, to) else {
            return Vec::new();
        };
        self.items
            .iter()
            .filter(|item| {
                item.status == "excluded"
                    && (item.index as isize) > from
                    && (item.index as isize) < to
            })
            .map(|item| ExistingChapter {
                id: item.chapter_id.clone(),
                title: item.title.clone(),
            })
            .collect()
    }
}

/// Storyteller `AudioChapterPlan`.
#[derive(Debug, Clone)]
pub struct AudioChapterPlan<'a> {
    pub track_titles: &'a std::collections::HashMap<String, String>,
    pub spine: Option<&'a AudioChapterSpine>,
}

impl<'a> AudioChapterPlan<'a> {
    pub fn new(track_titles: &'a std::collections::HashMap<String, String>) -> Self {
        Self {
            track_titles,
            spine: None,
        }
    }

    fn track_title(&self, audiofile: &str) -> Option<&str> {
        self.track_titles.get(audiofile).map(|s| s.as_str())
    }
}

fn hole_seconds(holes: &[Hole]) -> f64 {
    holes.iter().map(|h| (h.end - h.start).max(0.0)).sum()
}

/// `between` key → (prev chapter id, next chapter id), Storyteller `betweenChapters`.
fn between_chapters(key: &str) -> (Option<&str>, Option<&str>) {
    let mut parts = key.split('|');
    let left = parts.next().unwrap_or("");
    let right = parts.next().unwrap_or("");
    fn chapter_of(slot: &str) -> Option<&str> {
        if slot.is_empty() {
            None
        } else {
            Some(slot.split('\0').next().unwrap_or(slot))
        }
    }
    (chapter_of(left), chapter_of(right))
}

/// Attach remaining holes to matching chapters (Storyteller `holesByChapter`).
pub fn attach_remaining_holes(
    chapters: &mut [crate::align::readaloud::MarkedChapter],
    remaining: &[Hole],
) {
    for hole in remaining {
        if let Some(ch) = chapters
            .iter_mut()
            .find(|c| c.chapter_id == hole.anchor.chapter_id)
        {
            ch.holes.push(hole.clone());
        }
    }
}

/// Group long boundary holes into audio chapters (Storyteller `planAudioChapters`).
pub fn plan_audio_chapters(
    holes: &[Hole],
    behaviour: RangeBehaviour,
    durations: &std::collections::HashMap<String, f64>,
    plan: &AudioChapterPlan<'_>,
) -> (Vec<AudioChapter>, Vec<Hole>) {
    let Some(threshold) = behaviour.audio_chapter_seconds.filter(|t| *t > 0.0) else {
        return (Vec::new(), holes.to_vec());
    };

    let mut groups: Vec<(String, Vec<Hole>)> = Vec::new();
    let mut group_index: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut remaining = Vec::new();

    for hole in holes {
        if !hole.boundary {
            remaining.push(hole.clone());
            continue;
        }
        if let Some(&i) = group_index.get(&hole.between) {
            groups[i].1.push(hole.clone());
        } else {
            group_index.insert(hole.between.clone(), groups.len());
            groups.push((hole.between.clone(), vec![hole.clone()]));
        }
    }

    let whole_titled_track = |hole: &Hole| {
        hole.start == 0.0
            && (hole.end - durations.get(&hole.audiofile).copied().unwrap_or(0.0)).abs() < 1e-9
            && plan.track_title(&hole.audiofile).is_some()
    };

    let mut chapters = Vec::new();
    for (key, group) in groups {
        let mut runs: Vec<(HoleAnchor, Vec<Hole>)> = Vec::new();
        for hole in group {
            let start_new = runs
                .last()
                .map(|_| whole_titled_track(&hole))
                .unwrap_or(true);
            if start_new {
                runs.push((hole.anchor.clone(), vec![hole]));
            } else if let Some(current) = runs.last_mut() {
                current.1.push(hole);
            }
        }

        let mut kept: Vec<AudioChapter> = Vec::new();
        for (anchor, run_holes) in runs {
            let secs = hole_seconds(&run_holes);
            if secs < threshold {
                remaining.extend(run_holes);
                continue;
            }
            let title = run_holes
                .iter()
                .filter(|hole| {
                    let duration = durations.get(&hole.audiofile).copied().unwrap_or(0.0);
                    duration > 0.0 && (hole.end - hole.start) / duration >= 0.5
                })
                .find_map(|hole| plan.track_title(&hole.audiofile).map(|s| s.to_string()));
            let anchor_title = plan.spine.and_then(|s| s.chapter_title(&anchor.chapter_id));
            kept.push(AudioChapter {
                anchor,
                holes: run_holes,
                title,
                anchor_title,
                existing: None,
                duration_secs: secs,
            });
        }

        let (prev, next) = between_chapters(&key);
        let excluded = plan
            .spine
            .map(|s| s.excluded_between(prev, next))
            .unwrap_or_default();
        if kept.len() == 1 && excluded.len() == 1 {
            kept[0].existing = Some(excluded.into_iter().next().unwrap());
        }
        chapters.extend(kept);
    }

    (chapters, remaining)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn sr(
        chapter_id: &str,
        id: usize,
        start: f64,
        end: f64,
        audiofile: &str,
        state: RangeState,
    ) -> SentenceRange {
        SentenceRange::new(
            id,
            chapter_id,
            vec![AudioRange {
                audiofile: audiofile.into(),
                start,
                end,
            }],
        )
        .with_state(state)
    }

    fn behaviour() -> RangeBehaviour {
        RangeBehaviour {
            gap_seconds: Some(2.0),
            audio_chapter_seconds: Some(30.0),
            drop_unread: true,
        }
    }

    #[test]
    fn plans_long_boundary_holes_as_audio_chapters() {
        let holes = vec![Hole {
            audiofile: "a.mp3".into(),
            start: 0.0,
            end: 60.0,
            anchor: HoleAnchor {
                chapter_id: "c1".into(),
                sentence_id: 0,
                position: HolePosition::After,
            },
            boundary: true,
            between: "|".into(),
        }];
        let empty = HashMap::new();
        let (chapters, rem) = plan_audio_chapters(
            &holes,
            behaviour(),
            &HashMap::new(),
            &AudioChapterPlan::new(&empty),
        );
        assert_eq!(chapters.len(), 1);
        assert!((chapters[0].duration_secs - 60.0).abs() < 1e-9);
        assert!(rem.is_empty());
    }

    /// Title-page false-match at EOF then real chapter early: must not set
    /// eof_clip.end = early_start (that inverted the timeline into micros).
    #[test]
    fn collapse_preserves_out_of_order_eof_then_early() {
        let (ranges, _holes) = collapse_ranges(
            &[
                sr("title", 0, 78708.0, 78726.0, "a.mp3", RangeState::Matched),
                sr("body", 0, 70.0, 100.0, "a.mp3", RangeState::Matched),
                sr("body", 1, 100.0, 200.0, "a.mp3", RangeState::Matched),
            ],
            &["a.mp3".into()],
            &HashMap::from([("a.mp3".into(), 80000.0)]),
            behaviour(),
        );
        assert!(
            (ranges[0].first().start - 78708.0).abs() < 1e-6,
            "title start smashed: {:?}",
            ranges[0].first()
        );
        assert!(
            ranges[0].last().end >= 78726.0 - 1e-6,
            "title end shrunk backward to early next: {:?}",
            ranges[0].last()
        );
        assert!((ranges[1].first().start - 70.0).abs() < 1e-6);
        assert!(ranges[1].last().end > 90.0);
    }

    #[test]
    fn keeps_legacy_behaviour_when_gaps_disabled() {
        let (ranges, holes) = collapse_ranges(
            &[
                sr("c1", 0, 5.0, 10.0, "a.mp3", RangeState::Matched),
                sr("c1", 1, 40.0, 45.0, "a.mp3", RangeState::Matched),
            ],
            &["a.mp3".into()],
            &HashMap::from([("a.mp3".into(), 60.0)]),
            RangeBehaviour::LEGACY,
        );
        assert_eq!(
            ranges
                .iter()
                .map(|r| (r.first().start, r.last().end))
                .collect::<Vec<_>>(),
            vec![(0.0, 40.0), (40.0, 60.0)]
        );
        assert!(holes.is_empty());
    }

    #[test]
    fn bridges_small_gaps_and_turns_large_ones_into_holes() {
        let (ranges, holes) = collapse_ranges(
            &[
                sr("c1", 0, 5.0, 10.0, "a.mp3", RangeState::Matched),
                sr("c1", 1, 11.0, 15.0, "a.mp3", RangeState::Matched),
                sr("c1", 2, 40.0, 45.0, "a.mp3", RangeState::Matched),
            ],
            &["a.mp3".into()],
            &HashMap::from([("a.mp3".into(), 46.0)]),
            behaviour(),
        );
        assert_eq!(
            ranges
                .iter()
                .map(|r| (r.first().start, r.last().end))
                .collect::<Vec<_>>(),
            vec![(5.0, 11.0), (11.0, 15.0), (40.0, 46.0)]
        );
        assert_eq!(holes.len(), 2);
        assert_eq!(holes[0].start, 0.0);
        assert_eq!(holes[0].end, 5.0);
        assert_eq!(holes[0].anchor.sentence_id, 0);
        assert_eq!(holes[0].anchor.position, HolePosition::Before);
        assert!(holes[0].boundary);
        assert_eq!(holes[1].start, 15.0);
        assert_eq!(holes[1].end, 40.0);
        assert_eq!(holes[1].anchor.sentence_id, 1);
        assert_eq!(holes[1].anchor.position, HolePosition::After);
        assert!(!holes[1].boundary);
    }

    #[test]
    fn marks_holes_between_chapters_as_boundaries() {
        let (_ranges, holes) = collapse_ranges(
            &[
                sr("c1", 0, 0.0, 10.0, "a.mp3", RangeState::Matched),
                sr("c2", 0, 50.0, 60.0, "b.mp3", RangeState::Matched),
            ],
            &["a.mp3".into(), "b.mp3".into()],
            &HashMap::from([("a.mp3".into(), 100.0), ("b.mp3".into(), 60.0)]),
            behaviour(),
        );
        assert_eq!(holes.len(), 2);
        assert_eq!(holes[0].audiofile, "a.mp3");
        assert_eq!(holes[0].start, 10.0);
        assert_eq!(holes[0].end, 100.0);
        assert_eq!(holes[0].anchor.chapter_id, "c1");
        assert!(holes[0].boundary);
        assert_eq!(holes[1].audiofile, "b.mp3");
        assert_eq!(holes[1].start, 0.0);
        assert_eq!(holes[1].end, 50.0);
        assert_eq!(holes[1].anchor.chapter_id, "c2");
        assert!(holes[1].boundary);
    }

    #[test]
    fn unreferenced_tracks_get_a_hole() {
        let (_ranges, holes) = collapse_ranges(
            &[
                sr("c1", 0, 0.0, 10.0, "a.mp3", RangeState::Matched),
                sr("c2", 0, 0.0, 10.0, "c.mp3", RangeState::Matched),
            ],
            &["a.mp3".into(), "b.mp3".into(), "c.mp3".into()],
            &HashMap::from([
                ("a.mp3".into(), 10.0),
                ("b.mp3".into(), 90.0),
                ("c.mp3".into(), 10.0),
            ]),
            behaviour(),
        );
        assert_eq!(holes.len(), 1);
        assert_eq!(holes[0].audiofile, "b.mp3");
        assert_eq!(holes[0].start, 0.0);
        assert_eq!(holes[0].end, 90.0);
        assert_eq!(holes[0].anchor.chapter_id, "c1");
        assert_eq!(holes[0].anchor.position, HolePosition::After);
    }

    #[test]
    fn pins_dropped_sentences_before_next() {
        let (ranges, _holes) = collapse_ranges(
            &[
                sr("c1", 0, 0.0, 10.0, "a.mp3", RangeState::Matched),
                sr("c1", 1, 10.0, 12.0, "a.mp3", RangeState::Dropped),
                sr("c1", 2, 12.0, 20.0, "a.mp3", RangeState::Matched),
            ],
            &["a.mp3".into()],
            &HashMap::from([("a.mp3".into(), 20.0)]),
            behaviour(),
        );
        let millis: Vec<_> = ranges
            .iter()
            .map(|r| {
                (
                    r.id,
                    (r.first().start * 1000.0).round() as i64,
                    (r.last().end * 1000.0).round() as i64,
                )
            })
            .collect();
        assert_eq!(
            millis,
            vec![(0, 0, 11999), (1, 11999, 12000), (2, 12000, 20000)]
        );
    }

    #[test]
    fn short_remaining_holes_attach_and_emit_smil_audio_only() {
        use crate::align::readaloud::MarkedChapter;
        use crate::align::smil::chapter_smil_with_prefix;

        let holes = vec![Hole {
            audiofile: "a.mp3".into(),
            start: 10.0,
            end: 20.0, // under audio_chapter_seconds (30)
            anchor: HoleAnchor {
                chapter_id: "c1".into(),
                sentence_id: 0,
                position: HolePosition::After,
            },
            boundary: true,
            between: "|".into(),
        }];
        let empty = HashMap::new();
        let (planned, rem) = plan_audio_chapters(
            &holes,
            behaviour(),
            &HashMap::new(),
            &AudioChapterPlan::new(&empty),
        );
        assert!(planned.is_empty(), "short boundary hole must not promote");
        assert_eq!(rem.len(), 1);

        let mut chapters = vec![MarkedChapter {
            chapter_id: "c1".into(),
            href: "c1.xhtml".into(),
            xhtml: String::new(),
            smil_href: "MediaOverlays/c1.smil".into(),
            sentences: vec![sr("c1", 0, 0.0, 10.0, "a.mp3", RangeState::Matched)],
            holes: Vec::new(),
            fragment_prefix: "sentence".into(),
            text_ref: crate::align::smil::TextRef::IdFragment,
            sentence_texts: Vec::new(),
            audio_heading: None,
            spine_insert: None,
            title: None,
        }];
        attach_remaining_holes(&mut chapters, &rem);
        assert_eq!(chapters[0].holes.len(), 1);

        let smil = chapter_smil_with_prefix(
            "c1",
            "../c1.xhtml",
            "../Audio/",
            &chapters[0].sentences,
            "sentence",
            &chapters[0].holes,
        );
        assert!(smil.contains("storyteller:audio-only"));
        assert!(smil.contains("c1-s0-after0"));
        assert!(smil.contains("clipBegin=\"10.000s\""));
        assert!(smil.contains("clipEnd=\"20.000s\""));
    }

    #[test]
    fn splits_group_at_every_titled_whole_track() {
        let durations = HashMap::from([
            ("a.mp3".into(), 20.0),
            ("b.mp3".into(), 90.0),
            ("c.mp3".into(), 40.0),
        ]);
        let (_ranges, holes) = collapse_ranges(
            &[
                sr("c1", 0, 0.0, 10.0, "a.mp3", RangeState::Matched),
                sr("c2", 0, 30.0, 40.0, "c.mp3", RangeState::Matched),
            ],
            &["a.mp3".into(), "b.mp3".into(), "c.mp3".into()],
            &durations,
            behaviour(),
        );
        let titles = HashMap::from([("b.mp3".into(), "Interview".into())]);
        let (chapters, remaining) = plan_audio_chapters(
            &holes,
            behaviour(),
            &durations,
            &AudioChapterPlan::new(&titles),
        );
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title.as_deref(), Some("Interview"));
        assert_eq!(chapters[0].anchor.chapter_id, "c1");
        assert_eq!(chapters[0].anchor.position, HolePosition::After);
        assert_eq!(
            chapters[0]
                .holes
                .iter()
                .map(|h| h.audiofile.as_str())
                .collect::<Vec<_>>(),
            vec!["b.mp3", "c.mp3"]
        );
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].audiofile, "a.mp3");
        assert_eq!(remaining[0].start, 10.0);
        assert_eq!(remaining[0].end, 20.0);
    }

    #[test]
    fn each_titled_bonus_track_gets_own_chapter() {
        let durations = HashMap::from([
            ("a.mp3".into(), 10.0),
            ("b.mp3".into(), 90.0),
            ("c.mp3".into(), 60.0),
            ("d.mp3".into(), 10.0),
        ]);
        let (_ranges, holes) = collapse_ranges(
            &[
                sr("c1", 0, 0.0, 10.0, "a.mp3", RangeState::Matched),
                sr("c2", 0, 0.0, 10.0, "d.mp3", RangeState::Matched),
            ],
            &[
                "a.mp3".into(),
                "b.mp3".into(),
                "c.mp3".into(),
                "d.mp3".into(),
            ],
            &durations,
            behaviour(),
        );
        let titles = HashMap::from([
            ("b.mp3".into(), "Interview with the Author".into()),
            ("c.mp3".into(), "End credits".into()),
        ]);
        let (chapters, remaining) = plan_audio_chapters(
            &holes,
            behaviour(),
            &durations,
            &AudioChapterPlan::new(&titles),
        );
        assert_eq!(
            chapters
                .iter()
                .map(|c| {
                    (
                        c.title.clone(),
                        c.holes
                            .iter()
                            .map(|h| h.audiofile.clone())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                (
                    Some("Interview with the Author".into()),
                    vec!["b.mp3".into()]
                ),
                (Some("End credits".into()), vec!["c.mp3".into()]),
            ]
        );
        assert!(remaining.is_empty());
    }

    #[test]
    fn reuses_single_excluded_chapter_between_anchors() {
        let durations = HashMap::from([
            ("a.mp3".into(), 10.0),
            ("b.mp3".into(), 90.0),
            ("c.mp3".into(), 10.0),
        ]);
        let (_ranges, holes) = collapse_ranges(
            &[
                sr("c1", 0, 0.0, 10.0, "a.mp3", RangeState::Matched),
                sr("c3", 0, 0.0, 10.0, "c.mp3", RangeState::Matched),
            ],
            &["a.mp3".into(), "b.mp3".into(), "c.mp3".into()],
            &durations,
            behaviour(),
        );
        let spine = AudioChapterSpine::from_pending([
            SpineEntryLike {
                index: 0,
                chapter_id: "c1".into(),
                title: Some("Chapter 12".into()),
                status: "aligned".into(),
            },
            SpineEntryLike {
                index: 1,
                chapter_id: "c2".into(),
                title: Some("Bonus".into()),
                status: "excluded".into(),
            },
            SpineEntryLike {
                index: 2,
                chapter_id: "c3".into(),
                title: Some("Chapter 13".into()),
                status: "aligned".into(),
            },
        ]);
        let empty = HashMap::new();
        let plan = AudioChapterPlan {
            track_titles: &empty,
            spine: Some(&spine),
        };
        let (chapters, _) = plan_audio_chapters(&holes, behaviour(), &durations, &plan);
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, None);
        assert_eq!(chapters[0].anchor_title.as_deref(), Some("Chapter 12"));
        assert_eq!(
            chapters[0].existing,
            Some(ExistingChapter {
                id: "c2".into(),
                title: Some("Bonus".into()),
            })
        );
    }

    #[test]
    fn free_standing_when_two_excluded_between() {
        let durations = HashMap::from([
            ("a.mp3".into(), 10.0),
            ("b.mp3".into(), 90.0),
            ("c.mp3".into(), 10.0),
        ]);
        let (_ranges, holes) = collapse_ranges(
            &[
                sr("c1", 0, 0.0, 10.0, "a.mp3", RangeState::Matched),
                sr("c4", 0, 0.0, 10.0, "c.mp3", RangeState::Matched),
            ],
            &["a.mp3".into(), "b.mp3".into(), "c.mp3".into()],
            &durations,
            behaviour(),
        );
        let spine = AudioChapterSpine::from_pending([
            SpineEntryLike {
                index: 0,
                chapter_id: "c1".into(),
                title: Some("Chapter 12".into()),
                status: "aligned".into(),
            },
            SpineEntryLike {
                index: 1,
                chapter_id: "c2".into(),
                title: Some("Bonus".into()),
                status: "excluded".into(),
            },
            SpineEntryLike {
                index: 2,
                chapter_id: "c3".into(),
                title: Some("Notes".into()),
                status: "excluded".into(),
            },
            SpineEntryLike {
                index: 3,
                chapter_id: "c4".into(),
                title: Some("Chapter 13".into()),
                status: "aligned".into(),
            },
        ]);
        let empty = HashMap::new();
        let plan = AudioChapterPlan {
            track_titles: &empty,
            spine: Some(&spine),
        };
        let (chapters, _) = plan_audio_chapters(&holes, behaviour(), &durations, &plan);
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].anchor_title.as_deref(), Some("Chapter 12"));
        assert_eq!(chapters[0].existing, None);
    }
}
