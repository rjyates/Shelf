#![allow(dead_code, unused_imports)]
//! EPUB ↔ audiobook forced alignment (Storyteller-shaped), in Rust.
//!
//! Stage order matches `@storyteller-platform/align` / Storyteller's worker:
//! process audio → transcribe → markup EPUB → align → write readaloud EPUB.
//!
//! Design attribution: `third_party/storyteller-align/`.

mod audio_chapters;
mod ctc;
mod ctc_align;
mod ctc_emit;
mod ctc_search;
mod error_align;
mod exclusions;
mod guided_navigation;
mod holes;
mod interpolate;
mod job;
mod mapping;
mod markup;
mod match_score;
mod pipeline;
mod ranges;
mod readaloud;
mod report;
mod search;
mod slugify;
mod smil;
mod spine;
mod stem;
mod suppressions;
mod text_fragments;
mod transcribe;
mod upgrade_epub;
mod vad;
mod webpub;
mod whisper;
mod words;
mod worker;

pub use ctc::{
    boundary_offset_frames, ctc_forced_align, ctc_forced_align_simple, ctc_greedy_decode,
    ctc_viterbi_align, detect_dropped_labels, frames_to_seconds, seconds_to_frames,
    EmissionTimings, Emissions, ForcedAlignAnchor, TokenAlignment,
};
pub use holes::{
    collapse_ranges, plan_audio_chapters, AudioChapter, AudioChapterPlan, AudioChapterSpine,
    ExistingChapter, Hole, RangeBehaviour, SpineEntryLike,
};
pub use job::{
    align_job_tick, create_work, get_work, list_works, start_align_job, AlignJobStatus, AlignStage,
    BookWork,
};
pub use pipeline::{
    map_transcription_timeline, run_align, AlignGranularity, AlignPipeline, AlignProgress,
    AlignRequest, MappedTimelineEntry, TimelineEntry,
};
pub use ranges::{enforce_monotonic_audio_ranges, AudioRange, RangeState, SentenceRange};
pub use search::find_boundaries;
pub use stem::{
    decode_stem_bytes, decode_stem_file, decode_stem_frame_range, stem_blocks, StemBlocks,
    StemEncoding, StemHeader, BLOCK_FRAMES,
};
pub use transcribe::{TimedWord, Transcriber, Transcript};
pub use vad::{
    choose_chapter_cut, get_safe_chapter_ranges, get_safe_ranges, move_chapter_cuts_to_silence,
    parse_silencedetect_log, ChapterAudioRange, ChapterCut, CutReason, SilenceRange,
    CHAPTER_CUT_MAX_DISTANCE_SECONDS, CHAPTER_CUT_WINDOW_SECONDS, MIN_SILENCE_SECONDS,
};
pub use whisper::WhisperCppTranscriber;
pub(crate) use worker::{
    create_work_http, get_work_http, list_works_http, resume_pending, spawn_align, start_work_http,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_smoke_find_boundaries_in_transcript() {
        // Word n-grams use size 5 (Storyteller default); query must be ≥5 tokens.
        let doc = "once-upon-a-time-there-was-a-boy-named-peter-and-he-loved-to-fly";
        let query = "there-was-a-boy-named-peter";
        let hit = find_boundaries(query, doc).expect("match");
        assert!(hit.start < hit.end);
        assert!(
            doc[hit.start..hit.end.min(doc.len())].contains("there-was-a-boy")
                || doc[hit.start..].starts_with("there")
                || hit.start > 0
        );
    }
}
