//! Standalone audiobook APIs matching Book Orbit's contract:
//! `GET …/manifest`, `GET …/assets/{assetId}/content`, `GET|PUT|DELETE …/playback-state`.

use super::*;
use axum::extract::Path;
use std::io::{Cursor, Read};

const MANIFEST_SCHEMA: &str = "bookorbit.audiobook-manifest";
const MANIFEST_VERSION: u32 = 2;

pub(crate) fn is_audio_format(format: &str) -> bool {
    matches!(
        format,
        "mp3" | "m4b" | "opus" | "ogg" | "flac" | "audiozip" | "aax"
    )
}

pub(crate) fn is_playable_audio_format(format: &str) -> bool {
    matches!(format, "mp3" | "m4b" | "opus" | "ogg" | "flac" | "audiozip")
}

fn audio_ext(path: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    match ext.as_str() {
        "mp3" => Some("mp3"),
        "m4b" | "m4a" => Some("m4b"),
        "opus" => Some("opus"),
        "ogg" | "oga" => Some("ogg"),
        "flac" => Some("flac"),
        _ => None,
    }
}

fn track_mime(format: &str) -> &'static str {
    match format {
        "mp3" => "audio/mpeg",
        "m4b" => "audio/mp4",
        "opus" => "audio/ogg",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        _ => "application/octet-stream",
    }
}

/// Audio members inside a ZIP, lexicographic order (zero-padded track names sort correctly).
pub(crate) fn audio_zip_track_names(bytes: &[u8]) -> ApiResult<Vec<(String, String, u64)>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Not a supported audiobook ZIP."))?;
    if archive.len() > 10_000 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "This archive has too many entries.",
        ));
    }
    let mut tracks = Vec::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Could not read the audiobook ZIP."))?;
        if file.is_dir() || file.is_symlink() || file.enclosed_name().is_none() {
            continue;
        }
        let name = file.name().replace('\\', "/");
        let lower = name.to_ascii_lowercase();
        if lower.contains("__macosx/") || lower.split('/').any(|p| p.starts_with('.')) {
            continue;
        }
        let Some(format) = audio_ext(&name) else {
            continue;
        };
        tracks.push((name, format.to_string(), file.size()));
    }
    tracks.sort_by(|a, b| a.0.cmp(&b.0));
    if tracks.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "This ZIP has no MP3, M4B, Opus, Ogg, or FLAC tracks.",
        ));
    }
    Ok(tracks)
}

struct OwnedAudio {
    bytes: Vec<u8>,
    format: String,
    digest: String,
    title: String,
    author: String,
    has_cover: bool,
    chapter_markers: Option<String>,
}

fn owned_audio_meta(
    db: &Connection,
    data_dir: &std::path::Path,
    user: i64,
    id: i64,
) -> ApiResult<OwnedAudio> {
    // Confirm ownership first.
    let owned: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM books WHERE id=? AND user_id=?)",
        params![id, user],
        |r| r.get(0),
    )?;
    if !owned {
        return Err(ApiError(StatusCode::NOT_FOUND, "Book file not found."));
    }
    // Lazy cover/chapter/title backfill for files imported before metadata probing.
    let _ = audio_meta::enrich_audio_book(db, data_dir, id);

    let row = db
        .query_row(
            "SELECT f.content,f.storage_path,f.format,f.digest,b.title,b.author,f.cover IS NOT NULL,
                    f.chapter_markers
             FROM epub_files f JOIN books b ON b.id=f.book_id
             WHERE b.id=? AND b.user_id=?",
            params![id, user],
            |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, bool>(6)?,
                    r.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .optional()?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "Book file not found."))?;
    let (inline, storage_path, format, digest, title, author, has_cover, chapter_markers) = row;
    if format == "aax" {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "This audiobook is still converting from AAX.",
        ));
    }
    if !is_playable_audio_format(&format) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "This book is not an audiobook.",
        ));
    }
    let bytes = load_book_bytes(data_dir, inline, storage_path)?;
    Ok(OwnedAudio {
        bytes,
        format,
        digest,
        title,
        author,
        has_cover,
        chapter_markers,
    })
}

fn build_manifest(book_id: i64, meta: &OwnedAudio) -> ApiResult<serde_json::Value> {
    let OwnedAudio {
        bytes,
        format,
        digest,
        title,
        author,
        has_cover,
        chapter_markers,
    } = meta;
    let tracks: Vec<(String, String, u64)> = if format == "audiozip" {
        audio_zip_track_names(bytes)?
    } else {
        vec![(
            format!("{title}.{format}"),
            format.to_string(),
            bytes.len() as u64,
        )]
    };
    let mut assets = Vec::new();
    for (sequence, (_name, track_format, size)) in tracks.iter().enumerate() {
        assets.push(serde_json::json!({
            "assetId": sequence.to_string(),
            "fileId": book_id,
            "sequence": sequence + 1,
            "format": track_format,
            "durationMs": null,
            "sizeBytes": size,
            "etag": format!("{digest}:{sequence}"),
        }));
    }
    let marks = chapter_markers
        .as_deref()
        .map(audio_meta::chapters_from_json)
        .unwrap_or_default();
    let mut chapters = Vec::new();
    if format != "audiozip" && !marks.is_empty() {
        // Embedded M4B/MP4 chapter atoms — one asset, many seek points.
        for (sequence, mark) in marks.into_iter().enumerate() {
            chapters.push(serde_json::json!({
                "id": format!("ch-{sequence}"),
                "title": mark.title,
                "assetId": "0",
                "sequence": sequence + 1,
                "startMs": mark.start_ms,
                "endMs": mark.end_ms,
                "assetOffsetMs": mark.start_ms,
            }));
        }
    } else {
        for (sequence, (name, _track_format, _size)) in tracks.into_iter().enumerate() {
            let asset_id = sequence.to_string();
            let chapter_title = if format.as_str() == "audiozip" {
                std::path::Path::new(&name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&name)
                    .replace('_', " ")
            } else {
                title.clone()
            };
            chapters.push(serde_json::json!({
                "id": format!("ch-{sequence}"),
                "title": chapter_title,
                "assetId": asset_id,
                "sequence": sequence + 1,
                "startMs": 0,
                "endMs": 0,
                "assetOffsetMs": 0,
            }));
        }
    }
    let total_duration_ms = chapters
        .iter()
        .filter_map(|c| c.get("endMs").and_then(|v| v.as_i64()))
        .max()
        .unwrap_or(0);
    Ok(serde_json::json!({
        "schema": MANIFEST_SCHEMA,
        "schemaVersion": MANIFEST_VERSION,
        "revision": digest,
        "book": {
            "id": book_id,
            "title": title,
            "authors": if author.is_empty() { Vec::<String>::new() } else { vec![author.clone()] },
            "narrators": [],
            "hasCover": has_cover,
        },
        "assets": assets,
        "chapters": chapters,
        "totalDurationMs": total_duration_ms,
    }))
}

/// `GET /api/v1/audiobooks/{id}/manifest`
pub(super) async fn manifest(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let data_dir = state.data_dir.clone();
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let meta = owned_audio_meta(db, &data_dir, user, id)?;
            Ok(Json(build_manifest(id, &meta)?))
        })
        .await
}

fn read_zip_member(bytes: &[u8], name: &str) -> ApiResult<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not open the audiobook ZIP.",
        )
    })?;
    let mut file = archive
        .by_name(name)
        .map_err(|_| ApiError(StatusCode::NOT_FOUND, "Audiobook asset not found."))?;
    let mut out = Vec::new();
    file.read_to_end(&mut out).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read the audiobook track.",
        )
    })?;
    Ok(out)
}

/// `GET /api/v1/audiobooks/{id}/assets/{asset_id}/content`
pub(super) async fn asset_content(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, asset_id)): Path<(i64, String)>,
) -> ApiResult<Response> {
    let data_dir = state.data_dir.clone();
    let OwnedAudio { bytes, format, .. } = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                owned_audio_meta(db, &data_dir, user, id)
            }
        })
        .await?;
    let sequence: usize = asset_id
        .parse()
        .map_err(|_| ApiError(StatusCode::NOT_FOUND, "Audiobook asset not found."))?;
    let (track_bytes, track_format) = if format == "audiozip" {
        let tracks = audio_zip_track_names(&bytes)?;
        let (name, fmt, _) = tracks.get(sequence).cloned().ok_or(ApiError(
            StatusCode::NOT_FOUND,
            "Audiobook asset not found.",
        ))?;
        (read_zip_member(&bytes, &name)?, fmt)
    } else if sequence == 0 {
        (bytes, format)
    } else {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "Audiobook asset not found.",
        ));
    };
    let mime = track_mime(&track_format);
    let disposition = format!("inline; filename=track-{sequence}.{track_format}");
    // Small tracks return whole body; large spilled books already streamed via /file.
    // Asset payloads are per-track (usually small); support Range by slicing in memory.
    let len = track_bytes.len() as u64;
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| parse_bytes_range(s, len));
    if let Some((start, end)) = range {
        let body = track_bytes[start as usize..=end as usize].to_vec();
        let mut response = body.into_response();
        *response.status_mut() = StatusCode::PARTIAL_CONTENT;
        let headers = response.headers_mut();
        headers.insert(header::CONTENT_TYPE, header::HeaderValue::from_static(mime));
        headers.insert(
            header::CONTENT_DISPOSITION,
            header::HeaderValue::from_str(&disposition)
                .unwrap_or_else(|_| header::HeaderValue::from_static("inline")),
        );
        headers.insert(
            header::ACCEPT_RANGES,
            header::HeaderValue::from_static("bytes"),
        );
        headers.insert(
            header::CONTENT_LENGTH,
            header::HeaderValue::from(end - start + 1),
        );
        headers.insert(
            header::CONTENT_RANGE,
            header::HeaderValue::from_str(&format!("bytes {start}-{end}/{len}")).map_err(|_| {
                ApiError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not build Content-Range.",
                )
            })?,
        );
        return Ok(response);
    }
    let mut response = (
        [
            (header::CONTENT_TYPE, mime),
            (header::CONTENT_DISPOSITION, disposition.as_str()),
        ],
        track_bytes,
    )
        .into_response();
    response.headers_mut().insert(
        header::ACCEPT_RANGES,
        header::HeaderValue::from_static("bytes"),
    );
    Ok(response)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PutPlayback {
    asset_id: String,
    position_ms: u64,
    captured_at: String,
    #[allow(dead_code)]
    operation_id: String,
    base_revision: i64,
    manifest_revision: String,
}

/// `GET /api/v1/audiobooks/{id}/playback-state`
pub(super) async fn get_playback_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Response> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            // Confirm ownership + audio format.
            let format: String = db
                .query_row(
                    "SELECT f.format FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=? AND b.user_id=?",
                    params![id, user],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Book file not found."))?;
            if !is_audio_format(&format) {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "This book is not an audiobook.",
                ));
            }
            let row = db
                .query_row(
                    "SELECT asset_id,position_ms,percentage,completed,revision,captured_at,manifest_revision
                     FROM audiobook_playback WHERE book_id=?",
                    [id],
                    |r| {
                        Ok(serde_json::json!({
                            "assetId": r.get::<_, String>(0)?,
                            "positionMs": r.get::<_, i64>(1)?,
                            "percentage": r.get::<_, f64>(2)?,
                            "completed": r.get::<_, i64>(3)? != 0,
                            "capturedAt": r.get::<_, String>(5)?,
                            "revision": r.get::<_, i64>(4)?,
                            "manifestRevision": r.get::<_, String>(6)?,
                        }))
                    },
                )
                .optional()?;
            match row {
                Some(body) => Ok(Json(body).into_response()),
                None => Ok(StatusCode::NOT_FOUND.into_response()),
            }
        })
        .await
}

/// `PUT /api/v1/audiobooks/{id}/playback-state`
pub(super) async fn put_playback_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<PutPlayback>,
) -> ApiResult<Json<serde_json::Value>> {
    if body.captured_at.is_empty()
        || body.captured_at.len() > 64
        || body.asset_id.is_empty()
        || body.asset_id.len() > 64
        || body.manifest_revision.is_empty()
        || body.manifest_revision.len() > 128
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid audiobook playback state.",
        ));
    }
    let data_dir = state.data_dir.clone();
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let meta = owned_audio_meta(db, &data_dir, user, id)?;
            let manifest = build_manifest(id, &meta)?;
            if manifest["revision"] != body.manifest_revision {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "Audiobook manifest revision is stale.",
                ));
            }
            let assets = manifest["assets"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if !assets.iter().any(|a| a["assetId"] == body.asset_id) {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Unknown audiobook asset.",
                ));
            }
            let existing: Option<(i64,)> = db
                .query_row(
                    "SELECT revision FROM audiobook_playback WHERE book_id=?",
                    [id],
                    |r| Ok((r.get(0)?,)),
                )
                .optional()?;
            let next_revision = match existing {
                Some((rev,)) => {
                    if body.base_revision != rev {
                        return Err(ApiError(
                            StatusCode::CONFLICT,
                            "Audiobook playback revision is stale.",
                        ));
                    }
                    rev + 1
                }
                None => {
                    if body.base_revision != 0 {
                        return Err(ApiError(
                            StatusCode::CONFLICT,
                            "Audiobook playback revision is stale.",
                        ));
                    }
                    1
                }
            };
            let asset_index: usize = body.asset_id.parse().unwrap_or(0);
            let track_count = assets.len().max(1);
            let percentage = ((asset_index as f64) / track_count as f64).clamp(0.0, 1.0);
            let completed = percentage >= 1.0;
            db.execute(
                "INSERT INTO audiobook_playback
                   (book_id,asset_id,position_ms,percentage,completed,revision,captured_at,manifest_revision)
                 VALUES (?,?,?,?,?,?,?,?)
                 ON CONFLICT(book_id) DO UPDATE SET
                   asset_id=excluded.asset_id,
                   position_ms=excluded.position_ms,
                   percentage=excluded.percentage,
                   completed=excluded.completed,
                   revision=excluded.revision,
                   captured_at=excluded.captured_at,
                   manifest_revision=excluded.manifest_revision",
                params![
                    id,
                    body.asset_id,
                    body.position_ms as i64,
                    percentage,
                    completed as i64,
                    next_revision,
                    body.captured_at,
                    body.manifest_revision,
                ],
            )?;
            // Mirror into epub_files so the catalogue % stays useful.
            let _ = db.execute(
                "UPDATE epub_files SET percentage=?,position=? WHERE book_id=?",
                params![
                    percentage,
                    format!("audioms({})", body.position_ms),
                    id
                ],
            );
            Ok(Json(serde_json::json!({
                "assetId": body.asset_id,
                "positionMs": body.position_ms,
                "percentage": percentage,
                "completed": completed,
                "capturedAt": body.captured_at,
                "revision": next_revision,
                "manifestRevision": body.manifest_revision,
            })))
        })
        .await
}

/// `DELETE /api/v1/audiobooks/{id}/playback-state`
pub(super) async fn delete_playback_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let owned: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=? AND b.user_id=?)",
                params![id, user],
                |r| r.get(0),
            )?;
            if !owned {
                return Err(ApiError(StatusCode::NOT_FOUND, "Book file not found."));
            }
            db.execute("DELETE FROM audiobook_playback WHERE book_id=?", [id])?;
            let _ = db.execute(
                "UPDATE epub_files SET percentage=0,position=NULL WHERE book_id=?",
                [id],
            );
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_zip_lists_sorted_tracks() {
        use std::io::{Cursor, Write};
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in [
            ("02 Chapter.mp3", b"ID3two" as &[u8]),
            ("01 Intro.mp3", b"ID3one"),
            ("cover.jpg", b"not-audio"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(content).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let tracks = audio_zip_track_names(&bytes).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].0, "01 Intro.mp3");
        assert_eq!(tracks[1].0, "02 Chapter.mp3");
    }
}
