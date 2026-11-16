//! whisper.cpp transcription (Storyteller's default engine path).
//!
//! Invokes `whisper-cli` with `--output-json-full`. In Docker Compose the binary
//! and a default GGML model ship inside the server image — end users do not
//! install anything. `--print-progress` stderr lines drive live Combine %.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde::Deserialize;

use super::transcribe::{TimedWord, Transcriber, Transcript};

#[derive(Debug, Clone)]
pub struct WhisperCppTranscriber {
    pub binary: PathBuf,
    pub model: PathBuf,
    pub language: String,
    pub threads: usize,
}

impl WhisperCppTranscriber {
    /// Resolve binary + model. Prefer image-bundled paths, then env, then PATH/home.
    pub fn detect() -> Option<Self> {
        let binary = std::env::var_os("SHELF_WHISPER_CLI")
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .or_else(|| {
                [
                    PathBuf::from("/usr/local/bin/whisper-cli"),
                    PathBuf::from("/usr/bin/whisper-cli"),
                ]
                .into_iter()
                .find(|p| p.is_file())
            })
            .or_else(|| which("whisper-cli"))?;

        let model = std::env::var_os("SHELF_WHISPER_MODEL")
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .or_else(|| {
                let mut candidates = vec![
                    PathBuf::from("/usr/share/shelf/models/ggml-tiny.en.bin"),
                    PathBuf::from("/usr/share/shelf/models/ggml-base.en.bin"),
                ];
                if let Ok(data) = std::env::var("SHELF_DATA") {
                    candidates.push(PathBuf::from(data).join("models/ggml-tiny.en.bin"));
                }
                if let Some(home) = dirs_home() {
                    candidates.extend([
                        home.join("models/ggml-tiny.en.bin"),
                        home.join(".cache/whisper/ggml-tiny.en.bin"),
                    ]);
                }
                candidates.into_iter().find(|p| p.is_file())
            })?;

        Some(Self {
            binary,
            model,
            language: std::env::var("SHELF_WHISPER_LANG").unwrap_or_else(|_| "en".into()),
            threads: std::env::var("SHELF_WHISPER_THREADS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(default_whisper_threads),
        })
    }
}

fn default_whisper_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 16)
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Parse a whisper-cli `--print-progress` stderr line into 0.0..=1.0.
pub fn parse_whisper_progress_percent(line: &str) -> Option<f32> {
    let lower = line.to_ascii_lowercase();
    let idx = lower.find("progress")?;
    let rest = &line[idx..];
    let digits: String = rest
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let n: f32 = digits.parse().ok()?;
    if n > 100.0 {
        return None;
    }
    Some((n / 100.0).clamp(0.0, 1.0))
}

impl Transcriber for WhisperCppTranscriber {
    fn transcribe_file(&self, audio: &Path) -> Result<Transcript, String> {
        self.transcribe_file_with_progress(audio, &mut |_| {})
    }

    fn transcribe_file_with_progress(
        &self,
        audio: &Path,
        on_progress: &mut dyn FnMut(f32),
    ) -> Result<Transcript, String> {
        on_progress(0.0);
        let work = tempfile::tempdir().map_err(|e| e.to_string())?;
        // Prefer the source file when whisper.cpp can decode it (skips multi-GB WAV).
        // Fall back to 16 kHz mono PCM when the CLI lacks ffmpeg demux.
        let input = match try_whisper_accepts_source(audio) {
            true => {
                on_progress(0.02);
                audio.to_path_buf()
            }
            false => {
                let wav = ensure_wav_16k_mono(audio, work.path())?;
                on_progress(0.05);
                wav
            }
        };
        let out_base = work.path().join("out");
        let mut child = Command::new(&self.binary)
            .arg("-m")
            .arg(&self.model)
            .arg("-f")
            .arg(&input)
            .arg("-l")
            .arg(&self.language)
            .arg("-t")
            .arg(self.threads.to_string())
            .arg("--output-json-full")
            .arg("-of")
            .arg(&out_base)
            .arg("--print-progress")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn whisper-cli: {e}"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "whisper-cli stderr missing".to_string())?;
        let (tx, rx) = mpsc::channel::<f32>();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some(p) = parse_whisper_progress_percent(&line) {
                    let _ = tx.send(p);
                }
            }
        });
        let status = loop {
            while let Ok(p) = rx.try_recv() {
                // Leave headroom for the 0.05 ffmpeg slice; map whisper 0..=1 → 0.05..=1.0.
                on_progress(0.05 + 0.95 * p);
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(200)),
                Err(e) => return Err(format!("Failed waiting for whisper-cli: {e}")),
            }
        };
        while let Ok(p) = rx.try_recv() {
            on_progress(0.05 + 0.95 * p);
        }
        let _ = reader.join();
        if !status.success() {
            return Err(format!("whisper-cli exited with {status}"));
        }
        on_progress(1.0);
        let json_path = out_base.with_extension("json");
        let json = std::fs::read_to_string(&json_path).map_err(|e| {
            format!(
                "whisper-cli produced no JSON at {}: {e}",
                json_path.display()
            )
        })?;
        let name = audio
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("audio.mp3")
            .to_string();
        parse_whisper_json_full(&json, &name)
    }
}

fn try_whisper_accepts_source(audio: &Path) -> bool {
    let ext = audio
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    // WAV is always fine. Other containers need a whisper build with ffmpeg;
    // opt in via SHELF_WHISPER_NATIVE_AUDIO=1 once the image supports it.
    if ext == "wav" {
        return true;
    }
    matches!(
        std::env::var("SHELF_WHISPER_NATIVE_AUDIO").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

fn ensure_wav_16k_mono(audio: &Path, work: &Path) -> Result<PathBuf, String> {
    let ext = audio
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "wav" {
        return Ok(audio.to_path_buf());
    }
    let wav = work.join("input.wav");
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            audio.to_str().ok_or("bad audio path")?,
            "-ar",
            "16000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(&wav)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("ffmpeg missing or failed: {e}"))?;
    if !status.success() {
        return Err("ffmpeg could not convert audio for whisper".into());
    }
    Ok(wav)
}

#[derive(Debug, Deserialize)]
struct WhisperFull {
    transcription: Option<Vec<WhisperSegment>>,
}

#[derive(Debug, Deserialize)]
struct WhisperSegment {
    text: Option<String>,
    offsets: Option<WhisperOffsets>,
    tokens: Option<Vec<WhisperToken>>,
}

#[derive(Debug, Deserialize)]
struct WhisperOffsets {
    from: Option<i64>,
    to: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct WhisperToken {
    text: Option<String>,
    offsets: Option<WhisperOffsets>,
}

/// Parse whisper-cli `--output-json-full` into timed words.
pub fn parse_whisper_json_full(json: &str, audiofile: &str) -> Result<Transcript, String> {
    let parsed: WhisperFull =
        serde_json::from_str(json).map_err(|e| format!("Invalid whisper JSON: {e}"))?;
    let segments = parsed.transcription.unwrap_or_default();
    let mut words = Vec::new();
    for seg in segments {
        if let Some(tokens) = seg.tokens {
            for tok in tokens {
                let text = tok.text.unwrap_or_default();
                let trimmed = text.trim();
                if trimmed.is_empty() || trimmed.starts_with('[') {
                    continue;
                }
                let (start_ms, end_ms) = match tok.offsets {
                    Some(o) => (o.from.unwrap_or(0), o.to.unwrap_or(0)),
                    None => continue,
                };
                let start = start_ms as f64 / 1000.0;
                let mut end = end_ms as f64 / 1000.0;
                if end <= start {
                    end = start + 0.001;
                }
                words.push(TimedWord {
                    text: trimmed.to_string(),
                    start,
                    end,
                    audiofile: audiofile.to_string(),
                });
            }
        } else if let Some(text) = seg.text {
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            let (start_ms, end_ms) = match seg.offsets {
                Some(o) => (o.from.unwrap_or(0), o.to.unwrap_or(0)),
                None => (0, 0),
            };
            let start = start_ms as f64 / 1000.0;
            let end = (end_ms as f64 / 1000.0).max(start + 0.05);
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }
            let step = (end - start) / parts.len() as f64;
            for (i, w) in parts.iter().enumerate() {
                words.push(TimedWord {
                    text: (*w).to_string(),
                    start: start + i as f64 * step,
                    end: start + (i as f64 + 1.0) * step,
                    audiofile: audiofile.to_string(),
                });
            }
        }
    }
    if words.is_empty() {
        return Err("whisper produced no words".into());
    }
    Ok(Transcript { words })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_token_level_json_full() {
        let json = r#"{
          "transcription": [{
            "text": " Hello world",
            "offsets": {"from": 0, "to": 1000},
            "tokens": [
              {"text": "[_BEG_]", "offsets": {"from": 0, "to": 0}},
              {"text": " Hello", "offsets": {"from": 100, "to": 400}},
              {"text": " world", "offsets": {"from": 400, "to": 900}}
            ]
          }]
        }"#;
        let t = parse_whisper_json_full(json, "a.mp3").unwrap();
        assert_eq!(t.words.len(), 2);
        assert_eq!(t.words[0].text, "Hello");
        assert!((t.words[0].start - 0.1).abs() < 1e-9);
        assert_eq!(t.words[1].text, "world");
        assert_eq!(t.words[1].audiofile, "a.mp3");
    }

    #[test]
    fn parses_print_progress_lines() {
        assert_eq!(
            parse_whisper_progress_percent("whisper_print_progress_callback: progress =  42%"),
            Some(0.42)
        );
        assert_eq!(parse_whisper_progress_percent("progress = 100%"), Some(1.0));
        assert_eq!(parse_whisper_progress_percent("nope"), None);
    }
}
