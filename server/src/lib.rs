mod aax;
mod achievements;
mod align;
mod audio_meta;
mod audiobook;
mod epub;
mod epub_package;
mod hardcover;
mod insights;
mod kindle_delivery;
mod kobo_sync;
mod koreader_sync;
mod library_health;
mod library_org;
mod media_overlay;
mod metadata;
mod opds;
mod reader_data;
mod reading_stats;
mod readwise;
mod session_injectors;
mod storygraph;
mod tts;
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use rand::{rngs::OsRng, RngCore};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone)]
struct AppState {
    database: PathBuf,
    /// Parent of `shelf.db`; large book blobs spill under `{data_dir}/files/`.
    data_dir: PathBuf,
    login_attempts: Arc<Mutex<VecDeque<Instant>>>,
}

#[derive(Debug)]
struct ApiError(StatusCode, &'static str);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error": self.1}))).into_response()
    }
}
impl From<rusqlite::Error> for ApiError {
    fn from(err: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(code, _) = &err {
            if code.extended_code == rusqlite::ffi::SQLITE_TOOBIG {
                return Self(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "This book is too large for the library database.",
                );
            }
        }
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database operation failed.",
        )
    }
}
type ApiResult<T> = Result<T, ApiError>;

impl AppState {
    fn limit_login(&self) -> ApiResult<()> {
        let mut attempts = self
            .login_attempts
            .lock()
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Sign in failed."))?;
        let now = Instant::now();
        while attempts
            .front()
            .is_some_and(|time| now.duration_since(*time) >= Duration::from_secs(60))
        {
            attempts.pop_front();
        }
        if attempts.len() >= 20 {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many sign-in attempts. Try again in a minute.",
            ));
        }
        attempts.push_back(now);
        Ok(())
    }
    async fn db<T: Send + 'static>(
        &self,
        action: impl FnOnce(&mut Connection) -> ApiResult<T> + Send + 'static,
    ) -> ApiResult<T> {
        let path = self.database.clone();
        tokio::task::spawn_blocking(move || {
            let mut db = Connection::open(path)?;
            configure_connection(&db)?;
            action(&mut db)
        })
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Request failed."))?
    }
}

fn configure_connection(db: &Connection) -> rusqlite::Result<()> {
    // Large read-along imports can hold the write lock while hashing/spilling to disk.
    db.busy_timeout(std::time::Duration::from_secs(60))?;
    db.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

/// Bundled SQLite caps any BLOB at 1e9 bytes; spill before that so 1.5 GiB read-alongs import.
const INLINE_BLOB_MAX: usize = 900_000_000;

/// `content`, `storage_path`, `source`, `source_path`, `format` from `epub_files`.
type BookFileRow = (
    Vec<u8>,
    Option<String>,
    Option<Vec<u8>>,
    Option<String>,
    String,
);

fn store_book_bytes(
    data_dir: &std::path::Path,
    digest: &str,
    kind: &str,
    bytes: &[u8],
) -> ApiResult<(Vec<u8>, Option<String>)> {
    store_book_bytes_limited(data_dir, digest, kind, bytes, INLINE_BLOB_MAX)
}

fn store_book_bytes_limited(
    data_dir: &std::path::Path,
    digest: &str,
    kind: &str,
    bytes: &[u8],
    inline_max: usize,
) -> ApiResult<(Vec<u8>, Option<String>)> {
    if bytes.len() <= inline_max {
        return Ok((bytes.to_vec(), None));
    }
    let dir = data_dir.join("files");
    std::fs::create_dir_all(&dir).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not create on-disk book storage.",
        )
    })?;
    let relative = format!("files/{digest}.{kind}");
    let absolute = data_dir.join(&relative);
    std::fs::write(&absolute, bytes).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not write the book to disk.",
        )
    })?;
    // Empty BLOB satisfies NOT NULL while the bytes live on disk.
    Ok((Vec::new(), Some(relative)))
}

fn load_book_bytes(
    data_dir: &std::path::Path,
    inline: Vec<u8>,
    relative: Option<String>,
) -> ApiResult<Vec<u8>> {
    let Some(relative) = relative.filter(|p| !p.is_empty()) else {
        return Ok(inline);
    };
    if relative.contains("..") || PathBuf::from(&relative).is_absolute() {
        return Err(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Invalid on-disk book path.",
        ));
    }
    std::fs::read(data_dir.join(&relative)).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read the on-disk book file.",
        )
    })
}

fn remove_book_file(data_dir: &std::path::Path, relative: Option<&str>) {
    let Some(relative) = relative.filter(|p| !p.is_empty()) else {
        return;
    };
    if relative.contains("..") || PathBuf::from(relative).is_absolute() {
        return;
    }
    let _ = std::fs::remove_file(data_dir.join(relative));
}

fn load_owned_book_file(
    db: &Connection,
    data_dir: &std::path::Path,
    user: i64,
    id: i64,
) -> ApiResult<(Vec<u8>, String)> {
    let (inline, storage_path, format) = owned_book_file_row(db, user, id)?;
    Ok((load_book_bytes(data_dir, inline, storage_path)?, format))
}

fn owned_book_file_row(
    db: &Connection,
    user: i64,
    id: i64,
) -> ApiResult<(Vec<u8>, Option<String>, String)> {
    db.query_row(
        "SELECT f.content,f.storage_path,f.format FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=? AND b.user_id=?",
        params![id, user],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()?
    .ok_or(ApiError(StatusCode::NOT_FOUND, "Book file not found."))
}

fn book_file_mime(format: &str) -> (&'static str, &'static str) {
    match format {
        "pdf" => ("application/pdf", "attachment; filename=book.pdf"),
        "cbz" | "cbr" | "cb7" | "djvu" => (
            "application/vnd.comicbook+zip",
            "attachment; filename=book.cbz",
        ),
        "mp3" => ("audio/mpeg", "inline; filename=book.mp3"),
        "m4b" => ("audio/mp4", "inline; filename=book.m4b"),
        "opus" => ("audio/ogg", "inline; filename=book.opus"),
        "ogg" => ("audio/ogg", "inline; filename=book.ogg"),
        "flac" => ("audio/flac", "inline; filename=book.flac"),
        "audiozip" => ("application/zip", "attachment; filename=audiobook.zip"),
        _ => ("application/epub+zip", "attachment; filename=book.epub"),
    }
}

/// Parse `Range: bytes=start-end` (inclusive). Unsatisfiable ranges return None.
fn parse_bytes_range(header: &str, len: u64) -> Option<(u64, u64)> {
    let spec = header.strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (start_s, end_s) = spec.split_once('-')?;
    if start_s.is_empty() {
        let suffix: u64 = end_s.parse().ok()?;
        if suffix == 0 || len == 0 {
            return None;
        }
        let start = len.saturating_sub(suffix);
        return Some((start, len - 1));
    }
    let start: u64 = start_s.parse().ok()?;
    if start >= len {
        return None;
    }
    let end = if end_s.is_empty() {
        len - 1
    } else {
        end_s.parse::<u64>().ok()?.min(len - 1)
    };
    if end < start {
        return None;
    }
    Some((start, end))
}

/// Stream a spilled on-disk book without loading the whole file into RAM.
/// Honors `Range` so audiobook players can seek large M4Bs.
async fn stream_book_file(
    absolute: PathBuf,
    mime: &'static str,
    disposition: &'static str,
    range_header: Option<&header::HeaderValue>,
) -> ApiResult<Response> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    let mut file = tokio::fs::File::open(&absolute).await.map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read the on-disk book file.",
        )
    })?;
    let len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    let range = range_header
        .and_then(|v| v.to_str().ok())
        .and_then(|s| parse_bytes_range(s, len));

    if let Some((start, end)) = range {
        let take = end - start + 1;
        file.seek(std::io::SeekFrom::Start(start))
            .await
            .map_err(|_| {
                ApiError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not seek the on-disk book file.",
                )
            })?;
        let body =
            axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(file.take(take)));
        let mut response = body.into_response();
        *response.status_mut() = StatusCode::PARTIAL_CONTENT;
        let headers = response.headers_mut();
        headers.insert(header::CONTENT_TYPE, header::HeaderValue::from_static(mime));
        headers.insert(
            header::CONTENT_DISPOSITION,
            header::HeaderValue::from_static(disposition),
        );
        headers.insert(
            header::ACCEPT_RANGES,
            header::HeaderValue::from_static("bytes"),
        );
        headers.insert(header::CONTENT_LENGTH, header::HeaderValue::from(take));
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

    if range_header.is_some() && len > 0 {
        let mut response = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
        response.headers_mut().insert(
            header::CONTENT_RANGE,
            header::HeaderValue::from_str(&format!("bytes */{len}")).map_err(|_| {
                ApiError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not build Content-Range.",
                )
            })?,
        );
        return Ok(response);
    }

    let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(file));
    let mut response = (
        [
            (header::CONTENT_TYPE, mime),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        body,
    )
        .into_response();
    response.headers_mut().insert(
        header::ACCEPT_RANGES,
        header::HeaderValue::from_static("bytes"),
    );
    if len > 0 {
        response
            .headers_mut()
            .insert(header::CONTENT_LENGTH, header::HeaderValue::from(len));
    }
    Ok(response)
}

/// SQLite cannot ALTER a CHECK; rebuild epub_files when formats are missing.
fn migrate_epub_files_formats(db: &Connection) -> rusqlite::Result<()> {
    let sql: String = db.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='epub_files'",
        [],
        |r| r.get(0),
    )?;
    let _ = db.execute_batch("ALTER TABLE epub_files ADD COLUMN source BLOB");
    let _ = db.execute_batch("ALTER TABLE epub_files ADD COLUMN storage_path TEXT");
    let _ = db.execute_batch("ALTER TABLE epub_files ADD COLUMN source_path TEXT");
    let _ = db.execute_batch("ALTER TABLE epub_files ADD COLUMN conversion_status TEXT");
    let _ = db.execute_batch("ALTER TABLE epub_files ADD COLUMN conversion_error TEXT");
    let _ = db.execute_batch("ALTER TABLE epub_files ADD COLUMN chapter_markers TEXT");
    if sql.contains("'aax'") {
        return Ok(());
    }
    db.execute_batch(
        "PRAGMA foreign_keys=OFF;
         BEGIN;
         CREATE TABLE epub_files_new (
           book_id INTEGER PRIMARY KEY REFERENCES books(id),
           digest TEXT NOT NULL,
           content BLOB NOT NULL,
           cover BLOB,
           cover_mime TEXT,
           position TEXT,
           percentage REAL NOT NULL DEFAULT 0 CHECK(percentage BETWEEN 0 AND 1),
           format TEXT NOT NULL DEFAULT 'epub' CHECK(format IN ('epub','pdf','cbz','cbr','cb7','djvu','mp3','m4b','opus','ogg','flac','audiozip','aax')),
           source BLOB,
           storage_path TEXT,
           source_path TEXT,
           conversion_status TEXT,
           conversion_error TEXT,
           chapter_markers TEXT
         );
         INSERT INTO epub_files_new
           (book_id,digest,content,cover,cover_mime,position,percentage,format,source,storage_path,source_path,conversion_status,conversion_error,chapter_markers)
           SELECT book_id,digest,content,cover,cover_mime,position,percentage,format,
                  source,storage_path,source_path,conversion_status,conversion_error,chapter_markers
           FROM epub_files;
         DROP TABLE epub_files;
         ALTER TABLE epub_files_new RENAME TO epub_files;
         CREATE INDEX IF NOT EXISTS epub_digest ON epub_files(digest);
         COMMIT;
         PRAGMA foreign_keys=ON;",
    )?;
    Ok(())
}

pub fn app(database: PathBuf) -> rusqlite::Result<Router> {
    let db = Connection::open(&database)?;
    db.pragma_update(None, "journal_mode", "WAL")?;
    configure_connection(&db)?;
    db.execute_batch(include_str!("../schema.sql"))?;
    // Existing libraries created before PDF / reading-status support lack these columns.
    let _ =
        db.execute_batch("ALTER TABLE epub_files ADD COLUMN format TEXT NOT NULL DEFAULT 'epub'");
    let _ = db.execute_batch("ALTER TABLE annotations ADD COLUMN pdf TEXT");
    let _ = db
        .execute_batch("ALTER TABLE books ADD COLUMN reading_status TEXT NOT NULL DEFAULT 'open'");
    let _ = db.execute_batch("ALTER TABLE books ADD COLUMN series TEXT");
    let _ = db.execute_batch("ALTER TABLE books ADD COLUMN series_index REAL");
    let _ = db.execute_batch(
        "ALTER TABLE books ADD COLUMN merged_into INTEGER REFERENCES books(id) ON DELETE SET NULL",
    );
    let _ = db.execute_batch("CREATE INDEX IF NOT EXISTS books_merged_into ON books(merged_into)");
    let _ = db.execute_batch("ALTER TABLE epub_files ADD COLUMN original_filename TEXT");
    // Backfill finished from progress for libraries that only had % / pages.
    let _ = db.execute_batch(
        "UPDATE books SET reading_status='finished'
         WHERE reading_status='open' AND (
           pages_read=total_pages
           OR id IN (SELECT book_id FROM epub_files WHERE percentage=1)
         )",
    );
    migrate_epub_files_formats(&db)?;
    let _ = db.execute_batch(
        "CREATE TABLE IF NOT EXISTS aax_activation_cache (
           checksum TEXT PRIMARY KEY,
           activation_bytes TEXT NOT NULL CHECK(length(activation_bytes) = 8)
         )",
    );
    let _ = db.execute_batch(
        "CREATE TABLE IF NOT EXISTS collections (
           id INTEGER PRIMARY KEY,
           user_id INTEGER NOT NULL REFERENCES users(id),
           name TEXT NOT NULL,
           UNIQUE(user_id, name)
         );
         CREATE TABLE IF NOT EXISTS collection_books (
           collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
           book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
           PRIMARY KEY (collection_id, book_id)
         );
         CREATE TABLE IF NOT EXISTS smart_scopes (
           id INTEGER PRIMARY KEY,
           user_id INTEGER NOT NULL REFERENCES users(id),
           name TEXT NOT NULL,
           filter TEXT NOT NULL,
           UNIQUE(user_id, name)
         )",
    );
    // M11/M17 metadata tables come from schema.sql; backfill entities/covers.
    metadata::migrate(&db);
    let _ = reading_stats::migrate(&db);
    let _ = session_injectors::migrate(&db);
    let _ = kindle_delivery::migrate(&db);
    let _ = hardcover::migrate(&db);
    let _ = readwise::migrate(&db);
    let _ = storygraph::migrate(&db);
    let _ = koreader_sync::migrate(&db);
    let _ = kobo_sync::migrate(&db);
    let _ = opds::migrate(&db);
    let state = AppState {
        data_dir: database
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf(),
        database,
        login_attempts: Arc::new(Mutex::new(VecDeque::new())),
    };
    aax::resume_pending(&state);
    align::resume_pending(&state);
    metadata::resume_pending(&state);
    Ok(Router::new()
        .route("/", get(|| async { Html(include_str!("../../web/index.html")) }))
        .route("/app.css", get(|| async { ([(header::CONTENT_TYPE, "text/css")], include_str!("../../web/app.css")) }))
        .route("/app.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], include_str!("../../web/app.js")) }))
        .route("/api.mjs", get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], include_str!("../../web/api.mjs")) }))
        .route("/fonts/serif.ttf", get(|| async { ([(header::CONTENT_TYPE, "font/ttf")], include_bytes!("../../web/fonts/serif.ttf").as_slice()) }))
        .route("/fonts/sans.ttf", get(|| async { ([(header::CONTENT_TYPE, "font/ttf")], include_bytes!("../../web/fonts/sans.ttf").as_slice()) }))
        .route("/fonts/mono.ttf", get(|| async { ([(header::CONTENT_TYPE, "font/ttf")], include_bytes!("../../web/fonts/mono.ttf").as_slice()) }))
        .route("/ornaments/celestial.png", get(|| async { ([(header::CONTENT_TYPE, "image/png")], include_bytes!("../../web/ornaments/celestial.png").as_slice()) }))
        .route("/ornaments/sprig.png", get(|| async { ([(header::CONTENT_TYPE, "image/png")], include_bytes!("../../web/ornaments/sprig.png").as_slice()) }))
        .route("/ornaments/sprig-corner.png", get(|| async { ([(header::CONTENT_TYPE, "image/png")], include_bytes!("../../web/ornaments/sprig-corner.png").as_slice()) }))
        .route("/ornaments/sprig-trail.png", get(|| async { ([(header::CONTENT_TYPE, "image/png")], include_bytes!("../../web/ornaments/sprig-trail.png").as_slice()) }))
        .route("/ornaments/swallow.png", get(|| async { ([(header::CONTENT_TYPE, "image/png")], include_bytes!("../../web/ornaments/swallow.png").as_slice()) }))
        .route("/icons.svg", get(|| async { ([(header::CONTENT_TYPE, "image/svg+xml")], include_str!("../../web/icons.svg")) }))
        .route("/api/v1/health", get(|| async { Json(serde_json::json!({"status":"ok", "api_version":1, "version":option_env!("SHELF_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))})) }))
        .route("/api/v1/setup", get(setup_status).post(setup))
        .route("/api/v1/login", post(login))
        .route("/api/v1/logout", post(logout))
        .route("/api/v1/books", get(books).post(add_book))
        .route("/api/v1/books/merge", post(library_org::merge_editions))
        .route(
            "/api/v1/books/{id}",
            patch(patch_book).delete(epub::delete_book),
        )
        .route("/api/v1/authors", get(library_org::list_authors))
        .route(
            "/api/v1/collections",
            get(library_org::list_collections).post(library_org::create_collection),
        )
        .route(
            "/api/v1/collections/{id}",
            patch(library_org::patch_collection).delete(library_org::delete_collection),
        )
        .route(
            "/api/v1/scopes",
            get(library_org::list_scopes).post(library_org::create_scope),
        )
        .route(
            "/api/v1/scopes/{id}",
            patch(library_org::patch_scope).delete(library_org::delete_scope),
        )
        .route("/api/v1/library/roots", get(library_org::library_roots))
        .route("/api/v1/library/scan", post(library_org::library_scan))
        .route("/api/v1/metadata/search", post(metadata::search))
        .route("/api/v1/meta/image", get(metadata::proxy_image))
        .route(
            "/api/v1/metadata/providers",
            get(metadata::list_providers).post(metadata::add_external_provider),
        )
        .route("/api/v1/metadata/providers/{id}", patch(metadata::patch_provider))
        .route(
            "/api/v1/metadata/settings",
            get(metadata::get_settings).put(metadata::put_settings),
        )
        .route("/api/v1/metadata/fetch/status", get(metadata::fetch_status))
        .route("/api/v1/metadata/fetch/run", post(metadata::fetch_run))
        .route("/api/v1/metadata/fetch/pause", post(metadata::fetch_pause))
        .route("/api/v1/metadata/fetch/resume", post(metadata::fetch_resume))
        .route("/api/v1/metadata/fetch/cancel", post(metadata::fetch_cancel))
        .route(
            "/api/v1/books/{id}/metadata",
            get(metadata::get_book_metadata).patch(metadata::patch_book_metadata),
        )
        .route("/api/v1/books/{id}/metadata/apply", post(metadata::apply))
        .route("/api/v1/books/{id}/metadata/autofill", post(metadata::autofill))
        .route("/api/v1/books/{id}/metadata/refresh", post(metadata::refresh_book))
        .route(
            "/api/v1/books/{id}/covers",
            get(metadata::list_covers).post(metadata::add_cover_url),
        )
        .route(
            "/api/v1/books/{id}/covers/search",
            post(metadata::search_covers),
        )
        .route(
            "/api/v1/books/{id}/covers/{cover_id}",
            get(metadata::cover_bytes),
        )
        .route(
            "/api/v1/books/{id}/covers/{cover_id}/activate",
            post(metadata::activate_cover),
        )
        .route("/api/v1/books/{id}/progress", post(update_progress))
        .route("/api/v1/books/{id}/finished", post(mark_finished))
        .route("/api/v1/books/{id}/finishes", get(reading_stats::list_finishes))
        .route(
            "/api/v1/books/{id}/finishes/{event_id}",
            patch(reading_stats::patch_finish).delete(reading_stats::delete_finish),
        )
        .route("/api/v1/books/{id}/unfinished", post(mark_unfinished))
        .route("/api/v1/books/{id}/rereading", post(mark_rereading))
        .route("/api/v1/books/import", post(epub::import).layer(DefaultBodyLimit::disable()))
        .route("/api/v1/books/{id}/file", get(epub::file))
        .route("/api/v1/books/{id}/source", get(epub::source_file))
        .route("/api/v1/books/{id}/cover", get(epub::cover))
        .route("/api/v1/books/{id}/pages", get(epub::comic_page_count))
        .route("/api/v1/books/{id}/pages/{page}", get(epub::comic_page))
        // Book Orbit DjVu text sheet path.
        .route(
            "/api/v1/djvu/files/{id}/pages/{page}/text",
            get(epub::djvu_page_text),
        )
        .route(
            "/api/v1/tts/text/{id}/{chapter}",
            get(tts::chapter_text),
        )
        .route(
            "/api/v1/epub/{id}/media-overlay",
            get(media_overlay::media_overlay_playlist),
        )
        .route("/api/v1/epub/{id}/info", get(epub_package::book_info))
        .route(
            "/api/v1/epub/{id}/file/{*path}",
            get(epub_package::package_file),
        )
        .route(
            "/api/v1/audiobooks/{id}/manifest",
            get(audiobook::manifest),
        )
        .route(
            "/api/v1/audiobooks/{id}/assets/{asset_id}/content",
            get(audiobook::asset_content),
        )
        .route(
            "/api/v1/audiobooks/{id}/playback-state",
            get(audiobook::get_playback_state)
                .put(audiobook::put_playback_state)
                .delete(audiobook::delete_playback_state),
        )
        .route("/api/v1/works", get(align::list_works_http).post(align::create_work_http))
        .route("/api/v1/works/{id}", get(align::get_work_http))
        .route("/api/v1/works/{id}/align", post(align::start_work_http))
        .route("/api/v1/tts/providers", get(tts::list_providers))
        .route("/api/v1/tts/voices", get(tts::list_voices))
        .route("/api/v1/tts/synthesize", post(tts::synthesize))
        .route("/api/v1/tts/preview", post(tts::preview))
        .route(
            "/api/v1/tts/admin/providers",
            get(tts::admin_list_providers).post(tts::admin_add_provider),
        )
        .route(
            "/api/v1/tts/admin/providers/order",
            put(tts::admin_reorder_providers),
        )
        .route(
            "/api/v1/tts/admin/providers/{id}",
            put(tts::admin_update_provider).delete(tts::admin_delete_provider),
        )
        .route(
            "/api/v1/tts/admin/providers/{id}/test",
            post(tts::admin_test_provider),
        )
        .route(
            "/api/v1/tts/admin/providers/{id}/voices/discover",
            get(tts::admin_discover_voices),
        )
        .route("/api/v1/books/{id}/location", post(epub::location))
        .route(
            "/api/v1/books/files/{id}/sessions",
            post(reading_stats::post_file_session),
        )
        .route(
            "/api/v1/books/{id}/sessions",
            get(reading_stats::list_book_sessions).post(reading_stats::post_book_session),
        )
        .route(
            "/api/v1/books/{id}/sessions/{client_session_id}",
            delete(reading_stats::delete_book_session),
        )
        .route(
            "/api/v1/books/{id}/koreader/page-stats",
            post(session_injectors::post_koreader_page_stats),
        )
        .route(
            "/api/v1/books/{id}/kobo/statistics",
            post(session_injectors::post_kobo_statistics),
        )
        .route(
            "/api/v1/books/{id}/send-to-kindle",
            post(kindle_delivery::post_kindle_send),
        )
        .route(
            "/api/v1/kindle-delivery",
            get(kindle_delivery::get_kindle_settings).route(
            "/api/v1/hardcover",
            get(hardcover::get_hardcover_settings)
                .put(hardcover::put_hardcover_settings)
                .delete(hardcover::delete_hardcover_settings),
        )
        .route(
            "/api/v1/hardcover/validate",
            post(hardcover::validate_hardcover_token),
        )
        .route("/api/v1/hardcover/sync", post(hardcover::sync_hardcover_all))
        .route(
            "/api/v1/books/{id}/hardcover/sync",
            post(hardcover::sync_hardcover_book),
        )
.route(
            "/api/v1/readwise",
            get(readwise::get_readwise_settings)
                .put(readwise::put_readwise_settings)
                .delete(readwise::delete_readwise_settings),
        )
        .route(
            "/api/v1/readwise/validate",
            post(readwise::validate_readwise_token),
        )
        .route("/api/v1/readwise/sync", post(readwise::sync_readwise))
.route(
            "/api/v1/storygraph",
            get(storygraph::get_storygraph_settings)
                .put(storygraph::put_storygraph_settings)
                .delete(storygraph::delete_storygraph_settings),
        )
        .route(
            "/api/v1/storygraph/validate",
            post(storygraph::validate_storygraph_cookies),
        )
        .route("/api/v1/storygraph/sync", post(storygraph::sync_storygraph_all))
        .route(
            "/api/v1/books/{id}/storygraph/sync",
            post(storygraph::sync_storygraph_book),
        )
        .route(
            "/api/v1/books/{id}/storygraph/link",
            post(storygraph::link_storygraph_book),
        )
.route(
            "/api/v1/koreader",
            get(koreader_sync::get_koreader_settings)
                .put(koreader_sync::put_koreader_settings)
                .delete(koreader_sync::delete_koreader_settings),
        )
        .route(
            "/api/v1/koreader/unmatched",
            get(koreader_sync::list_unmatched),
        )
        .route(
            "/api/v1/koreader/unmatched/{hash}/link",
            post(koreader_sync::link_unmatched),
        )
        .route(
            "/api/v1/koreader/unmatched/{hash}/dismiss",
            post(koreader_sync::dismiss_unmatched),
        )
        .route(
            "/api/v1/koreader/hash-links",
            get(koreader_sync::list_hash_links),
        )
        .route(
            "/api/v1/koreader/hash-links/{hash}",
            delete(koreader_sync::unlink_hash),
        )
        .route(
            "/api/v1/koreader/plugin/match-check",
            post(koreader_sync::plugin_match_check),
        )
        .route(
            "/api/v1/koreader/plugin/annotations",
            post(koreader_sync::plugin_annotations_upload),
        )
        .route(
            "/api/v1/koreader/plugin/annotations/exchange",
            post(koreader_sync::plugin_annotations_exchange),
        )
        .route(
            "/api/v1/koreader/plugin/progress",
            post(koreader_sync::plugin_bulk_progress),
        )
        .route(
            "/api/v1/koreader/plugin/catalog",
            get(koreader_sync::plugin_catalog),
        )
.route(
            "/api/v1/kobo",
            get(kobo_sync::get_kobo_settings).put(kobo_sync::put_kobo_settings),
        )
        .route("/api/v1/kobo/devices", post(kobo_sync::create_kobo_device))
        .route(
            "/api/v1/kobo/devices/{id}",
            patch(kobo_sync::rename_kobo_device).delete(kobo_sync::delete_kobo_device),
        )
        .route(
            "/api/v1/kobo/{device_token}/v1/initialization",
            get(kobo_sync::device_initialization),
        )
        .route(
            "/api/v1/kobo/{device_token}/v1/library/sync",
            get(kobo_sync::device_library_sync),
        )
        .route(
            "/api/v1/kobo/{device_token}/v1/library/{book_key}/state",
            get(kobo_sync::device_get_reading_state).put(kobo_sync::device_put_reading_state),
        )
        .route(
            "/api/v1/kobo/{device_token}/v1/library/{book_key}/download",
            get(kobo_sync::device_download),
        )
.put(kindle_delivery::put_kindle_settings),
        )
        .route("/api/v1/opds", get(opds::root))
        .route("/api/v1/opds/search.opds", get(opds::search_desc))
        .route("/api/v1/opds/catalog", get(opds::catalog))
        .route("/api/v1/opds/recent", get(opds::recent))
        .route("/api/v1/opds/authors", get(opds::authors))
        .route("/api/v1/opds/series", get(opds::series))
        .route("/api/v1/opds/collections", get(opds::collections))
        .route("/api/v1/opds/{id}/file", get(opds::download))
        .route("/api/v1/opds/{id}/cover", get(opds::cover))
        .route(
            "/api/v1/opds-users",
            get(opds::list_users).post(opds::create_user),
        )
        .route("/api/v1/opds-users/{id}", delete(opds::delete_user))
        .route("/api/v1/reading-sessions", get(reading_stats::list_sessions))
        .route("/api/v1/reading-daily", get(reading_stats::list_daily))
        .route(
            "/api/v1/reading-prefs",
            get(reading_stats::get_prefs).put(reading_stats::put_prefs),
        )
        .route("/api/v1/insights/reading", get(insights::reading_insights))
        .route("/api/v1/insights/widgets", get(insights::reading_widgets))
        .route("/api/v1/insights/achievements", get(achievements::list_achievements))
        .route("/api/v1/insights/library", get(library_health::library_insights))
        .route(
            "/api/v1/books/{id}/bookmarks",
            get(reader_data::list_bookmarks).post(reader_data::create_bookmark),
        )
        .route(
            "/api/v1/books/{id}/bookmarks/{mark_id}",
            patch(reader_data::patch_bookmark).delete(reader_data::delete_bookmark),
        )
        .route(
            "/api/v1/books/{id}/annotations",
            get(reader_data::list_annotations).post(reader_data::create_annotation),
        )
        .route(
            "/api/v1/books/{id}/annotations/{annotation_id}",
            patch(reader_data::patch_annotation).delete(reader_data::delete_annotation),
        )
        .route(
            "/api/v1/reader/preferences/{id}",
            get(reader_data::get_preferences)
                .patch(reader_data::put_preferences)
                .put(reader_data::put_preferences)
                .delete(reader_data::delete_preferences),
        )
        .route(
            "/api/v1/reader/defaults",
            get(reader_data::get_defaults)
                .patch(reader_data::put_defaults)
                .put(reader_data::put_defaults)
                .delete(reader_data::delete_defaults),
        )
        .route(
            "/api/v1/reader/defaults/{group}",
            get(reader_data::get_default_group)
                .patch(reader_data::put_default_group)
                .put(reader_data::put_default_group)
                .delete(reader_data::delete_default_group),
        )
        // Book Orbit EPUB reader (AGPL-3.0-only). Built assets live in web/orbit-reader/dist.
        .route("/reader", get(epub::reader))
        .route("/reader/{*rest}", get(epub::reader_path))
        .route("/orbit-reader/{*path}", get(epub::orbit_asset))
        .route("/api/v1/stats", get(stats))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(axum::middleware::map_response(|mut response: Response| async {
            let headers = response.headers_mut();
            headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
            if !headers.contains_key(header::CONTENT_SECURITY_POLICY) { headers.insert(header::CONTENT_SECURITY_POLICY, "default-src 'self'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'".parse().unwrap()); }
            response
        }))
        .with_state(state))
}

async fn setup_status(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    state
        .db(|db| {
            let count: i64 = db.query_row("SELECT count(*) FROM users", [], |r| r.get(0))?;
            Ok(Json(serde_json::json!({"setup_required":count == 0})))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    username: String,
    password: String,
}

fn validate_credentials(value: &Credentials) -> ApiResult<()> {
    if value.username.trim().is_empty()
        || value.username.len() > 100
        || value.password.len() < 12
        || value.password.len() > 1024
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Use a username of 1–100 characters and a password of 12–1024 bytes.",
        ));
    }
    Ok(())
}

async fn setup(
    State(state): State<AppState>,
    Json(value): Json<Credentials>,
) -> ApiResult<StatusCode> {
    validate_credentials(&value)?;
    state
        .db(move |db| {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let count: i64 = tx.query_row("SELECT count(*) FROM users", [], |r| r.get(0))?;
            if count != 0 {
                return Err(ApiError(StatusCode::CONFLICT, "Setup is already complete."));
            }
            let salt = SaltString::generate(&mut OsRng);
            let hash = Argon2::default()
                .hash_password(value.password.as_bytes(), &salt)
                .map_err(|_| {
                    ApiError(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Account creation failed.",
                    )
                })?
                .to_string();
            tx.execute(
                "INSERT INTO users(username, password_hash) VALUES (?, ?)",
                params![value.username.trim(), hash],
            )?;
            tx.commit()?;
            Ok(StatusCode::CREATED)
        })
        .await
}

/// Sliding idle window renewed on every authenticated request. Active clients stay
/// signed in; idle clients must sign in again after this period.
const SESSION_IDLE_SECS: i64 = 30 * 24 * 60 * 60;
/// Browser cookie lifetime (Chrome caps ~400d). Server idle sliding is authoritative.
const SESSION_COOKIE_MAX_AGE: i64 = 400 * 24 * 60 * 60;

fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

fn authenticate(db: &Connection, headers: &HeaderMap) -> ApiResult<i64> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .find_map(|item| item.trim().strip_prefix("shelf_session="))
        });
    let token = bearer
        .or(cookie)
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "Please sign in."))?;
    let hash = token_hash(token);
    let user_id: i64 = db
        .query_row(
            "SELECT user_id FROM sessions WHERE token_hash=? AND expires_at > unixepoch()",
            [&hash],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(ApiError(
            StatusCode::UNAUTHORIZED,
            "Your session has expired. Please sign in.",
        ))?;
    db.execute(
        "UPDATE sessions SET expires_at = unixepoch() + ?1 WHERE token_hash = ?2",
        params![SESSION_IDLE_SECS, hash],
    )?;
    Ok(user_id)
}

async fn login(
    State(state): State<AppState>,
    Json(value): Json<Credentials>,
) -> ApiResult<Response> {
    state.limit_login()?;
    if value.username.len() > 100 || value.password.len() > 1024 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid credentials."));
    }
    state.db(move |db| {
        let record: Option<(i64, String)> = db.query_row("SELECT id,password_hash FROM users WHERE username=?", [value.username.trim()], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        // Always perform a password verification, including for unknown usernames.
        let dummy_salt = SaltString::encode_b64(b"shelf-login-padding").map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Sign in failed."))?;
        let dummy = Argon2::default().hash_password(b"not-a-user-password", &dummy_salt).map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Sign in failed."))?.to_string();
        let hash = record.as_ref().map(|r| r.1.as_str()).unwrap_or(&dummy);
        let parsed = PasswordHash::new(hash).map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Sign in failed."))?;
        if Argon2::default().verify_password(value.password.as_bytes(), &parsed).is_err() || record.is_none() {
            return Err(ApiError(StatusCode::UNAUTHORIZED, "Username or password is incorrect."));
        }
        let mut bytes = [0u8; 32]; OsRng.fill_bytes(&mut bytes);
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        db.execute("DELETE FROM sessions WHERE expires_at <= unixepoch()", [])?;
        db.execute(
            "INSERT INTO sessions(token_hash,user_id,expires_at) VALUES (?,?,unixepoch()+?)",
            params![token_hash(&token), record.unwrap().0, SESSION_IDLE_SECS],
        )?;
        let secure = if std::env::var("SHELF_SECURE_COOKIE").as_deref() == Ok("true") { "; Secure" } else { "" };
        let cookie = format!(
            "shelf_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={SESSION_COOKIE_MAX_AGE}{secure}"
        );
        Ok(([(header::SET_COOKIE, cookie), (header::CACHE_CONTROL, "no-store".into())], Json(serde_json::json!({"token":token}))).into_response())
    }).await
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            // Explicit sign out revokes all sessions for this initial single-account edition.
            db.execute("DELETE FROM sessions WHERE user_id=?", [user])?;
            Ok((
                [(
                    header::SET_COOKIE,
                    "shelf_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
                )],
                StatusCode::NO_CONTENT,
            )
                .into_response())
        })
        .await
}

#[derive(Serialize)]
struct Book {
    id: i64,
    title: String,
    author: String,
    total_pages: Option<i64>,
    pages_read: i64,
    reading_status: String,
    series: Option<String>,
    series_index: Option<f64>,
    /// When set, this edition is folded under another shelf card.
    #[serde(skip_serializing_if = "Option::is_none")]
    merged_into: Option<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    genres: Vec<String>,
    #[serde(flatten)]
    epub: Option<EpubInfo>,
}
#[derive(Serialize)]
struct EpubInfo {
    format: &'static str,
    /// SHA-256 of the uploaded bytes (hex). Clients key offline caches on this.
    digest: String,
    percentage: f64,
    position: Option<String>,
    has_cover: bool,
    /// Original upload basename when the client sent one.
    #[serde(skip_serializing_if = "Option::is_none")]
    original_filename: Option<String>,
    /// AAX conversion job status when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    conversion_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    conversion_error: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewBook {
    title: String,
    author: String,
    total_pages: i64,
}

#[derive(Deserialize)]
struct BooksQuery {
    limit: Option<i64>,
    offset: Option<i64>,
    author: Option<String>,
    series: Option<String>,
    status: Option<String>,
    format: Option<String>,
    collection_id: Option<i64>,
    q: Option<String>,
}

fn map_book_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Book> {
    let imported = r.get::<_, Option<i64>>(6)?.is_some();
    let format = r.get::<_, Option<String>>(10)?;
    let digest = r.get::<_, Option<String>>(11)?;
    let conversion_status: Option<String> = r.get(12)?;
    let conversion_error: Option<String> = r.get(13)?;
    let original_filename: Option<String> = r.get(17)?;
    Ok(Book {
        id: r.get(0)?,
        title: r.get(1)?,
        author: r.get(2)?,
        total_pages: r.get(3)?,
        pages_read: r.get(4)?,
        reading_status: r.get(5)?,
        series: r.get(14)?,
        series_index: r.get(15)?,
        merged_into: r.get(18)?,
        genres: r
            .get::<_, Option<String>>(16)?
            .map(|joined| joined.split('\u{1f}').map(str::to_string).collect())
            .unwrap_or_default(),
        epub: if imported {
            Some(EpubInfo {
                format: match format.as_deref() {
                    Some("pdf") => "pdf",
                    Some("cbz") => "cbz",
                    Some("cbr") => "cbr",
                    Some("cb7") => "cb7",
                    Some("djvu") => "djvu",
                    Some("mp3") => "mp3",
                    Some("m4b") => "m4b",
                    Some("opus") => "opus",
                    Some("ogg") => "ogg",
                    Some("flac") => "flac",
                    Some("audiozip") => "audiozip",
                    Some("aax") => "aax",
                    _ => "epub",
                },
                digest: digest.unwrap_or_default(),
                percentage: r.get(7)?,
                position: r.get(8)?,
                has_cover: r.get(9)?,
                original_filename,
                conversion_status,
                conversion_error,
            })
        } else {
            None
        },
    })
}

async fn books(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<BooksQuery>,
) -> ApiResult<Json<Vec<Book>>> {
    let limit = query.limit.unwrap_or(500).clamp(1, 1000);
    let offset = query.offset.unwrap_or(0).max(0);
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            if let Some(collection_id) = query.collection_id {
                let owned: bool = db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM collections WHERE id=? AND user_id=?)",
                    params![collection_id, user],
                    |r| r.get(0),
                )?;
                if !owned {
                    return Err(ApiError(StatusCode::NOT_FOUND, "Collection not found."));
                }
            }
            let mut sql = String::from(
                "SELECT b.id,b.title,b.author,CASE WHEN f.book_id IS NULL THEN b.total_pages END, \
                 b.pages_read,b.reading_status,f.book_id,f.percentage,f.position,f.cover IS NOT NULL, \
                 f.format,f.digest,f.conversion_status,f.conversion_error,b.series,b.series_index, \
                 (SELECT group_concat(name, char(31)) FROM (SELECT g.name FROM book_genres bg \
                 JOIN genres g ON g.id=bg.genre_id WHERE bg.book_id=b.id ORDER BY g.name)), \
                 f.original_filename,b.merged_into \
                 FROM books b LEFT JOIN epub_files f ON f.book_id=b.id WHERE b.user_id=?",
            );
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(user)];
            if let Some(ref author) = query.author {
                sql.push_str(" AND b.author=?");
                params_vec.push(Box::new(author.clone()));
            }
            if let Some(ref series) = query.series {
                sql.push_str(" AND b.series=?");
                params_vec.push(Box::new(series.clone()));
            }
            if let Some(ref status) = query.status {
                sql.push_str(" AND b.reading_status=?");
                params_vec.push(Box::new(status.clone()));
            }
            if let Some(ref format) = query.format {
                sql.push_str(" AND f.format=?");
                params_vec.push(Box::new(format.clone()));
            }
            if let Some(collection_id) = query.collection_id {
                sql.push_str(
                    " AND b.id IN (SELECT book_id FROM collection_books WHERE collection_id=?)",
                );
                params_vec.push(Box::new(collection_id));
            }
            if let Some(ref q) = query.q {
                let needle = format!("%{}%", q);
                sql.push_str(" AND (b.title LIKE ? COLLATE NOCASE OR b.author LIKE ? COLLATE NOCASE)");
                params_vec.push(Box::new(needle.clone()));
                params_vec.push(Box::new(needle));
            }
            sql.push_str(" ORDER BY b.id DESC LIMIT ? OFFSET ?");
            params_vec.push(Box::new(limit));
            params_vec.push(Box::new(offset));
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|p| p.as_ref()).collect();
            let mut stmt = db.prepare(&sql)?;
            let books = stmt
                .query_map(param_refs.as_slice(), map_book_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Json(books))
        })
        .await
}

async fn add_book(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(value): Json<NewBook>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    if value.title.trim().is_empty()
        || value.title.len() > 500
        || value.author.len() > 300
        || !(1..=1_000_000).contains(&value.total_pages)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Enter a title, an author up to 300 characters, and 1–1,000,000 pages.",
        ));
    }
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            db.execute(
                "INSERT INTO books(user_id,title,author,total_pages) VALUES (?,?,?,?)",
                params![
                    user,
                    value.title.trim(),
                    value.author.trim(),
                    value.total_pages
                ],
            )?;
            Ok((
                StatusCode::CREATED,
                Json(serde_json::json!({"id":db.last_insert_rowid()})),
            ))
        })
        .await
}

fn null_as_clear_str<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<String>>, D::Error> {
    Ok(Some(Option::<String>::deserialize(d)?))
}

fn null_as_clear_f64<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<f64>>, D::Error> {
    Ok(Some(Option::<f64>::deserialize(d)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateBook {
    title: String,
    author: String,
    #[serde(default, deserialize_with = "null_as_clear_str")]
    series: Option<Option<String>>,
    #[serde(default, deserialize_with = "null_as_clear_f64")]
    series_index: Option<Option<f64>>,
}

/// Edit catalogue metadata without re-importing the file.
async fn patch_book(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(value): Json<UpdateBook>,
) -> ApiResult<StatusCode> {
    if value.title.trim().is_empty() || value.title.len() > 500 || value.author.len() > 300 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Enter a title up to 500 characters and an author up to 300 characters.",
        ));
    }
    if let Some(Some(ref series)) = value.series {
        if series.len() > 300 {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "Series name must be at most 300 characters.",
            ));
        }
    }
    if let Some(Some(index)) = value.series_index {
        if !index.is_finite() {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "series_index must be a finite number.",
            ));
        }
    }
    let title = value.title.trim().to_string();
    let author = value.author.trim().to_string();
    let series_patch = value.series.map(|s| {
        s.and_then(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
    });
    let series_index_patch = value.series_index;
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let (cur_series, cur_index): (Option<String>, Option<f64>) = db
                .query_row(
                    "SELECT series, series_index FROM books WHERE id=? AND user_id=?",
                    params![id, user],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Book not found."))?;
            let series = match series_patch {
                Some(v) => v,
                None => cur_series,
            };
            let series_index = match series_index_patch {
                Some(v) => v,
                None => cur_index,
            };
            let changed = db.execute(
                "UPDATE books SET title=?, author=?, series=?, series_index=? WHERE id=? AND user_id=?",
                params![title, author, series, series_index, id, user],
            )?;
            if changed == 0 {
                return Err(ApiError(StatusCode::NOT_FOUND, "Book not found."));
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Progress {
    pages_read: i64,
}
async fn update_progress(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(value): Json<Progress>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let imported: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=? AND b.user_id=?)",params![id,user],|r|r.get(0))?;
            if imported { return Err(ApiError(StatusCode::BAD_REQUEST,"Use the reader to record progress.")); }
            let total: Option<i64> = db
                .query_row(
                    "SELECT total_pages FROM books WHERE id=? AND user_id=?",
                    params![id, user],
                    |r| r.get(0),
                )
                .optional()?;
            let total = total.ok_or(ApiError(StatusCode::NOT_FOUND, "Book not found."))?;
            if !(0..=total).contains(&value.pages_read) {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Progress must be between zero and the book's page count.",
                ));
            }
            db.execute(
                "UPDATE books SET pages_read=? WHERE id=? AND user_id=?",
                params![value.pages_read, id, user],
            )?;
            if value.pages_read == total {
                db.execute(
                    "UPDATE books SET reading_status='finished' WHERE id=? AND user_id=?",
                    params![id, user],
                )?;
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

/// Mirror text progress between a work's source EPUB and its readaloud.
/// Library cards show the EPUB; Read opens the readaloud — keep them in sync.
fn mirror_work_text_progress(
    db: &Connection,
    user: i64,
    book_id: i64,
    position: Option<&str>,
    percentage: Option<f64>,
    reading_status: Option<&str>,
) -> ApiResult<()> {
    let mut stmt = db.prepare(
        "SELECT epub_book_id, readaloud_book_id FROM book_works
         WHERE readaloud_book_id IS NOT NULL
           AND (epub_book_id = ?1 OR readaloud_book_id = ?1)
           AND epub_book_id IN (SELECT id FROM books WHERE user_id = ?2)",
    )?;
    let peers: Vec<i64> = stmt
        .query_map(params![book_id, user], |row| {
            let epub: i64 = row.get(0)?;
            let readaloud: i64 = row.get(1)?;
            Ok(if epub == book_id { readaloud } else { epub })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for peer in peers {
        if let Some(pct) = percentage {
            if let Some(pos) = position {
                db.execute(
                    "UPDATE epub_files SET position=?, percentage=? WHERE book_id=?",
                    params![pos, pct, peer],
                )?;
            } else {
                db.execute(
                    "UPDATE epub_files SET percentage=? WHERE book_id=?",
                    params![pct, peer],
                )?;
            }
        } else if let Some(pos) = position {
            db.execute(
                "UPDATE epub_files SET position=? WHERE book_id=?",
                params![pos, peer],
            )?;
        }
        if let Some(status) = reading_status {
            db.execute(
                "UPDATE books SET reading_status=? WHERE id=? AND user_id=?",
                params![status, peer, user],
            )?;
        }
    }
    Ok(())
}

/// Parse a client calendar day `YYYY-MM-DD` into an RFC3339 UTC noon stamp for stats.
pub(crate) fn parse_finished_day(day: &str) -> ApiResult<String> {
    let day = day.trim();
    let parts: Vec<&str> = day.split('-').collect();
    if parts.len() != 3 || day.len() != 10 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "finished_at must be YYYY-MM-DD.",
        ));
    }
    let year: i32 = parts[0]
        .parse()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "finished_at must be YYYY-MM-DD."))?;
    let month: u32 = parts[1]
        .parse()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "finished_at must be YYYY-MM-DD."))?;
    let day_n: u32 = parts[2]
        .parse()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "finished_at must be YYYY-MM-DD."))?;
    if !(1970..=2100).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day_n) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "finished_at must be YYYY-MM-DD.",
        ));
    }
    let today = {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let days = secs / 86400;
        // Civil date from Unix day count (Howard Hinnant algorithm).
        let z = days + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = (z - era * 146097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = mp as i64 + if mp < 10 { 3 } else { -9 };
        let y = y + if m <= 2 { 1 } else { 0 };
        (y as i32, m as u32, d as u32)
    };
    if (year, month, day_n) > today {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "finished_at cannot be in the future.",
        ));
    }
    Ok(format!("{year:04}-{month:02}-{day_n:02}T12:00:00Z"))
}

/// Set catalogue lifecycle status; finished also completes progress, reopen clears it.
fn set_reading_status(
    db: &Connection,
    user: i64,
    id: i64,
    status: &str,
    finished_at: Option<&str>,
) -> ApiResult<()> {
    let imported: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=? AND b.user_id=?)",
        params![id, user],
        |r| r.get(0),
    )?;
    let owned = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM books WHERE id=? AND user_id=?)",
        params![id, user],
        |r| r.get::<_, bool>(0),
    )?;
    if !owned {
        return Err(ApiError(StatusCode::NOT_FOUND, "Book not found."));
    }
    match status {
        "finished" => {
            if imported {
                db.execute(
                    "UPDATE epub_files SET percentage=1 WHERE book_id=?",
                    params![id],
                )?;
            } else {
                db.execute(
                    "UPDATE books SET pages_read=total_pages WHERE id=? AND user_id=?",
                    params![id, user],
                )?;
            }
        }
        "unfinished" | "rereading" => {
            // Keep imported position so re-open can resume; clear completion only.
            if imported {
                db.execute(
                    "UPDATE epub_files SET percentage=0 WHERE book_id=?",
                    params![id],
                )?;
            } else {
                db.execute(
                    "UPDATE books SET pages_read=0 WHERE id=? AND user_id=?",
                    params![id, user],
                )?;
            }
        }
        _ => {
            return Err(ApiError(StatusCode::BAD_REQUEST, "Unknown reading status."));
        }
    }
    db.execute(
        "UPDATE books SET reading_status=? WHERE id=? AND user_id=?",
        params![status, id, user],
    )?;
    let at = if status == "finished" {
        finished_at.map(parse_finished_day).transpose()?
    } else {
        None
    };
    reading_stats::record_event_at(
        db,
        user,
        id,
        status,
        "manual",
        serde_json::json!({ "status": status }),
        at.as_deref(),
    )?;
    let pct = match status {
        "finished" => Some(1.0),
        "unfinished" | "rereading" => Some(0.0),
        _ => None,
    };
    mirror_work_text_progress(db, user, id, None, pct, Some(status))?;
    Ok(())
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct MarkFinishedBody {
    /// Client calendar day `YYYY-MM-DD`; defaults to today when omitted.
    finished_at: Option<String>,
}

async fn mark_finished(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    // `null` / omitted fields: Json<Option<_>> keeps existing Value::Null callers working.
    Json(body): Json<Option<MarkFinishedBody>>,
) -> ApiResult<StatusCode> {
    let finished_at = body.and_then(|b| b.finished_at);
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            set_reading_status(db, user, id, "finished", finished_at.as_deref())?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

async fn mark_unfinished(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            set_reading_status(db, user, id, "unfinished", None)?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

async fn mark_rereading(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            set_reading_status(db, user, id, "rereading", None)?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

async fn stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    state.db(move |db| {
        let user = authenticate(db, &headers)?;
        // pages_read stays manual-only (decision 0003); average_progress folds EPUB % and page ratios.
        // Hide work audio + readaloud editions so a Combine pair counts as one shelf book.
        let values: (i64, i64, i64, i64, Option<f64>) = db.query_row(
            "SELECT count(*),
                    coalesce(sum(b.pages_read),0),
                    coalesce(sum(CASE WHEN b.reading_status='finished' THEN 1 ELSE 0 END),0),
                    coalesce(sum(CASE WHEN b.reading_status='rereading' OR (b.reading_status='open' AND CASE WHEN f.book_id IS NULL THEN b.pages_read>0 AND b.pages_read<b.total_pages ELSE f.position IS NOT NULL AND f.percentage<1 END) THEN 1 ELSE 0 END),0),
                    (SELECT avg(CASE WHEN f2.book_id IS NOT NULL THEN f2.percentage
                                     WHEN b2.total_pages>0 THEN cast(b2.pages_read as real)/b2.total_pages END)
                     FROM books b2 LEFT JOIN epub_files f2 ON f2.book_id=b2.id
                     WHERE b2.user_id=?1
                       AND b2.merged_into IS NULL
                       AND b2.id NOT IN (SELECT audio_book_id FROM book_works UNION SELECT readaloud_book_id FROM book_works WHERE readaloud_book_id IS NOT NULL)
                       AND CASE WHEN f2.book_id IS NOT NULL THEN f2.position IS NOT NULL ELSE b2.pages_read>0 END)
             FROM books b LEFT JOIN epub_files f ON f.book_id=b.id
             WHERE b.user_id=?1
               AND b.merged_into IS NULL
               AND b.id NOT IN (SELECT audio_book_id FROM book_works UNION SELECT readaloud_book_id FROM book_works WHERE readaloud_book_id IS NOT NULL)",
            [user],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
        Ok(Json(serde_json::json!({
            "books": values.0,
            "pages_read": values.1,
            "finished": values.2,
            "reading": values.3,
            "average_progress": values.4.unwrap_or(0.0),
        })))
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_book_bytes_spill_to_disk_under_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_path_buf();
        let digest = "abc123";
        let payload = vec![7u8; 64];
        let (inline, path) =
            store_book_bytes_limited(&data_dir, digest, "epub", &payload, 32).unwrap();
        assert!(inline.is_empty());
        let relative = path.expect("spilled path");
        assert_eq!(relative, format!("files/{digest}.epub"));
        assert_eq!(
            load_book_bytes(&data_dir, inline, Some(relative.clone())).unwrap(),
            payload
        );
        remove_book_file(&data_dir, Some(relative.as_str()));
        assert!(!data_dir.join(&relative).exists());
    }

    #[test]
    fn parse_bytes_range_accepts_start_end_and_suffix() {
        assert_eq!(parse_bytes_range("bytes=0-0", 10), Some((0, 0)));
        assert_eq!(parse_bytes_range("bytes=2-5", 10), Some((2, 5)));
        assert_eq!(parse_bytes_range("bytes=8-", 10), Some((8, 9)));
        assert_eq!(parse_bytes_range("bytes=-3", 10), Some((7, 9)));
        assert_eq!(parse_bytes_range("bytes=10-12", 10), None);
        assert_eq!(parse_bytes_range("bytes=0-1,2-3", 10), None);
    }

    #[tokio::test]
    async fn stream_book_file_returns_partial_content_for_range() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.m4b");
        std::fs::write(&path, b"0123456789ABCDEF").unwrap();
        let range = header::HeaderValue::from_static("bytes=4-7");
        let response =
            stream_book_file(path, "audio/mp4", "inline; filename=book.m4b", Some(&range))
                .await
                .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.headers().get(header::CONTENT_RANGE).unwrap(),
            "bytes 4-7/16"
        );
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(&body[..], b"4567");
    }

    #[test]
    fn sign_in_limit_rejects_bursts_and_expires_old_attempts() {
        let state = AppState {
            database: PathBuf::new(),
            data_dir: PathBuf::new(),
            login_attempts: Arc::new(Mutex::new(VecDeque::new())),
        };
        for _ in 0..20 {
            assert!(state.limit_login().is_ok());
        }
        assert!(matches!(
            state.limit_login(),
            Err(ApiError(StatusCode::TOO_MANY_REQUESTS, _))
        ));
        let old = Instant::now() - Duration::from_secs(61);
        *state.login_attempts.lock().unwrap() = VecDeque::from(vec![old; 20]);
        assert!(state.limit_login().is_ok());
    }
}
