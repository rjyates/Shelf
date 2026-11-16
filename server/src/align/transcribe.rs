//! Transcription interface for the align pipeline.
//!
//! Storyteller supports Whisper and CTC emit; Shelf uses a trait so tests use a
//! deterministic fake and production uses whisper.cpp when bundled.

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct TimedWord {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub audiofile: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub words: Vec<TimedWord>,
}

impl Transcript {
    pub fn joined_text(&self) -> String {
        self.words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub trait Transcriber: Send + Sync {
    fn transcribe_file(&self, audio: &Path) -> Result<Transcript, String>;

    /// Same as [`Self::transcribe_file`], with `on_progress` called as 0.0..=1.0
    /// while work runs (ffmpeg prep + whisper). Default ignores progress.
    fn transcribe_file_with_progress(
        &self,
        audio: &Path,
        on_progress: &mut dyn FnMut(f32),
    ) -> Result<Transcript, String> {
        let _ = on_progress;
        self.transcribe_file(audio)
    }
}

/// Test double: spreads `script` evenly across `duration_secs`.
pub struct ScriptedTranscriber {
    pub script: String,
    pub duration_secs: f64,
}

impl Transcriber for ScriptedTranscriber {
    fn transcribe_file(&self, audio: &Path) -> Result<Transcript, String> {
        let words: Vec<&str> = self.script.split_whitespace().collect();
        if words.is_empty() {
            return Ok(Transcript { words: Vec::new() });
        }
        let step = self.duration_secs / words.len() as f64;
        let name = audio
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("audio.mp3")
            .to_string();
        Ok(Transcript {
            words: words
                .iter()
                .enumerate()
                .map(|(i, w)| TimedWord {
                    text: (*w).to_string(),
                    start: i as f64 * step,
                    end: (i as f64 + 1.0) * step,
                    audiofile: name.clone(),
                })
                .collect(),
        })
    }
}

/// Placeholder when Combine should use CTC emit (not the Whisper transcript path).
pub struct MissingTranscriber;

impl Transcriber for MissingTranscriber {
    fn transcribe_file(&self, _audio: &Path) -> Result<Transcript, String> {
        Err(
            "Alignment uses CTC emit (ghost-story). This Whisper transcript path should not run in production."
                .into(),
        )
    }
}
