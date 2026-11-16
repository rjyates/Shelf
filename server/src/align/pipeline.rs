//! End-to-end align pipeline (Storyteller stage order).
//!
//! process → transcribe → markup → align (per spine chapter) → collapse holes → write readaloud.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use zip::ZipArchive;

use crate::align::audio_chapters::{materialize_audio_chapters, track_titles_from_audio_paths};
use crate::align::ctc_align;
use crate::align::ctc_emit::CtcEmitClient;
use crate::align::error_align::{
    alignments_for_sentence, error_align, error_align_with_narrowing, hyp_span_chars,
};
use crate::align::exclusions::{
    chapter_aligned_from, classify_spine_item, exclusion_context_from_opf, is_xhtml_document,
    nav_document_href, ExclusionOptions, SpineItemMeta,
};
use crate::align::holes::{
    attach_remaining_holes, collapse_ranges, plan_audio_chapters, AudioChapterPlan,
    AudioChapterSpine, RangeBehaviour, SpineEntryLike,
};
use crate::align::interpolate::{interpolate_sentence_ranges_with, InterpolateOptions};
use crate::align::mapping::{MapAssoc, Mapping};
use crate::align::markup::markup_xhtml_body_locale;
use crate::align::match_score::accept_match;
use crate::align::ranges::{
    enforce_monotonic_audio_ranges, expand_empty_sentence_ranges, AudioRange, SentenceRange,
};
use crate::align::readaloud::{write_readaloud, MarkedChapter, ReadaloudInputs};
use crate::align::report::ReportAligner;
use crate::align::search::{find_boundaries_with_index, NgramIndex};
use crate::align::slugify::slugify_for_search;
use crate::align::spine::list_spine_chapters;
use crate::align::transcribe::{Transcriber, Transcript};
use crate::align::words::{
    expand_to_word_ranges_ctc, expand_to_word_ranges_with, inject_word_spans_locale,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlignGranularity {
    #[default]
    Sentence,
    Word,
}

#[derive(Debug, Clone)]
pub struct AlignRequest {
    pub epub_path: PathBuf,
    pub audio_paths: Vec<PathBuf>,
    pub output_path: PathBuf,
    /// When set, only this chapter is aligned (tests). Otherwise every spine XHTML.
    pub chapter_href: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AlignProgress {
    pub stage: &'static str,
    pub fraction: f32,
    pub detail: String,
}

pub type ProgressFn = Box<dyn FnMut(AlignProgress) + Send>;

pub struct AlignPipeline<'a> {
    pub transcriber: &'a dyn Transcriber,
    pub behaviour: RangeBehaviour,
    pub granularity: AlignGranularity,
    pub exclusions: ExclusionOptions,
    /// When set (with `work_dir`), Combine uses CTC emit + forced-align instead of Whisper.
    pub ctc_emit: Option<&'a CtcEmitClient>,
    pub work_dir: Option<PathBuf>,
}

impl<'a> AlignPipeline<'a> {
    pub fn new(transcriber: &'a dyn Transcriber) -> Self {
        Self {
            transcriber,
            behaviour: RangeBehaviour::DEFAULT,
            granularity: AlignGranularity::Sentence,
            exclusions: ExclusionOptions::default(),
            ctc_emit: None,
            work_dir: None,
        }
    }
}

/// UTF-16 code-unit length (Storyteller / JS string `.length`). BMP ≡ Unicode scalar.
pub(crate) fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn byte_to_utf16(s: &str, byte: usize) -> usize {
    let byte = byte.min(s.len());
    // Avoid slicing mid-codepoint: walk back to char boundary.
    let mut b = byte;
    while b > 0 && !s.is_char_boundary(b) {
        b -= 1;
    }
    utf16_len(&s[..b])
}

/// Whisper / ST timeline entry with UTF-16 offsets into the pre-slug transcript.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineEntry {
    pub start_time: f64,
    pub end_time: f64,
    pub audiofile: String,
    pub start_offset_utf16: usize,
    pub end_offset_utf16: usize,
}

/// ST `mapTranscriptionTimeline` result row.
#[derive(Debug, Clone, PartialEq)]
pub struct MappedTimelineEntry {
    pub start_time: f64,
    pub end_time: f64,
    pub audiofile: String,
    pub start_offset_utf16: usize,
    pub end_offset_utf16: usize,
    pub mapped_start_offset_utf16: usize,
    pub mapped_end_offset_utf16: usize,
}

/// Storyteller `mapTranscriptionTimeline` — remap UTF-16 offsets through slugify Mapping.
pub fn map_transcription_timeline(
    timeline: &[TimelineEntry],
    mapping: &mut Mapping,
) -> Vec<MappedTimelineEntry> {
    let mut cursor = mapping.cursor();
    timeline
        .iter()
        .map(|entry| MappedTimelineEntry {
            start_time: entry.start_time,
            end_time: entry.end_time,
            audiofile: entry.audiofile.clone(),
            start_offset_utf16: entry.start_offset_utf16,
            end_offset_utf16: entry.end_offset_utf16,
            mapped_start_offset_utf16: cursor
                .map(entry.start_offset_utf16 as i32, MapAssoc::End)
                .max(0) as usize,
            mapped_end_offset_utf16: cursor
                .map(entry.end_offset_utf16 as i32, MapAssoc::Start)
                .max(0) as usize,
        })
        .collect()
}

/// Map slug UTF-16 offsets → transcript word indices (built once per job).
struct SlugTimeMap {
    /// (UTF-16 offset at start of word token, word index)
    starts: Vec<(usize, usize)>,
}

impl SlugTimeMap {
    fn build(transcript: &Transcript) -> Self {
        let mut starts = Vec::with_capacity(transcript.words.len());
        let mut cursor = 0usize;
        for (i, w) in transcript.words.iter().enumerate() {
            let token = slugify_for_search(&w.text);
            if token.is_empty() {
                continue;
            }
            starts.push((cursor, i));
            // ST / JS: hyphen-joined slug tokens indexed in UTF-16 code units.
            cursor += utf16_len(&token) + 1;
        }
        Self { starts }
    }

    /// `start`/`end` are UTF-8 byte offsets into `slug_doc` (from find_boundaries);
    /// converted to UTF-16 before lookup.
    fn times_for_span(
        &self,
        slug_doc: &str,
        start_byte: usize,
        end_byte: usize,
        transcript: &Transcript,
    ) -> Option<(f64, f64, String)> {
        if self.starts.is_empty() {
            return None;
        }
        let start = byte_to_utf16(slug_doc, start_byte);
        let end = byte_to_utf16(slug_doc, end_byte);
        let start_word = self
            .starts
            .iter()
            .find(|&&(off, idx)| {
                let token = slugify_for_search(&transcript.words[idx].text);
                off + utf16_len(&token) >= start
            })
            .map(|&(_, i)| i)?;
        let end_word = self
            .starts
            .iter()
            .rev()
            .find(|&&(off, _)| off < end)
            .map(|&(_, i)| i)
            .unwrap_or(start_word);
        let a = &transcript.words[start_word];
        let b = &transcript.words[end_word.min(transcript.words.len() - 1)];
        Some((a.start, b.end, a.audiofile.clone()))
    }
}

/// Run process→transcribe→markup→align→write (process is identity while audio is already split).
pub fn run_align(
    pipeline: &AlignPipeline<'_>,
    req: &AlignRequest,
    mut on_progress: Option<ProgressFn>,
) -> Result<(), String> {
    if let (Some(client), Some(work_dir)) = (pipeline.ctc_emit, pipeline.work_dir.as_ref()) {
        return run_align_ctc(pipeline, req, client, work_dir, on_progress);
    }

    let report =
        |stage: &'static str, fraction: f32, detail: String, cb: &mut Option<ProgressFn>| {
            if let Some(cb) = cb {
                cb(AlignProgress {
                    stage,
                    fraction,
                    detail,
                });
            }
        };

    report(
        "SPLIT_TRACKS",
        0.05,
        "using source audio".into(),
        &mut on_progress,
    );

    report(
        "TRANSCRIBE_CHAPTERS",
        0.1,
        "transcribing".into(),
        &mut on_progress,
    );
    let mut words = Vec::new();
    let n_audio = req.audio_paths.len().max(1);
    for (i, audio) in req.audio_paths.iter().enumerate() {
        let base = 0.1 + 0.35 * (i as f32 / n_audio as f32);
        let span = 0.35 / n_audio as f32;
        let label = audio.display().to_string();
        let t = pipeline
            .transcriber
            .transcribe_file_with_progress(audio, &mut |file_frac| {
                let frac = (base + span * file_frac.clamp(0.0, 1.0)).clamp(0.0, 0.45);
                report(
                    "TRANSCRIBE_CHAPTERS",
                    frac,
                    format!("transcribing {label}"),
                    &mut on_progress,
                );
            })?;
        words.extend(t.words);
        report(
            "TRANSCRIBE_CHAPTERS",
            base + span,
            format!("transcribed {label}"),
            &mut on_progress,
        );
    }
    let transcript = Transcript { words };
    if transcript.words.is_empty() {
        return Err("Transcription produced no words".into());
    }

    let chapters = if let Some(href) = &req.chapter_href {
        vec![single_chapter_meta(href.clone())]
    } else {
        list_spine_chapters(&req.epub_path)?
    };

    report(
        "SYNC_CHAPTERS",
        0.5,
        format!("aligning {} chapters", chapters.len()),
        &mut on_progress,
    );

    let doc_plain = transcript.joined_text();
    let doc = slugify_for_search(&doc_plain);
    let doc_index = NgramIndex::build(&doc, 5);
    let time_map = SlugTimeMap::build(&transcript);
    let mut epub_zip = open_epub_zip(&req.epub_path)?;
    let exclusions = pipeline.exclusions.clone();
    let exclusion_ctx = load_exclusion_context(&mut epub_zip);

    let mut marked_chapters: Vec<MarkedChapter> = Vec::new();
    let mut chapter_sentence_counts: HashMap<String, usize> = HashMap::new();
    let mut sentence_lengths: HashMap<(String, usize), usize> = HashMap::new();
    let mut words_per_sentence: Vec<(String, usize, Vec<String>)> = Vec::new();
    let mut next_word_id = 0usize;
    let mut pending_spine: Vec<SpineEntryLike> = Vec::new();
    let mut spine_xhtml: HashMap<String, (String, String)> = HashMap::new();
    let fragment_prefix = match pipeline.granularity {
        AlignGranularity::Sentence => "sentence",
        AlignGranularity::Word => "word",
    };
    let n_ch = chapters.len().max(1);
    // Storyteller advances through the transcript so later chapters do not
    // re-latch onto earlier narration.
    let mut align_start = 0usize;

    for (ci, chapter) in chapters.iter().enumerate() {
        let xhtml_opt = if is_xhtml_document(&chapter.media_type) {
            read_zip_entry(&mut epub_zip, &chapter.href).ok()
        } else {
            None
        };
        if let Some(ref xhtml) = xhtml_opt {
            spine_xhtml.insert(chapter.id.clone(), (chapter.href.clone(), xhtml.clone()));
        }
        let classification = classify_spine_item(chapter, xhtml_opt.as_deref(), &exclusion_ctx);
        if !chapter_aligned_from(&classification, &exclusions) {
            pending_spine.push(SpineEntryLike {
                index: chapter.index,
                chapter_id: chapter.id.clone(),
                title: None,
                status: "excluded".into(),
            });
            continue;
        }
        let Some(xhtml) = xhtml_opt else {
            pending_spine.push(SpineEntryLike {
                index: chapter.index,
                chapter_id: chapter.id.clone(),
                title: None,
                status: "excluded".into(),
            });
            continue;
        };
        let (mut marked, sentences) = match markup_xhtml_body_locale(
            &xhtml,
            Some(chapter.id.as_str()),
            &exclusion_ctx.language,
        ) {
            Ok(v) => v,
            Err(_) => continue, // empty / no sentences — ST skips
        };
        if pipeline.granularity == AlignGranularity::Word {
            let (xhtml_w, words, nid) = inject_word_spans_locale(
                &marked,
                &sentences,
                next_word_id,
                Some(chapter.id.as_str()),
                &exclusion_ctx.language,
            );
            marked = xhtml_w;
            next_word_id = nid;
            for (id, words) in words.into_iter().enumerate() {
                words_per_sentence.push((chapter.id.clone(), id, words));
            }
        }
        // Storyteller skips chapters with <2 sentences and <4 words on a single sentence.
        let word_count: usize = sentences.iter().map(|s| s.split_whitespace().count()).sum();
        if sentences.len() < 2 && word_count < 4 {
            continue;
        }

        chapter_sentence_counts.insert(chapter.id.clone(), sentences.len());
        for (id, sentence) in sentences.iter().enumerate() {
            sentence_lengths.insert((chapter.id.clone(), id), sentence.chars().count());
        }

        let chapter_query = slugify_for_search(&sentences.join(" "));
        let window =
            find_boundaries_with_index(&chapter_query, &doc, &doc_index, align_start, 1000)
                .or_else(|| {
                    if align_start == 0 {
                        None
                    } else {
                        find_boundaries_with_index(&chapter_query, &doc, &doc_index, 0, 1000)
                    }
                });
        // shortcut: chapter windows skip Levenshtein (query can be tens of KB);
        // sentence-level accept_match below is the score>0 gate. Upgrade when
        // errorAlign windows replace n-gram chapter locate.
        // Real chapter hit → advance cursor. Miss → search the remaining
        // transcript; unmatched sentences are filled later by interpolate
        // (Storyteller), never by inventing Soft stub times that mono parks.
        let chapter_window = match window {
            Some(b) => {
                align_start = b.end;
                ChapterWindow::Trusted {
                    start: b.start,
                    end: b.end,
                }
            }
            None => ChapterWindow::Soft {
                start: align_start,
                end: doc.len(),
            },
        };
        let ranges = align_sentences_in_window(
            &sentences,
            &transcript,
            &doc,
            &doc_index,
            &time_map,
            &chapter.id,
            chapter_window,
        )?;

        let stem = Path::new(&chapter.href)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("chapter");
        // Storyteller: MediaOverlays next to the chapter (under OEBPS/ when used).
        let smil_href = match Path::new(&chapter.href).parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                format!("{}/MediaOverlays/{stem}.smil", parent.to_string_lossy())
            }
            _ => format!("MediaOverlays/{stem}.smil"),
        };
        marked_chapters.push(MarkedChapter {
            chapter_id: chapter.id.clone(),
            href: chapter.href.clone(),
            xhtml: marked,
            smil_href,
            sentences: ranges,
            holes: Vec::new(),
            fragment_prefix: fragment_prefix.to_string(),
            text_ref: crate::align::smil::TextRef::IdFragment,
            sentence_texts: Vec::new(),
            audio_heading: None,
            spine_insert: None,
            title: None,
        });
        pending_spine.push(SpineEntryLike {
            index: chapter.index,
            chapter_id: chapter.id.clone(),
            title: None,
            status: "aligned".into(),
        });

        report(
            "SYNC_CHAPTERS",
            0.5 + 0.35 * ((ci + 1) as f32 / n_ch as f32),
            format!("aligned {}", chapter.href),
            &mut on_progress,
        );
    }

    if marked_chapters.is_empty() {
        return Err("No chapters could be aligned to the transcript".into());
    }
    let matched_any = marked_chapters.iter().any(|c| !c.sentences.is_empty());
    if !matched_any {
        return Err("No sentences matched the transcript".into());
    }

    let files: Vec<String> = req
        .audio_paths
        .iter()
        .filter_map(|p| p.file_name()?.to_str().map(|s| s.to_string()))
        .collect();
    let mut durations = HashMap::new();
    for f in &files {
        let file_words: Vec<_> = transcript
            .words
            .iter()
            .filter(|w| &w.audiofile == f)
            .collect();
        let dur = file_words.last().map(|w| w.end).unwrap_or_else(|| {
            if files.len() == 1 {
                transcript.words.last().map(|w| w.end).unwrap_or(1.0)
            } else {
                1.0
            }
        });
        durations.insert(f.clone(), dur);
    }

    // Storyteller: keep only matched anchors, interpolate gaps, then mono/collapse.
    let matched: Vec<SentenceRange> = marked_chapters
        .iter()
        .flat_map(|c| c.sentences.iter().cloned())
        .collect();
    let chapter_order: Vec<String> = marked_chapters
        .iter()
        .map(|c| c.chapter_id.clone())
        .collect();
    let interp_opts = InterpolateOptions {
        dropped_slots: None, // CTC/exclusions not yet sourced
        drop_unread: pipeline.behaviour.drop_unread,
    };
    let filled = interpolate_sentence_ranges_with(
        &matched,
        &chapter_order,
        &chapter_sentence_counts,
        &durations,
        &sentence_lengths,
        &interp_opts,
    );
    let mono = enforce_monotonic_audio_ranges(&filled, &files, &durations);
    let expanded = expand_empty_sentence_ranges(&mono);
    let (collapsed, holes) = collapse_ranges(&expanded, &files, &durations, pipeline.behaviour);
    let audio_spine = AudioChapterSpine::from_pending(pending_spine);
    let track_titles = track_titles_from_audio_paths(&req.audio_paths);
    let audio_plan = AudioChapterPlan {
        track_titles: &track_titles,
        spine: Some(&audio_spine),
    };
    let (planned_audio, remaining_holes) =
        plan_audio_chapters(&holes, pipeline.behaviour, &durations, &audio_plan);
    let package_audio_duration_secs = Some(durations.values().copied().sum::<f64>());

    let final_ranges = if pipeline.granularity == AlignGranularity::Word {
        let times = |start: usize, end: usize| {
            time_map.times_for_span(doc.as_str(), start, end, &transcript)
        };
        expand_to_word_ranges_with(
            &collapsed,
            &words_per_sentence,
            Some(doc.as_str()),
            Some(&times),
        )
    } else {
        collapsed
    };

    // Redistribute ranges back into chapters by (chapter_id, id).
    let mut by_key: HashMap<(String, usize), SentenceRange> = HashMap::new();
    for r in final_ranges {
        by_key.insert((r.chapter_id.clone(), r.id), r);
    }
    for ch in &mut marked_chapters {
        if pipeline.granularity == AlignGranularity::Word {
            let mut ids: Vec<usize> = by_key
                .keys()
                .filter(|(cid, _)| cid == &ch.chapter_id)
                .map(|(_, id)| *id)
                .collect();
            ids.sort_unstable();
            ch.sentences = ids
                .into_iter()
                .filter_map(|id| by_key.remove(&(ch.chapter_id.clone(), id)))
                .collect();
        } else {
            let n = chapter_sentence_counts
                .get(&ch.chapter_id)
                .copied()
                .unwrap_or(0);
            ch.sentences = (0..n)
                .filter_map(|id| by_key.remove(&(ch.chapter_id.clone(), id)))
                .collect();
        }
    }
    attach_remaining_holes(&mut marked_chapters, &remaining_holes);

    let content_root = marked_chapters
        .first()
        .map(|c| {
            std::path::Path::new(&c.href.replace('\\', "/"))
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default()
        })
        .unwrap_or_default();
    marked_chapters.extend(materialize_audio_chapters(
        &planned_audio,
        &content_root,
        &spine_xhtml,
        &exclusion_ctx.language,
    ));

    report(
        "SYNC_CHAPTERS",
        0.9,
        "writing readaloud EPUB".into(),
        &mut on_progress,
    );
    write_readaloud(
        &ReadaloudInputs {
            source_epub: req.epub_path.clone(),
            chapters: marked_chapters,
            audio_files: req.audio_paths.clone(),
            package_audio_duration_secs,
            aligner: ReportAligner::Classic,
        },
        &req.output_path,
    )?;
    report("SYNC_CHAPTERS", 1.0, "done".into(), &mut on_progress);
    Ok(())
}

/// Soft-fail discontinuous leap to outro: match start in last 10% of STEM while
/// the prior CTC cursor is still in the first half (RotK title→outro). ST has
/// no spine-index front-zone heuristic; this is Shelf cursor-continuity soft-fail.
pub(crate) fn soft_fail_discontinuous_outro(
    match_start_frame: usize,
    prev_ctc_end_frame: i32,
    total_frames: usize,
) -> bool {
    let total = total_frames.max(1) as f64;
    let near_eof = (match_start_frame as f64) > 0.9 * total;
    let cursor_early = (prev_ctc_end_frame.max(0) as f64) < 0.5 * total;
    near_eof && cursor_early
}

/// CTC emit → STEM decode → forced-align chapters → same interpolate/write path as Whisper.
fn run_align_ctc(
    pipeline: &AlignPipeline<'_>,
    req: &AlignRequest,
    client: &CtcEmitClient,
    work_dir: &Path,
    mut on_progress: Option<ProgressFn>,
) -> Result<(), String> {
    let report =
        |stage: &'static str, fraction: f32, detail: String, cb: &mut Option<ProgressFn>| {
            if let Some(cb) = cb {
                cb(AlignProgress {
                    stage,
                    fraction,
                    detail,
                });
            }
        };

    report(
        "SPLIT_TRACKS",
        0.05,
        "using source audio".into(),
        &mut on_progress,
    );
    report(
        "TRANSCRIBE_CHAPTERS",
        0.1,
        "generating_emissions".into(),
        &mut on_progress,
    );

    let n_audio = req.audio_paths.len().max(1);
    // Emit progress ticks per file around the blocking emit_book call.
    for (i, audio) in req.audio_paths.iter().enumerate() {
        let base = 0.1 + 0.35 * (i as f32 / n_audio as f32);
        report(
            "TRANSCRIBE_CHAPTERS",
            base,
            format!("generating_emissions {}", audio.display()),
            &mut on_progress,
        );
    }
    let mut book = ctc_align::emit_book(client, &req.audio_paths, work_dir)?;
    {
        let audio_names: Vec<String> = req
            .audio_paths
            .iter()
            .map(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("audio")
                    .to_string()
            })
            .collect();
        let emission_names: Vec<String> =
            book.frame_map.iter().map(|(n, _, _)| n.clone()).collect();
        ctc_align::check_audio_emissions_pairing(&audio_names, &emission_names)?;
    }
    report(
        "TRANSCRIBE_CHAPTERS",
        0.45,
        "emissions ready".into(),
        &mut on_progress,
    );

    let chapters = if let Some(href) = &req.chapter_href {
        vec![single_chapter_meta(href.clone())]
    } else {
        list_spine_chapters(&req.epub_path)?
    };

    report(
        "SYNC_CHAPTERS",
        0.5,
        format!("ctc aligning {} chapters", chapters.len()),
        &mut on_progress,
    );

    let mut epub_zip = open_epub_zip(&req.epub_path)?;
    let exclusions = pipeline.exclusions.clone();
    let exclusion_ctx = load_exclusion_context(&mut epub_zip);

    let mut marked_chapters: Vec<MarkedChapter> = Vec::new();
    let mut chapter_sentence_counts: HashMap<String, usize> = HashMap::new();
    let mut sentence_lengths: HashMap<(String, usize), usize> = HashMap::new();
    let mut dropped_slots: HashSet<(String, usize)> = HashSet::new();
    let mut words_per_sentence: Vec<(String, usize, Vec<String>)> = Vec::new();
    let mut ctc_word_clips: HashMap<(String, usize), Vec<AudioRange>> = HashMap::new();
    let mut next_word_id = 0usize;
    let mut pending_spine: Vec<SpineEntryLike> = Vec::new();
    let mut spine_xhtml: HashMap<String, (String, String)> = HashMap::new();
    let fragment_prefix = match pipeline.granularity {
        AlignGranularity::Sentence => "sentence",
        AlignGranularity::Word => "word",
    };
    let n_ch = chapters.len().max(1);
    let mut prev_ctc_end_frame: i32 = -1;
    // ST withMasks: blank emission rows + seed search excluded_ranges.
    let audio_masks = ctc_align::audio_exclusion_frame_masks(
        &exclusions.decisions.audio,
        &book.frame_map,
        &book.file_timings,
    );
    ctc_align::apply_emission_masks(&mut book.emissions, &audio_masks, book.header.blank_index);
    // ST withMasks returns a new EmissionsReader (empty decode WeakMap).
    book.clear_decoded();
    let mut matched_boundaries = audio_masks;

    for (ci, chapter) in chapters.iter().enumerate() {
        let xhtml_opt = if is_xhtml_document(&chapter.media_type) {
            read_zip_entry(&mut epub_zip, &chapter.href).ok()
        } else {
            None
        };
        if let Some(ref xhtml) = xhtml_opt {
            spine_xhtml.insert(chapter.id.clone(), (chapter.href.clone(), xhtml.clone()));
        }
        let classification = classify_spine_item(chapter, xhtml_opt.as_deref(), &exclusion_ctx);
        if !chapter_aligned_from(&classification, &exclusions) {
            pending_spine.push(SpineEntryLike {
                index: chapter.index,
                chapter_id: chapter.id.clone(),
                title: None,
                status: "excluded".into(),
            });
            continue;
        }
        let Some(xhtml) = xhtml_opt else {
            pending_spine.push(SpineEntryLike {
                index: chapter.index,
                chapter_id: chapter.id.clone(),
                title: None,
                status: "excluded".into(),
            });
            continue;
        };
        let (mut marked, sentences) = match markup_xhtml_body_locale(
            &xhtml,
            Some(chapter.id.as_str()),
            &exclusion_ctx.language,
        ) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if pipeline.granularity == AlignGranularity::Word {
            let (xhtml_w, words, nid) = inject_word_spans_locale(
                &marked,
                &sentences,
                next_word_id,
                Some(chapter.id.as_str()),
                &exclusion_ctx.language,
            );
            marked = xhtml_w;
            next_word_id = nid;
            for (id, words) in words.into_iter().enumerate() {
                words_per_sentence.push((chapter.id.clone(), id, words));
            }
        }
        let word_count: usize = sentences.iter().map(|s| s.split_whitespace().count()).sum();
        if sentences.len() < 2 && word_count < 4 {
            continue;
        }

        chapter_sentence_counts.insert(chapter.id.clone(), sentences.len());
        for (id, sentence) in sentences.iter().enumerate() {
            sentence_lengths.insert((chapter.id.clone(), id), sentence.chars().count());
        }

        let ((mut ranges, dropped, word_clips), mut chapter_end, mut matched_span) =
            ctc_align::align_chapter(
                &chapter.id,
                &sentences,
                &book,
                prev_ctc_end_frame,
                &matched_boundaries,
            )?;
        ctc_word_clips.extend(word_clips);
        // Soft-fail discontinuous leap to outro (RotK title→outro).
        if let Some((s, _)) = matched_span {
            if soft_fail_discontinuous_outro(s, prev_ctc_end_frame, book.emissions.frames) {
                ranges.clear();
                chapter_end = prev_ctc_end_frame;
                matched_span = None;
            }
        }
        if chapter_end >= 0 {
            prev_ctc_end_frame = chapter_end;
        }
        if let Some(span) = matched_span {
            matched_boundaries.push(span);
        }
        dropped_slots.extend(dropped);

        let stem = Path::new(&chapter.href)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("chapter");
        let smil_href = match Path::new(&chapter.href).parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                format!("{}/MediaOverlays/{stem}.smil", parent.to_string_lossy())
            }
            _ => format!("MediaOverlays/{stem}.smil"),
        };
        marked_chapters.push(MarkedChapter {
            chapter_id: chapter.id.clone(),
            href: chapter.href.clone(),
            xhtml: marked,
            smil_href,
            sentences: ranges,
            holes: Vec::new(),
            fragment_prefix: fragment_prefix.to_string(),
            text_ref: crate::align::smil::TextRef::IdFragment,
            sentence_texts: Vec::new(),
            audio_heading: None,
            spine_insert: None,
            title: None,
        });
        pending_spine.push(SpineEntryLike {
            index: chapter.index,
            chapter_id: chapter.id.clone(),
            title: None,
            status: "aligned".into(),
        });

        report(
            "SYNC_CHAPTERS",
            0.5 + 0.35 * ((ci + 1) as f32 / n_ch as f32),
            format!("ctc aligned {}", chapter.href),
            &mut on_progress,
        );
    }

    if marked_chapters.is_empty() {
        return Err("No chapters could be aligned to CTC emissions".into());
    }
    let matched_any = marked_chapters.iter().any(|c| !c.sentences.is_empty());
    if !matched_any {
        return Err("No sentences matched CTC emissions".into());
    }

    let files: Vec<String> = req
        .audio_paths
        .iter()
        .filter_map(|p| p.file_name()?.to_str().map(|s| s.to_string()))
        .collect();
    let mut durations = HashMap::new();
    for (i, (name, start, end)) in book.frame_map.iter().enumerate() {
        let t = book
            .file_timings
            .get(i)
            .copied()
            .unwrap_or(book.header.timings);
        let secs = crate::align::ctc::frames_to_seconds(end.saturating_sub(*start), t);
        durations.insert(name.clone(), secs.max(0.001));
    }
    for f in &files {
        durations.entry(f.clone()).or_insert(1.0);
    }

    let matched: Vec<SentenceRange> = marked_chapters
        .iter()
        .flat_map(|c| c.sentences.iter().cloned())
        .collect();
    let chapter_order: Vec<String> = marked_chapters
        .iter()
        .map(|c| c.chapter_id.clone())
        .collect();
    let interp_opts = InterpolateOptions {
        dropped_slots: Some(&dropped_slots),
        drop_unread: pipeline.behaviour.drop_unread,
    };
    let filled = interpolate_sentence_ranges_with(
        &matched,
        &chapter_order,
        &chapter_sentence_counts,
        &durations,
        &sentence_lengths,
        &interp_opts,
    );
    let mono = enforce_monotonic_audio_ranges(&filled, &files, &durations);
    let expanded = expand_empty_sentence_ranges(&mono);
    let (collapsed, holes) = collapse_ranges(&expanded, &files, &durations, pipeline.behaviour);
    let audio_spine = AudioChapterSpine::from_pending(pending_spine);
    let track_titles = track_titles_from_audio_paths(&req.audio_paths);
    let audio_plan = AudioChapterPlan {
        track_titles: &track_titles,
        spine: Some(&audio_spine),
    };
    let (planned_audio, remaining_holes) =
        plan_audio_chapters(&holes, pipeline.behaviour, &durations, &audio_plan);
    let package_audio_duration_secs = Some(ctc_align::package_audio_duration_secs(
        &book.frame_map,
        &book.file_timings,
    ));

    let final_ranges = if pipeline.granularity == AlignGranularity::Word {
        expand_to_word_ranges_ctc(&collapsed, &words_per_sentence, &ctc_word_clips)
    } else {
        collapsed
    };

    let mut by_key: HashMap<(String, usize), SentenceRange> = HashMap::new();
    for r in final_ranges {
        by_key.insert((r.chapter_id.clone(), r.id), r);
    }
    for ch in &mut marked_chapters {
        if pipeline.granularity == AlignGranularity::Word {
            let mut ids: Vec<usize> = by_key
                .keys()
                .filter(|(cid, _)| cid == &ch.chapter_id)
                .map(|(_, id)| *id)
                .collect();
            ids.sort_unstable();
            ch.sentences = ids
                .into_iter()
                .filter_map(|id| by_key.remove(&(ch.chapter_id.clone(), id)))
                .collect();
        } else {
            let n = chapter_sentence_counts
                .get(&ch.chapter_id)
                .copied()
                .unwrap_or(0);
            ch.sentences = (0..n)
                .filter_map(|id| by_key.remove(&(ch.chapter_id.clone(), id)))
                .collect();
        }
    }
    attach_remaining_holes(&mut marked_chapters, &remaining_holes);

    let content_root = marked_chapters
        .first()
        .map(|c| {
            std::path::Path::new(&c.href.replace('\\', "/"))
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default()
        })
        .unwrap_or_default();
    marked_chapters.extend(materialize_audio_chapters(
        &planned_audio,
        &content_root,
        &spine_xhtml,
        &exclusion_ctx.language,
    ));

    report(
        "SYNC_CHAPTERS",
        0.9,
        "writing readaloud EPUB".into(),
        &mut on_progress,
    );
    write_readaloud(
        &ReadaloudInputs {
            source_epub: req.epub_path.clone(),
            chapters: marked_chapters,
            audio_files: req.audio_paths.clone(),
            package_audio_duration_secs,
            aligner: ReportAligner::Ctc,
        },
        &req.output_path,
    )?;
    report("SYNC_CHAPTERS", 1.0, "done".into(), &mut on_progress);
    Ok(())
}

enum ChapterWindow {
    /// Chapter-level findBoundaries hit — miss fallback may spread inside it.
    Trusted { start: usize, end: usize },
    /// No chapter hit — sentence search may use this span, but miss fallback
    /// must stay tiny so we do not claim the whole remaining audiobook.
    Soft { start: usize, end: usize },
}

impl ChapterWindow {
    fn span(&self) -> (usize, usize) {
        match *self {
            Self::Trusted { start, end } | Self::Soft { start, end } => (start, end),
        }
    }

    fn trusted(&self) -> bool {
        matches!(self, Self::Trusted { .. })
    }
}

fn align_sentences_in_window(
    sentences: &[String],
    transcript: &Transcript,
    slug_doc: &str,
    doc_index: &NgramIndex,
    time_map: &SlugTimeMap,
    chapter_id: &str,
    window: ChapterWindow,
) -> Result<Vec<SentenceRange>, String> {
    let (win_start, win_end) = window.span();
    // Trusted chapter windows: Storyteller getSentenceRanges via errorAlign.
    if window.trusted() {
        let via_ea = align_sentences_error_align(
            sentences, transcript, slug_doc, time_map, chapter_id, win_start, win_end,
        );
        if !via_ea.is_empty() {
            return Ok(via_ea);
        }
    }
    // Soft (or empty EA): per-sentence n-gram + score>0 gate.
    Ok(align_sentences_ngram(
        sentences, transcript, slug_doc, doc_index, time_map, chapter_id, win_start, win_end,
    ))
}

/// Storyteller-shaped windowed errorAlign inside a chapter transcript span.
fn align_sentences_error_align(
    sentences: &[String],
    transcript: &Transcript,
    slug_doc: &str,
    time_map: &SlugTimeMap,
    chapter_id: &str,
    win_start: usize,
    win_end: usize,
) -> Vec<SentenceRange> {
    let win_end = win_end.min(slug_doc.len());
    let win_start = win_start.min(win_end);
    if win_end <= win_start {
        return Vec::new();
    }
    let chapter_hyp = &slug_doc[win_start..win_end];
    let queries: Vec<String> = sentences.iter().map(|s| slugify_for_search(s)).collect();

    let mut out = Vec::new();
    let mut sentence_idx = 0usize;
    let mut hyp_cursor = 0usize;

    while sentence_idx < queries.len() && hyp_cursor < chapter_hyp.len() {
        let mut window_sents: Vec<String> = Vec::new();
        let mut window_len = 0usize;
        let mut i = sentence_idx;
        while window_len < 5000 && i < queries.len() {
            window_sents.push(queries[i].clone());
            window_len += queries[i].len();
            i += 1;
        }
        let remaining: usize = queries[i..].iter().map(|s| s.len()).sum();
        if remaining < 5000 {
            while i < queries.len() {
                window_sents.push(queries[i].clone());
                i += 1;
            }
        }
        if window_sents.is_empty() {
            break;
        }

        let hyp_take = ((window_len as f64) * 1.2).ceil() as usize;
        let mut hyp_slice_end = (hyp_cursor + hyp_take).min(chapter_hyp.len());
        if hyp_slice_end < chapter_hyp.len()
            && hyp_slice_end > hyp_cursor
            && !chapter_hyp[hyp_slice_end..].starts_with('-')
        {
            if let Some(rel) = chapter_hyp[hyp_slice_end..].find('-') {
                hyp_slice_end += rel;
            }
        }
        let hyp_slice = &chapter_hyp[hyp_cursor..hyp_slice_end.max(hyp_cursor)];
        if hyp_slice.is_empty() {
            break;
        }

        // Storyteller: narrow first/last chapter windows before scoring sentences.
        let at_chapter_start = sentence_idx == 0;
        let at_chapter_end = i == queries.len();
        let (alignments, slice_start, slice_end) = if at_chapter_start || at_chapter_end {
            error_align_with_narrowing(&window_sents, hyp_slice, at_chapter_start, at_chapter_end)
        } else {
            (
                error_align(&window_sents.join("-"), hyp_slice),
                0,
                window_sents.len(),
            )
        };
        let window_view = &window_sents[slice_start..slice_end];
        if window_view.is_empty() {
            break;
        }

        let mut align_idx = 0usize;
        let mut hyp_local = 0usize;
        let mut last_good_j: Option<usize> = None;
        let mut last_good_hyp = 0usize;

        for (j, sent) in window_view.iter().enumerate() {
            if sent.is_empty() {
                continue;
            }
            if align_idx >= alignments.len() {
                break;
            }
            let (used, score) = alignments_for_sentence(sent, &alignments[align_idx..]);
            if used == 0 {
                break;
            }
            let span_ops = &alignments[align_idx..align_idx + used];
            let hyp_chars = hyp_span_chars(span_ops);

            if score > 0.0 && hyp_chars > 0 {
                let abs_start = win_start + hyp_cursor + hyp_local;
                let abs_end = (abs_start + hyp_chars).min(slug_doc.len());
                if abs_end > abs_start {
                    if let Some((start_t, end_t, audiofile)) =
                        time_map.times_for_span(slug_doc, abs_start, abs_end, transcript)
                    {
                        out.push(
                            SentenceRange::new(
                                sentence_idx + slice_start + j,
                                chapter_id,
                                vec![AudioRange {
                                    audiofile,
                                    start: start_t,
                                    end: end_t.max(start_t + 0.001),
                                }],
                            )
                            .with_slug_span(abs_start, abs_end),
                        );
                    }
                }
                last_good_j = Some(j);
                last_good_hyp = hyp_local + hyp_chars;
            }

            align_idx += used;
            hyp_local += hyp_chars;
            if hyp_slice.as_bytes().get(hyp_local) == Some(&b'-') {
                hyp_local += 1;
            }
        }

        let Some(good_j) = last_good_j else {
            break;
        };
        sentence_idx += slice_start + good_j + 1;
        hyp_cursor += last_good_hyp;
        if chapter_hyp.as_bytes().get(hyp_cursor) == Some(&b'-') {
            hyp_cursor += 1;
        }
    }
    out
}

#[allow(clippy::too_many_arguments)] // mirrors align_sentences_error_align window args
fn align_sentences_ngram(
    sentences: &[String],
    transcript: &Transcript,
    slug_doc: &str,
    doc_index: &NgramIndex,
    time_map: &SlugTimeMap,
    chapter_id: &str,
    win_start: usize,
    win_end: usize,
) -> Vec<SentenceRange> {
    let window_doc = if win_end > win_start && win_end <= slug_doc.len() {
        &slug_doc[win_start..win_end]
    } else {
        slug_doc
    };
    let window_index = if std::ptr::eq(window_doc as *const str, slug_doc as *const str) {
        None
    } else {
        Some(NgramIndex::build(window_doc, 5))
    };
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for (id, sentence) in sentences.iter().enumerate() {
        let query = slugify_for_search(sentence);
        let hit = window_index
            .as_ref()
            .and_then(|idx| {
                find_boundaries_with_index(&query, window_doc, idx, cursor, 1000)
                    .or_else(|| find_boundaries_with_index(&query, window_doc, idx, 0, 1000))
            })
            .or_else(|| {
                find_boundaries_with_index(&query, slug_doc, doc_index, win_start + cursor, 1000)
                    .or_else(|| find_boundaries_with_index(&query, slug_doc, doc_index, 0, 1000))
            });
        let Some(hit) = hit else {
            continue;
        };
        let abs_start = if std::ptr::eq(window_doc as *const str, slug_doc as *const str) {
            hit.start
        } else {
            win_start + hit.start
        };
        let abs_end = if std::ptr::eq(window_doc as *const str, slug_doc as *const str) {
            hit.end
        } else {
            win_start + hit.end
        };
        if abs_end <= abs_start + 1 {
            continue;
        }
        let abs_end = abs_end.min(slug_doc.len());
        let abs_start = abs_start.min(abs_end);
        let hyp = &slug_doc[abs_start..abs_end];
        if !accept_match(&query, hyp) {
            continue;
        }
        let Some((start_t, end_t, audiofile)) =
            time_map.times_for_span(slug_doc, abs_start, abs_end, transcript)
        else {
            continue;
        };
        cursor = hit.end;
        out.push(
            SentenceRange::new(
                id,
                chapter_id,
                vec![AudioRange {
                    audiofile,
                    start: start_t,
                    end: end_t.max(start_t + 0.001),
                }],
            )
            .with_slug_span(abs_start, abs_end),
        );
    }
    out
}

fn open_epub_zip(epub: &Path) -> Result<ZipArchive<File>, String> {
    ZipArchive::new(File::open(epub).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn read_first_opf(archive: &mut ZipArchive<File>) -> Result<(String, String), String> {
    let names: Vec<String> = (0..archive.len())
        .filter_map(|i| {
            archive
                .by_index(i)
                .ok()
                .map(|f| f.name().replace('\\', "/"))
        })
        .collect();
    let opf_name = names
        .into_iter()
        .find(|n| n.ends_with(".opf"))
        .ok_or_else(|| "OPF missing".to_string())?;
    let body = read_zip_entry(archive, &opf_name)?;
    Ok((opf_name, body))
}

fn resolve_against_opf(opf_name: &str, href: &str) -> String {
    let href = href.replace('\\', "/");
    let opf_dir = Path::new(opf_name)
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    if opf_dir.is_empty() {
        href
    } else {
        format!("{opf_dir}/{href}").replace("//", "/")
    }
}

fn single_chapter_meta(href: String) -> SpineItemMeta {
    SpineItemMeta {
        index: 0,
        spine_len: 1,
        id: "chapter".into(),
        href,
        linear: true,
        media_type: "application/xhtml+xml".into(),
        properties: String::new(),
    }
}

fn load_exclusion_context(
    archive: &mut ZipArchive<File>,
) -> crate::align::exclusions::EpubExclusionContext {
    let Ok((opf_name, opf)) = read_first_opf(archive) else {
        return crate::align::exclusions::EpubExclusionContext::default();
    };
    let nav_xhtml = nav_document_href(&opf).and_then(|rel| {
        let path = resolve_against_opf(&opf_name, &rel);
        read_zip_entry(archive, &path).ok()
    });
    let mut ctx = exclusion_context_from_opf(&opf, nav_xhtml.as_deref());
    // Re-key OPF-relative hrefs against the zip paths spine uses (opf_dir/href).
    let opf_dir = Path::new(&opf_name)
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    if !opf_dir.is_empty() {
        ctx.landmark_types = rekey_map(ctx.landmark_types, &opf_dir);
        ctx.guide_types = rekey_map(ctx.guide_types, &opf_dir);
        ctx.toc_titles = rekey_string_map(ctx.toc_titles, &opf_dir);
        ctx.nav_property_hrefs = ctx
            .nav_property_hrefs
            .into_iter()
            .map(|h| join_opf(&opf_dir, &h))
            .collect();
    }
    ctx
}

fn join_opf(opf_dir: &str, href: &str) -> String {
    if href.starts_with(&format!("{opf_dir}/")) || opf_dir.is_empty() {
        href.to_string()
    } else {
        format!("{opf_dir}/{href}").replace("//", "/")
    }
}

fn rekey_map(
    map: std::collections::HashMap<String, Vec<String>>,
    opf_dir: &str,
) -> std::collections::HashMap<String, Vec<String>> {
    map.into_iter()
        .map(|(k, v)| (join_opf(opf_dir, &k), v))
        .collect()
}

fn rekey_string_map(
    map: std::collections::HashMap<String, String>,
    opf_dir: &str,
) -> std::collections::HashMap<String, String> {
    map.into_iter()
        .map(|(k, v)| (join_opf(opf_dir, &k), v))
        .collect()
}

fn read_zip_entry(archive: &mut ZipArchive<File>, href: &str) -> Result<String, String> {
    let href = href.replace('\\', "/");
    let resolved = {
        if archive.by_name(&href).is_ok() {
            href.clone()
        } else {
            let mut found = None;
            for i in 0..archive.len() {
                let name = archive
                    .by_index(i)
                    .map(|f| f.name().replace('\\', "/"))
                    .map_err(|e| e.to_string())?;
                if name.eq_ignore_ascii_case(&href) {
                    found = Some(name);
                    break;
                }
            }
            found.ok_or_else(|| format!("Missing EPUB entry {href}"))?
        }
    };
    let mut file = archive
        .by_name(&resolved)
        .map_err(|_| format!("Missing EPUB entry {href}"))?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::transcribe::ScriptedTranscriber;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn multi_epub(dir: &Path) -> PathBuf {
        let path = dir.join("book.epub");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default();
        zip.start_file("META-INF/container.xml", opts).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/content.opf", opts).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">shelf-align-multi</dc:identifier>
    <dc:title>Align Multi</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="chapter2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="nav"/>
    <itemref idref="ch1"/>
    <itemref idref="ch2"/>
  </spine>
</package>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/nav.xhtml", opts).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>nav</title></head><body><nav epub:type="toc"><ol>
<li><a href="chapter1.xhtml">c1</a></li><li><a href="chapter2.xhtml">c2</a></li>
</ol></nav></body></html>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/chapter1.xhtml", opts).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>c1</title></head>
<body><p>Hello world. Goodbye moon!</p></body></html>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/chapter2.xhtml", opts).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>c2</title></head>
<body><p>Second chapter begins. Stars shine brightly tonight.</p></body></html>"#,
        )
        .unwrap();
        zip.finish().unwrap();
        path
    }

    #[test]
    fn pipeline_builds_multichapter_readaloud() {
        let dir = tempfile::tempdir().unwrap();
        let epub = multi_epub(dir.path());
        let audio = dir.path().join("narration.mp3");
        std::fs::write(&audio, b"ID3fake").unwrap();
        let out = dir.path().join("readaloud.epub");
        let script =
            "Hello world. Goodbye moon! Second chapter begins. Stars shine brightly tonight."
                .to_string();
        let t = ScriptedTranscriber {
            script,
            duration_secs: 8.0,
        };
        run_align(
            &AlignPipeline::new(&t),
            &AlignRequest {
                epub_path: epub,
                audio_paths: vec![audio],
                output_path: out.clone(),
                chapter_href: None,
            },
            None,
        )
        .expect("align");
        assert!(out.is_file());
        let data = std::fs::read(&out).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).unwrap();
        let names: Vec<_> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n.contains("chapter1.smil")),
            "{names:?}"
        );
        assert!(
            names.iter().any(|n| n.contains("chapter2.smil")),
            "{names:?}"
        );

        // EPUB3 / Foliate: media-overlay on manifest <item>, not only spine itemref.
        let mut opf = String::new();
        zip.by_name("OEBPS/content.opf")
            .unwrap()
            .read_to_string(&mut opf)
            .unwrap();
        let item_mo = opf
            .split('<')
            .filter(|t| {
                t.starts_with("item ") || t.starts_with("item\t") || t.starts_with("item\n")
            })
            .filter(|t| !t.starts_with("itemref"))
            .filter(|t| t.contains("media-overlay="))
            .count();
        assert!(item_mo >= 2, "manifest media-overlay missing: {opf}");
        // Self-closing source items must stay well-formed after attach.
        assert!(
            !opf.contains("/> media-overlay=") && !opf.contains("/ media-overlay="),
            "media-overlay inserted after self-close slash: {opf}"
        );
        assert!(
            opf.contains("Audio/narration.mp3") && opf.contains("audio/mpeg"),
            "audio missing from manifest: {opf}"
        );
        assert!(
            opf.contains("property=\"media:duration\"") && opf.contains("refines=\"#mo"),
            "media:duration package meta missing: {opf}"
        );
        // DOM rewrite must stay well-formed XML.
        xmltree::Element::parse(opf.as_bytes()).expect("OPF must parse after wire");
        // SMIL text src is relative to MediaOverlays/
        let mut smil = String::new();
        let smil_name = names.iter().find(|n| n.contains("chapter1.smil")).unwrap();
        zip.by_name(smil_name)
            .unwrap()
            .read_to_string(&mut smil)
            .unwrap();
        assert!(
            smil.contains("../") && smil.contains("chapter1.xhtml#"),
            "smil text path should be relative to MediaOverlays: {smil}"
        );
    }

    #[test]
    fn soft_fail_rejects_near_eof_while_cursor_in_front_half() {
        // Front-zone / outro leap: match at 95% while cursor still at 10%.
        assert!(soft_fail_discontinuous_outro(950, 100, 1000));
        assert!(soft_fail_discontinuous_outro(901, 0, 1000));
        // Cursor already past mid — keep the match (not a title→outro leap).
        assert!(!soft_fail_discontinuous_outro(950, 500, 1000));
        assert!(!soft_fail_discontinuous_outro(950, 600, 1000));
        // Match not near EOF.
        assert!(!soft_fail_discontinuous_outro(800, 100, 1000));
        assert!(!soft_fail_discontinuous_outro(900, 100, 1000)); // boundary: > 0.9
    }

    #[test]
    fn utf16_len_bmp_equals_scalar_astral_is_two() {
        assert_eq!(utf16_len("café"), 4); // é is one UTF-16 unit
        assert_eq!(utf16_len("café"), "café".chars().count());
        assert_eq!(utf16_len("a😀b"), 4); // 😀 = 2 UTF-16 units
        assert_ne!(utf16_len("a😀b"), "a😀b".chars().count());
        assert_ne!(utf16_len("a😀b"), "a😀b".len()); // 6 UTF-8 bytes
    }

    #[test]
    fn map_transcription_timeline_applies_mapping() {
        let mut mapping = Mapping::new();
        // Expand first char 1→3 (ST insert_map style).
        mapping.insert_map(0, 1, 3);
        let timeline = vec![TimelineEntry {
            start_time: 0.0,
            end_time: 1.0,
            audiofile: "a.mp3".into(),
            start_offset_utf16: 0,
            end_offset_utf16: 5,
        }];
        let mapped = map_transcription_timeline(&timeline, &mut mapping);
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].mapped_start_offset_utf16, 0);
        // End at 5 shifts by +2 from the expansion at 0.
        assert_eq!(mapped[0].mapped_end_offset_utf16, 7);
    }

    #[test]
    fn slug_time_map_utf16_lookup_matches_ascii_bytes() {
        use crate::align::transcribe::TimedWord;
        let transcript = Transcript {
            words: vec![
                TimedWord {
                    text: "Hello".into(),
                    start: 0.0,
                    end: 0.5,
                    audiofile: "a.mp3".into(),
                },
                TimedWord {
                    text: "world".into(),
                    start: 0.5,
                    end: 1.0,
                    audiofile: "a.mp3".into(),
                },
            ],
        };
        let map = SlugTimeMap::build(&transcript);
        let doc = "hello-world";
        let (s, e, file) = map.times_for_span(doc, 0, 5, &transcript).unwrap();
        assert!((s - 0.0).abs() < 1e-9);
        assert!((e - 0.5).abs() < 1e-9);
        assert_eq!(file, "a.mp3");
        let (s2, e2, _) = map.times_for_span(doc, 6, 11, &transcript).unwrap();
        assert!((s2 - 0.5).abs() < 1e-9);
        assert!((e2 - 1.0).abs() < 1e-9);
    }
}
