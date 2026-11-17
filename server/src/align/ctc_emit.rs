//! Sync HTTP client for Storyteller `ghost-story serve` CTC emissions.
//!
//! Protocol: upload audio by SHA-256 → POST job → poll → download STEM bytes.
//! Combine always uses CTC emit (`worker` → `run_align_ctc`). Defaults to loopback
//! ghost-story inside the Shelf image (`http://127.0.0.1:8765`, key `shelf-internal`).

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

const WAIT_SECONDS: u64 = 25;
/// Long audiobooks can take hours on CPU MMS.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone)]
pub struct CtcEmitClient {
    pub(crate) base_url: String,
    pub(crate) api_key: String,
}

#[derive(Debug, Clone, Default)]
pub struct CtcEmitOpts {
    pub model: Option<String>,
    pub language: Option<String>,
    pub dtype: Option<String>,
    /// Inclusive start sample / second range as understood by ghost-story (optional).
    pub range_start: Option<f64>,
    pub range_end: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct JobResponse {
    id: String,
    state: String,
    revision: u64,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    result: Option<JobResult>,
}

#[derive(Debug, Deserialize)]
struct JobResult {
    emissions: EmissionsRef,
}

#[derive(Debug, Deserialize)]
struct EmissionsRef {
    sha256: String,
}

impl CtcEmitClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        let base = base_url.into().trim_end_matches('/').to_string();
        Self {
            base_url: base,
            api_key: api_key.into(),
        }
    }

    /// `SHELF_CTC_EMIT_URL` / `SHELF_CTC_EMIT_API_KEY`, defaulting to in-image ghost-story.
    pub fn from_env() -> Self {
        let url = std::env::var("SHELF_CTC_EMIT_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "http://127.0.0.1:8765".into());
        let key = std::env::var("SHELF_CTC_EMIT_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "shelf-internal".into());
        Self::new(url, key)
    }

    fn http(&self) -> Result<reqwest::blocking::Client, String> {
        reqwest::blocking::Client::builder()
            .timeout(CLIENT_TIMEOUT)
            .build()
            .map_err(|e| format!("ctc emit client: {e}"))
    }

    fn auth(&self, req: reqwest::blocking::RequestBuilder) -> reqwest::blocking::RequestBuilder {
        req.header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", self.api_key),
        )
    }

    /// Upload `audio`, run emissions job, write STEM body to `work_dir/{audio_stem}.stem`.
    ///
    /// Reuses an existing STEM when present (Storyteller emit cache) so a failed
    /// SYNC can restart without re-emitting — including a sole `*.stem` left from
    /// an older emissions-sha filename.
    pub fn emit_file(
        &self,
        audio: &Path,
        work_dir: &Path,
        opts: &CtcEmitOpts,
    ) -> Result<PathBuf, String> {
        fs::create_dir_all(work_dir).map_err(|e| format!("ctc emit work_dir: {e}"))?;
        if let Some(cached) = cached_stem_for_audio(work_dir, audio) {
            return Ok(cached);
        }
        let file_sha = sha256_file(audio)?;
        self.ensure_uploaded(audio, &file_sha)?;

        let job_id = self.create_job(&file_sha, opts)?;
        let emissions_sha = match self.poll_job(&job_id) {
            Ok(sha) => {
                let _ = self.delete_job(&job_id);
                sha
            }
            Err(e) => {
                let _ = self.delete_job(&job_id);
                return Err(e);
            }
        };

        let out = stem_cache_path(work_dir, audio);
        let bytes = self.download_file(&emissions_sha)?;
        let mut f = File::create(&out).map_err(|e| format!("ctc emit write: {e}"))?;
        f.write_all(&bytes)
            .map_err(|e| format!("ctc emit write: {e}"))?;
        Ok(out)
    }

    fn ensure_uploaded(&self, audio: &Path, sha256: &str) -> Result<(), String> {
        let client = self.http()?;
        let head_url = format!("{}/v1/files/{sha256}", self.base_url);
        let head = self
            .auth(client.head(&head_url))
            .send()
            .map_err(|e| format!("ctc emit HEAD: {e}"))?;
        if head.status().as_u16() == 200 {
            return Ok(());
        }
        if head.status().as_u16() != 404 {
            return Err(format!(
                "ctc emit HEAD {}: {}",
                head.status(),
                head.text().unwrap_or_default()
            ));
        }

        let ext = audio
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        let put_url = if ext.is_empty() {
            format!("{}/v1/files/{sha256}", self.base_url)
        } else {
            format!("{}/v1/files/{sha256}?ext={ext}", self.base_url)
        };
        let body = fs::read(audio).map_err(|e| format!("ctc emit read audio: {e}"))?;
        let put = self
            .auth(client.put(&put_url))
            .body(body)
            .send()
            .map_err(|e| format!("ctc emit PUT: {e}"))?;
        if !put.status().is_success() {
            return Err(format!(
                "ctc emit PUT {}: {}",
                put.status(),
                put.text().unwrap_or_default()
            ));
        }
        Ok(())
    }

    fn create_job(&self, file_sha: &str, opts: &CtcEmitOpts) -> Result<String, String> {
        let client = self.http()?;
        let url = format!("{}/v1/emissions/jobs", self.base_url);
        let mut body = serde_json::json!({ "file": file_sha });
        if let Some(m) = &opts.model {
            body["model"] = serde_json::json!(m);
        }
        if let Some(l) = &opts.language {
            body["language"] = serde_json::json!(l);
        }
        if let Some(d) = &opts.dtype {
            body["dtype"] = serde_json::json!(d);
        }
        if opts.range_start.is_some() || opts.range_end.is_some() {
            let mut range = serde_json::Map::new();
            if let Some(s) = opts.range_start {
                range.insert("start".into(), serde_json::json!(s));
            }
            if let Some(e) = opts.range_end {
                range.insert("end".into(), serde_json::json!(e));
            }
            body["range"] = serde_json::Value::Object(range);
        }

        let resp = self
            .auth(client.post(&url))
            .json(&body)
            .send()
            .map_err(|e| format!("ctc emit POST job: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!(
                "ctc emit POST job {}: {}",
                resp.status(),
                resp.text().unwrap_or_default()
            ));
        }
        let job: JobResponse = resp
            .json()
            .map_err(|e| format!("ctc emit POST job json: {e}"))?;
        Ok(job.id)
    }

    fn poll_job(&self, job_id: &str) -> Result<String, String> {
        let client = self.http()?;
        let mut revision: u64 = 0;
        let mut send_failures: u32 = 0;
        loop {
            let url = format!(
                "{}/v1/emissions/jobs/{job_id}?wait={WAIT_SECONDS}&revision={revision}",
                self.base_url
            );
            let resp = match self.auth(client.get(&url)).send() {
                Ok(r) => {
                    send_failures = 0;
                    r
                }
                // ghost-story can drop the long-poll when the process is OOM-killed or
                // briefly restarting; a few soft retries beat failing the whole Combine.
                Err(_e) if send_failures < 8 => {
                    send_failures += 1;
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
                Err(e) => return Err(format!("ctc emit poll: {e}")),
            };
            if !resp.status().is_success() {
                return Err(format!(
                    "ctc emit poll {}: {}",
                    resp.status(),
                    resp.text().unwrap_or_default()
                ));
            }
            let job: JobResponse = resp
                .json()
                .map_err(|e| format!("ctc emit poll json: {e}"))?;
            revision = job.revision;
            match job.state.as_str() {
                "queued" | "running" => continue,
                "done" => {
                    let Some(result) = job.result else {
                        return Err("ctc emit job done without result".into());
                    };
                    return Ok(result.emissions.sha256);
                }
                "failed" => {
                    return Err(format!(
                        "ctc emit job failed: {}",
                        job.error.unwrap_or_else(|| "unknown".into())
                    ));
                }
                other => return Err(format!("ctc emit job unexpected state: {other}")),
            }
        }
    }

    fn delete_job(&self, job_id: &str) -> Result<(), String> {
        let client = self.http()?;
        let url = format!("{}/v1/emissions/jobs/{job_id}", self.base_url);
        let resp = self
            .auth(client.delete(&url))
            .send()
            .map_err(|e| format!("ctc emit DELETE: {e}"))?;
        let code = resp.status().as_u16();
        if code == 200 || code == 204 || code == 404 {
            Ok(())
        } else {
            Err(format!("ctc emit DELETE {code}"))
        }
    }

    fn download_file(&self, sha256: &str) -> Result<Vec<u8>, String> {
        let client = self.http()?;
        let url = format!("{}/v1/files/{sha256}", self.base_url);
        let resp = self
            .auth(client.get(&url))
            .send()
            .map_err(|e| format!("ctc emit download: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!(
                "ctc emit download {}: {}",
                resp.status(),
                resp.text().unwrap_or_default()
            ));
        }
        resp.bytes()
            .map(|b| b.to_vec())
            .map_err(|e| format!("ctc emit download body: {e}"))
    }
}

fn stem_cache_path(work_dir: &Path, audio: &Path) -> PathBuf {
    let stem = audio
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio");
    work_dir.join(format!("{stem}.stem"))
}

/// Prefer `{audio_stem}.stem`; else a sole non-empty `*.stem` in `work_dir` (prior emit).
fn cached_stem_for_audio(work_dir: &Path, audio: &Path) -> Option<PathBuf> {
    let preferred = stem_cache_path(work_dir, audio);
    if file_nonempty(&preferred) {
        return Some(preferred);
    }
    let Ok(entries) = fs::read_dir(work_dir) else {
        return None;
    };
    let mut stems = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("stem") {
            continue;
        }
        if file_nonempty(&path) {
            stems.push(path);
        }
    }
    if stems.len() == 1 {
        Some(stems.remove(0))
    } else {
        None
    }
}

fn file_nonempty(path: &Path) -> bool {
    fs::metadata(path)
        .map(|m| m.is_file() && m.len() > 0)
        .unwrap_or(false)
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("ctc emit hash open: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 64];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("ctc emit hash read: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_defaults_when_unset() {
        let prev_url = std::env::var_os("SHELF_CTC_EMIT_URL");
        let prev_key = std::env::var_os("SHELF_CTC_EMIT_API_KEY");
        std::env::remove_var("SHELF_CTC_EMIT_URL");
        std::env::remove_var("SHELF_CTC_EMIT_API_KEY");
        let client = CtcEmitClient::from_env();
        assert_eq!(client.base_url, "http://127.0.0.1:8765");
        assert_eq!(client.api_key, "shelf-internal");
        match prev_url {
            Some(v) => std::env::set_var("SHELF_CTC_EMIT_URL", v),
            None => std::env::remove_var("SHELF_CTC_EMIT_URL"),
        }
        match prev_key {
            Some(v) => std::env::set_var("SHELF_CTC_EMIT_API_KEY", v),
            None => std::env::remove_var("SHELF_CTC_EMIT_API_KEY"),
        }
    }

    #[test]
    fn cached_stem_prefers_audio_basename() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("narration.mp3");
        fs::write(&audio, b"x").unwrap();
        let preferred = dir.path().join("narration.stem");
        fs::write(&preferred, b"STEM").unwrap();
        fs::write(dir.path().join("other.stem"), b"STEM2").unwrap();
        assert_eq!(
            cached_stem_for_audio(dir.path(), &audio).as_deref(),
            Some(preferred.as_path())
        );
    }

    #[test]
    fn cached_stem_reuses_sole_emissions_sha_named_file() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("narration.mp3");
        fs::write(&audio, b"x").unwrap();
        let sole = dir
            .path()
            .join("528e9c496996c6c5eb1e37e3bd92e4c0f6bbacc5fcf7162983b3ced34342c0b0.stem");
        fs::write(&sole, b"STEM").unwrap();
        assert_eq!(
            cached_stem_for_audio(dir.path(), &audio).as_deref(),
            Some(sole.as_path())
        );
    }
}
