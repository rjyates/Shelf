//! Background align worker (Storyteller stage order), AAX-style spawn.

use super::ctc_emit::CtcEmitClient;
use super::job::{
    align_job_tick, fail_work, finish_work, get_work, start_align_job, AlignJobStatus, AlignStage,
};
use super::pipeline::{run_align, AlignPipeline, AlignProgress, AlignRequest};
use super::transcribe::{MissingTranscriber, ScriptedTranscriber, Transcriber};
use crate::{
    configure_connection, load_book_bytes, store_book_bytes, ApiError, ApiResult, AppState,
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read};
use std::path::{Path as FsPath, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateWorkBody {
    pub epub_book_id: i64,
    pub audio_book_id: i64,
    /// When true (default), queue alignment immediately.
    #[serde(default = "default_true")]
    pub start: bool,
    /// Optional shared catalogue title for both editions (manual merge).
    #[serde(default)]
    pub title: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct WorkJson {
    pub id: i64,
    pub epub_book_id: i64,
    pub audio_book_id: i64,
    pub readaloud_book_id: Option<i64>,
    pub status: String,
    pub stage: Option<String>,
    pub progress: f32,
    pub error: Option<String>,
}

fn work_json(w: &super::job::BookWork) -> WorkJson {
    WorkJson {
        id: w.id,
        epub_book_id: w.epub_book_id,
        audio_book_id: w.audio_book_id,
        readaloud_book_id: w.readaloud_book_id,
        status: w.status.as_str().to_string(),
        stage: w.stage.map(|s| s.as_str().to_string()),
        progress: w.progress,
        error: w.error.clone(),
    }
}

pub async fn list_works_http(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<WorkJson>>> {
    state
        .db(move |db| {
            let user = crate::authenticate(db, &headers)?;
            let works = owned_works(db, user)?;
            Ok(Json(works.iter().map(work_json).collect()))
        })
        .await
}

pub async fn get_work_http(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Json<WorkJson>> {
    state
        .db(move |db| {
            let user = crate::authenticate(db, &headers)?;
            let w = owned_work(db, user, id)?
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Work not found."))?;
            Ok(Json(work_json(&w)))
        })
        .await
}

pub async fn create_work_http(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateWorkBody>,
) -> ApiResult<(StatusCode, Json<WorkJson>)> {
    let spawn_state = state.clone();
    let (id, should_start, json) = state
        .db(move |db| {
            let user = crate::authenticate(db, &headers)?;
            validate_pair(db, user, body.epub_book_id, body.audio_book_id)?;
            if let Some(title) = body
                .title
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                if title.len() > 500 {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "Merged title must be 500 characters or fewer.",
                    ));
                }
                apply_merge_title(db, user, body.epub_book_id, body.audio_book_id, title)?;
            }
            let existing: Option<i64> = db
                .query_row(
                    "SELECT id FROM book_works WHERE epub_book_id=?1 AND audio_book_id=?2",
                    params![body.epub_book_id, body.audio_book_id],
                    |r| r.get(0),
                )
                .optional()?;
            let id = match existing {
                Some(id) => id,
                None => super::job::create_work(db, body.epub_book_id, body.audio_book_id)?,
            };
            let mut should_start = body.start;
            if should_start {
                let w = get_work(db, id)?.ok_or(ApiError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Work missing after create.",
                ))?;
                // Do not reset or double-spawn an in-flight job (Combine re-click).
                if matches!(
                    w.status,
                    AlignJobStatus::Processing | AlignJobStatus::Queued
                ) {
                    should_start = false;
                } else {
                    start_align_job(db, id)?;
                }
            }
            let w = get_work(db, id)?.ok_or(ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Work missing after create.",
            ))?;
            Ok((id, should_start, Json(work_json(&w))))
        })
        .await?;
    if should_start {
        spawn_align(spawn_state, id);
    }
    Ok((StatusCode::CREATED, json))
}

pub async fn start_work_http(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Json<WorkJson>> {
    let spawn_state = state.clone();
    let (json, should_spawn) = state
        .db(move |db| {
            let user = crate::authenticate(db, &headers)?;
            let w = owned_work(db, user, id)?
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Work not found."))?;
            if matches!(
                w.status,
                AlignJobStatus::Processing | AlignJobStatus::Queued
            ) {
                return Ok((Json(work_json(&w)), false));
            }
            start_align_job(db, id)?;
            let w = get_work(db, id)?.unwrap();
            Ok((Json(work_json(&w)), true))
        })
        .await?;
    if should_spawn {
        spawn_align(spawn_state, id);
    }
    Ok(json)
}

fn owned_works(db: &Connection, user: i64) -> ApiResult<Vec<super::job::BookWork>> {
    let mut stmt = db.prepare(
        "SELECT w.id, w.epub_book_id, w.audio_book_id, w.readaloud_book_id, w.status, w.stage, w.progress, w.error
         FROM book_works w
         JOIN books e ON e.id = w.epub_book_id
         JOIN books a ON a.id = w.audio_book_id
         WHERE e.user_id = ?1 AND a.user_id = ?1
         ORDER BY w.id DESC",
    )?;
    let rows = stmt.query_map(params![user], |row| {
        Ok(super::job::BookWork {
            id: row.get(0)?,
            epub_book_id: row.get(1)?,
            audio_book_id: row.get(2)?,
            readaloud_book_id: row.get(3)?,
            status: AlignJobStatus::parse(&row.get::<_, String>(4)?)
                .unwrap_or(AlignJobStatus::Idle),
            stage: row
                .get::<_, Option<String>>(5)?
                .and_then(|s| AlignStage::parse(&s)),
            progress: row.get::<_, f64>(6)? as f32,
            error: row.get(7)?,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

fn owned_work(db: &Connection, user: i64, id: i64) -> ApiResult<Option<super::job::BookWork>> {
    Ok(owned_works(db, user)?.into_iter().find(|w| w.id == id))
}

fn validate_pair(db: &Connection, user: i64, epub_id: i64, audio_id: i64) -> ApiResult<()> {
    let epub_fmt: String = db
        .query_row(
            "SELECT f.format FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=? AND b.user_id=?",
            params![epub_id, user],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(ApiError(StatusCode::BAD_REQUEST, "EPUB book not found."))?;
    if epub_fmt != "epub" {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Combine needs an EPUB edition.",
        ));
    }
    let audio_fmt: String = db
        .query_row(
            "SELECT f.format FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=? AND b.user_id=?",
            params![audio_id, user],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(ApiError(
            StatusCode::BAD_REQUEST,
            "Audiobook edition not found.",
        ))?;
    if !crate::audiobook::is_playable_audio_format(&audio_fmt) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Combine needs a playable audiobook (not AAX-pending).",
        ));
    }
    Ok(())
}

fn apply_merge_title(
    db: &Connection,
    user: i64,
    epub_id: i64,
    audio_id: i64,
    title: &str,
) -> ApiResult<()> {
    let n = db.execute(
        "UPDATE books SET title=?1 WHERE user_id=?2 AND id IN (?3, ?4)",
        params![title, user, epub_id, audio_id],
    )?;
    if n != 2 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Could not rename both editions for merge.",
        ));
    }
    Ok(())
}

pub(crate) fn spawn_align(state: AppState, work_id: i64) {
    tokio::spawn(async move {
        let database = state.database.clone();
        let data_dir = state.data_dir.clone();
        let _ = tokio::task::spawn_blocking(move || run_one(&database, &data_dir, work_id)).await;
    });
}

/// Re-queue works left queued/processing after a restart.
pub(crate) fn resume_pending(state: &AppState) {
    let database = state.database.clone();
    let Ok(db) = Connection::open(&database) else {
        return;
    };
    let Ok(()) = configure_connection(&db) else {
        return;
    };
    let Ok(mut stmt) =
        db.prepare("SELECT id FROM book_works WHERE status IN ('queued','processing')")
    else {
        return;
    };
    let Ok(ids) = stmt
        .query_map([], |r| r.get::<_, i64>(0))
        .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
    else {
        return;
    };
    for id in ids {
        spawn_align(state.clone(), id);
    }
}

fn run_one(database: &FsPath, data_dir: &FsPath, work_id: i64) {
    let Ok(db) = Connection::open(database) else {
        return;
    };
    let _ = configure_connection(&db);
    if let Err(err) = run_one_inner(&db, data_dir, work_id) {
        let msg = err.to_string();
        let _ = fail_work(&db, work_id, &msg);
    }
}

/// Clear align work dir but keep `*.stem` so CTC emit can resume after SYNC failure.
fn reset_work_dir_preserving_stems(work_dir: &FsPath) -> std::io::Result<()> {
    if work_dir.is_dir() {
        for entry in std::fs::read_dir(work_dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) == Some("stem") {
                continue;
            }
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
    } else {
        std::fs::create_dir_all(work_dir)?;
    }
    Ok(())
}

fn run_one_inner(db: &Connection, data_dir: &FsPath, work_id: i64) -> Result<(), String> {
    let work = get_work(db, work_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "work missing".to_string())?;
    align_job_tick(
        db,
        work_id,
        AlignJobStatus::Processing,
        AlignStage::SplitTracks,
        0.05,
        None,
    )
    .map_err(|e| e.to_string())?;

    let work_dir = data_dir.join(format!("align-work/{work_id}"));
    // Keep *.stem so a failed SYNC can reuse CTC emit (Storyteller emit cache).
    reset_work_dir_preserving_stems(&work_dir).map_err(|e| e.to_string())?;

    let (epub_bytes, epub_title, user_id) = load_edition(db, data_dir, work.epub_book_id)?;
    let (audio_bytes, audio_fmt, _) = load_edition_audio(db, data_dir, work.audio_book_id)?;

    let epub_path = work_dir.join("source.epub");
    std::fs::write(&epub_path, &epub_bytes).map_err(|e| e.to_string())?;
    // ST upgradeEpub before align (EPUB3 no-op; removeNcx=false).
    super::upgrade_epub::upgrade_epub_in_place(&epub_path, false)?;
    let audio_paths = materialize_audio(&work_dir, &audio_bytes, &audio_fmt)?;
    // Multi-chapter: pipeline walks the full spine (Storyteller alignBook).

    align_job_tick(
        db,
        work_id,
        AlignJobStatus::Processing,
        AlignStage::TranscribeChapters,
        0.1,
        None,
    )
    .map_err(|e| e.to_string())?;

    let out = work_dir.join("readaloud.epub");
    let db_path = db.path().map(PathBuf::from);
    let progress_db = db_path.clone();
    let on_progress: Option<super::pipeline::ProgressFn> =
        Some(Box::new(move |p: AlignProgress| {
            let Some(ref path) = progress_db else {
                return;
            };
            let Ok(conn) = Connection::open(path) else {
                return;
            };
            let _ = configure_connection(&conn);
            let stage = AlignStage::parse(p.stage).unwrap_or(AlignStage::SyncChapters);
            let _ = align_job_tick(
                &conn,
                work_id,
                AlignJobStatus::Processing,
                stage,
                p.fraction,
                None,
            );
        }));

    let req = AlignRequest {
        epub_path,
        audio_paths,
        output_path: out.clone(),
        chapter_href: None,
    };
    // Production: CTC via in-image ghost-story. Script path is cargo/API tests only.
    if let Some(scripted) = scripted_transcriber(data_dir) {
        let pipeline = AlignPipeline::new(scripted.as_ref());
        run_align(&pipeline, &req, on_progress)?;
    } else {
        let ctc_client = CtcEmitClient::from_env();
        let missing = MissingTranscriber;
        let mut pipeline = AlignPipeline::new(&missing);
        pipeline.ctc_emit = Some(&ctc_client);
        pipeline.work_dir = Some(work_dir.clone());
        run_align(&pipeline, &req, on_progress)?;
    }

    let readaloud = std::fs::read(&out).map_err(|e| e.to_string())?;
    let title = format!("{epub_title} (readaloud)");
    let readaloud_id = import_readaloud(db, data_dir, user_id, &title, &readaloud)?;
    finish_work(db, work_id, readaloud_id).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&work_dir);
    Ok(())
}

/// Cargo/API test stand-in only (`align-script.txt` / `SHELF_ALIGN_SCRIPT_FILE`).
fn scripted_transcriber(data_dir: &FsPath) -> Option<Box<dyn Transcriber>> {
    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("SHELF_ALIGN_SCRIPT_FILE") {
        candidates.push(PathBuf::from(path));
    }
    candidates.push(data_dir.join("align-script.txt"));
    for path in candidates {
        if let Ok(script) = std::fs::read_to_string(&path) {
            if script.trim().is_empty() {
                continue;
            }
            let duration = std::env::var("SHELF_ALIGN_SCRIPT_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(60.0);
            return Some(Box::new(ScriptedTranscriber {
                script,
                duration_secs: duration,
            }));
        }
    }
    None
}

fn load_edition(
    db: &Connection,
    data_dir: &FsPath,
    book_id: i64,
) -> Result<(Vec<u8>, String, i64), String> {
    let row: (Vec<u8>, Option<String>, String, i64) = db
        .query_row(
            "SELECT f.content, f.storage_path, b.title, b.user_id
             FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=?",
            params![book_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|e| e.to_string())?;
    let bytes = load_book_bytes(data_dir, row.0, row.1).map_err(|e| e.1.to_string())?;
    Ok((bytes, row.2, row.3))
}

fn load_edition_audio(
    db: &Connection,
    data_dir: &FsPath,
    book_id: i64,
) -> Result<(Vec<u8>, String, i64), String> {
    let row: (Vec<u8>, Option<String>, String, i64) = db
        .query_row(
            "SELECT f.content, f.storage_path, f.format, b.user_id
             FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=?",
            params![book_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|e| e.to_string())?;
    let bytes = load_book_bytes(data_dir, row.0, row.1).map_err(|e| e.1.to_string())?;
    Ok((bytes, row.2, row.3))
}

fn materialize_audio(
    work_dir: &FsPath,
    bytes: &[u8],
    format: &str,
) -> Result<Vec<PathBuf>, String> {
    if format == "audiozip" {
        let tracks = crate::audiobook::audio_zip_track_names(bytes).map_err(|e| e.1.to_string())?;
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        let mut paths = Vec::new();
        for (name, _fmt, _) in tracks {
            let mut file = archive.by_name(&name).map_err(|e| e.to_string())?;
            let leaf = FsPath::new(&name)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("track.mp3");
            let dest = work_dir.join(leaf);
            let mut out = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut out).map_err(|e| e.to_string())?;
            paths.push(dest);
        }
        return Ok(paths);
    }
    let dest = work_dir.join(format!("narration.{format}"));
    std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
    Ok(vec![dest])
}

fn import_readaloud(
    db: &Connection,
    data_dir: &FsPath,
    user_id: i64,
    title: &str,
    bytes: &[u8],
) -> Result<i64, String> {
    let digest = format!("{:x}", Sha256::digest(bytes));
    // Dedup by digest for this user.
    if let Ok(existing) = db.query_row(
        "SELECT b.id FROM books b JOIN epub_files f ON f.book_id=b.id WHERE b.user_id=? AND f.digest=?",
        params![user_id, digest],
        |r| r.get::<_, i64>(0),
    ) {
        return Ok(existing);
    }
    db.execute(
        "INSERT INTO books(user_id,title,author,total_pages) VALUES (?,?,?,1)",
        params![user_id, title, ""],
    )
    .map_err(|e| e.to_string())?;
    let id = db.last_insert_rowid();
    let (content, storage_path) =
        store_book_bytes(data_dir, &digest, "epub", bytes).map_err(|e| e.1.to_string())?;
    db.execute(
        "INSERT INTO epub_files(book_id,digest,content,format,storage_path) VALUES (?,?,?,'epub',?)",
        params![id, digest, content, storage_path],
    )
    .map_err(|e| e.to_string())?;
    let _ = content;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::spine::list_spine_chapters;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn tiny_epub(path: &FsPath) {
        let file = std::fs::File::create(path).unwrap();
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
    <dc:identifier id="uid">spine-test</dc:identifier>
    <dc:title>T</dc:title><dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="nav"/>
    <itemref idref="ch1"/>
  </spine>
</package>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/nav.xhtml", opts).unwrap();
        zip.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><nav/></body></html>"#)
            .unwrap();
        zip.start_file("OEBPS/chapter1.xhtml", opts).unwrap();
        zip.write_all(
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Hi there friend forever.</p></body></html>"#,
        )
        .unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn lists_nav_then_chapter_spine_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.epub");
        tiny_epub(&path);
        let chapters = list_spine_chapters(&path).unwrap();
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].href, "OEBPS/nav.xhtml");
        assert!(chapters[0]
            .properties
            .split_whitespace()
            .any(|p| p == "nav"));
        assert_eq!(chapters[1].href, "OEBPS/chapter1.xhtml");
    }

    #[test]
    fn reset_work_dir_keeps_stem_drops_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("align-work/1");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(work.join("narration.mp3"), b"audio").unwrap();
        std::fs::write(work.join("source.epub"), b"epub").unwrap();
        let stem = work.join("narration.stem");
        std::fs::write(&stem, b"STEM").unwrap();
        reset_work_dir_preserving_stems(&work).unwrap();
        assert!(stem.is_file());
        assert!(!work.join("narration.mp3").exists());
        assert!(!work.join("source.epub").exists());
    }
}
