//! Probe audiobook tags/chapters/cover via ffprobe + ffmpeg (same tools as AAX convert).

use super::*;
use std::process::Command;

#[derive(Debug, Clone)]
pub(crate) struct ChapterMark {
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct AudioProbe {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub chapters: Vec<ChapterMark>,
}

fn ffmpeg_bin() -> PathBuf {
    std::env::var_os("SHELF_AAX_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

fn ffprobe_bin() -> PathBuf {
    if let Some(ffmpeg) = std::env::var_os("SHELF_AAX_FFMPEG") {
        let path = PathBuf::from(&ffmpeg);
        if let Some(parent) = path.parent() {
            let sibling = parent.join("ffprobe");
            if sibling.exists() {
                return sibling;
            }
        }
    }
    PathBuf::from("ffprobe")
}

fn clean_tag(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

/// Read title/artist/chapters from an on-disk audio file (headers only via ffprobe).
pub(crate) fn probe_file(path: &std::path::Path) -> Option<AudioProbe> {
    let output = Command::new(ffprobe_bin())
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_chapters",
            path.to_str()?,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let tags = json
        .pointer("/format/tags")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let title = tags
        .get("title")
        .and_then(|v| v.as_str())
        .and_then(clean_tag);
    let artist = tags
        .get("artist")
        .or_else(|| tags.get("album_artist"))
        .and_then(|v| v.as_str())
        .and_then(clean_tag);
    let mut chapters = Vec::new();
    if let Some(list) = json.get("chapters").and_then(|v| v.as_array()) {
        for (i, ch) in list.iter().enumerate() {
            let start = ch
                .get("start_time")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .or_else(|| ch.get("start").and_then(|v| v.as_f64()))
                .unwrap_or(0.0);
            let end = ch
                .get("end_time")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .or_else(|| ch.get("end").and_then(|v| v.as_f64()))
                .unwrap_or(start);
            let title = ch
                .pointer("/tags/title")
                .and_then(|v| v.as_str())
                .and_then(clean_tag)
                .unwrap_or_else(|| format!("Chapter {}", i + 1));
            chapters.push(ChapterMark {
                title,
                start_ms: (start * 1000.0).round() as i64,
                end_ms: (end * 1000.0).round() as i64,
            });
        }
    }
    Some(AudioProbe {
        title,
        artist,
        chapters,
    })
}

/// Extract embedded cover art to JPEG/PNG bytes when present.
pub(crate) fn extract_cover(path: &std::path::Path) -> Option<(Vec<u8>, &'static str)> {
    let dir = tempfile::tempdir().ok()?;
    let out = dir.path().join("cover.jpg");
    let status = Command::new(ffmpeg_bin())
        .args([
            "-y",
            "-i",
            path.to_str()?,
            "-an",
            "-vcodec",
            "copy",
            out.to_str()?,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    if !status.success() || !out.is_file() {
        return None;
    }
    let bytes = std::fs::read(&out).ok()?;
    if bytes.len() < 24 {
        return None;
    }
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else {
        "image/jpeg"
    };
    Some((bytes, mime))
}

pub(crate) fn chapters_to_json(chapters: &[ChapterMark]) -> String {
    let values: Vec<serde_json::Value> = chapters
        .iter()
        .enumerate()
        .map(|(i, ch)| {
            serde_json::json!({
                "id": format!("ch-{i}"),
                "title": ch.title,
                "startMs": ch.start_ms,
                "endMs": ch.end_ms,
            })
        })
        .collect();
    serde_json::to_string(&values).unwrap_or_else(|_| "[]".into())
}

pub(crate) fn chapters_from_json(raw: &str) -> Vec<ChapterMark> {
    let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(raw) else {
        return Vec::new();
    };
    list.into_iter()
        .filter_map(|v| {
            Some(ChapterMark {
                title: v.get("title")?.as_str()?.to_string(),
                start_ms: v.get("startMs")?.as_i64()?,
                end_ms: v.get("endMs").and_then(|x| x.as_i64()).unwrap_or(0),
            })
        })
        .collect()
}

/// Materialize book bytes to a path ffprobe/ffmpeg can read (temp or storage_path).
pub(crate) fn with_audio_path<T>(
    data_dir: &std::path::Path,
    inline: &[u8],
    storage_path: Option<&str>,
    format: &str,
    f: impl FnOnce(&std::path::Path) -> T,
) -> ApiResult<T> {
    if let Some(relative) = storage_path.filter(|p| !p.is_empty()) {
        if relative.contains("..") || std::path::Path::new(relative).is_absolute() {
            return Err(ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid on-disk book path.",
            ));
        }
        return Ok(f(&data_dir.join(relative)));
    }
    let dir = tempfile::tempdir().map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not create temp audio workspace.",
        )
    })?;
    let path = dir.path().join(format!("book.{format}"));
    std::fs::write(&path, inline).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not write temp audio for metadata.",
        )
    })?;
    Ok(f(&path))
}

/// Fill cover + chapter markers (and improve title/author) when missing.
pub(crate) fn enrich_audio_book(
    db: &Connection,
    data_dir: &std::path::Path,
    book_id: i64,
) -> ApiResult<()> {
    let (inline, storage_path, format, has_cover, markers, title, author): (
        Vec<u8>,
        Option<String>,
        String,
        bool,
        Option<String>,
        String,
        String,
    ) = db.query_row(
        "SELECT f.content, f.storage_path, f.format, f.cover IS NOT NULL,
                f.chapter_markers, b.title, b.author
         FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=?",
        [book_id],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        },
    )?;
    if !matches!(
        format.as_str(),
        "mp3" | "m4b" | "opus" | "ogg" | "flac" | "aax"
    ) {
        return Ok(());
    }
    let need_markers = markers.as_ref().map(|s| s.is_empty()).unwrap_or(true);
    let need_cover = !has_cover;
    // Filename stems often look like CamelCase dumps or use underscores.
    let need_title = title.contains('_')
        || !title.contains(' ')
        || title.to_ascii_lowercase().ends_with(&format!(".{format}"))
        || title
            .chars()
            .zip(title.chars().skip(1))
            .any(|(a, b)| a.is_ascii_lowercase() && b.is_ascii_uppercase());
    let need_author = author.trim().is_empty();
    if !need_markers && !need_cover && !need_title && !need_author {
        return Ok(());
    }

    let probe = with_audio_path(
        data_dir,
        &inline,
        storage_path.as_deref(),
        &format,
        |path| {
            let probe = probe_file(path);
            let cover = if need_cover {
                extract_cover(path)
            } else {
                None
            };
            (probe, cover)
        },
    )?;
    let (probe, cover) = probe;

    if let Some(probe) = probe {
        if need_markers {
            let json = if probe.chapters.is_empty() {
                "[]".to_string()
            } else {
                chapters_to_json(&probe.chapters)
            };
            db.execute(
                "UPDATE epub_files SET chapter_markers=? WHERE book_id=?",
                params![json, book_id],
            )?;
        }
        if need_title {
            if let Some(t) = probe.title {
                db.execute("UPDATE books SET title=? WHERE id=?", params![t, book_id])?;
            }
        }
        if need_author {
            if let Some(a) = probe.artist {
                db.execute("UPDATE books SET author=? WHERE id=?", params![a, book_id])?;
            }
        }
    } else if need_markers {
        db.execute(
            "UPDATE epub_files SET chapter_markers='[]' WHERE book_id=?",
            [book_id],
        )?;
    }
    if let Some((bytes, mime)) = cover {
        db.execute(
            "UPDATE epub_files SET cover=?, cover_mime=? WHERE book_id=?",
            params![bytes, mime, book_id],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapters_round_trip_json() {
        let marks = vec![
            ChapterMark {
                title: "One".into(),
                start_ms: 0,
                end_ms: 1000,
            },
            ChapterMark {
                title: "Two".into(),
                start_ms: 1000,
                end_ms: 2000,
            },
        ];
        let raw = chapters_to_json(&marks);
        let back = chapters_from_json(&raw);
        assert_eq!(back.len(), 2);
        assert_eq!(back[1].title, "Two");
        assert_eq!(back[1].start_ms, 1000);
    }
}
