mod epub;
mod media_overlay;
mod reader_data;
mod tts;
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, patch, post, put},
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
        _ => ("application/epub+zip", "attachment; filename=book.epub"),
    }
}

/// Stream a spilled on-disk book without loading the whole file into RAM.
async fn stream_book_file(
    absolute: PathBuf,
    mime: &'static str,
    disposition: &'static str,
) -> ApiResult<Response> {
    let file = tokio::fs::File::open(&absolute).await.map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read the on-disk book file.",
        )
    })?;
    let len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(file));
    let mut response = (
        [
            (header::CONTENT_TYPE, mime),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        body,
    )
        .into_response();
    // Clients need a length for a determinate download progress bar.
    if len > 0 {
        response
            .headers_mut()
            .insert(header::CONTENT_LENGTH, header::HeaderValue::from(len));
    }
    Ok(response)
}

/// SQLite cannot ALTER a CHECK; rebuild epub_files when comic formats are missing.
fn migrate_epub_files_formats(db: &Connection) -> rusqlite::Result<()> {
    let sql: String = db.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='epub_files'",
        [],
        |r| r.get(0),
    )?;
    if sql.contains("'cb7'") {
        let _ = db.execute_batch("ALTER TABLE epub_files ADD COLUMN source BLOB");
        let _ = db.execute_batch("ALTER TABLE epub_files ADD COLUMN storage_path TEXT");
        let _ = db.execute_batch("ALTER TABLE epub_files ADD COLUMN source_path TEXT");
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
           format TEXT NOT NULL DEFAULT 'epub' CHECK(format IN ('epub','pdf','cbz','cbr','cb7','djvu')),
           source BLOB,
           storage_path TEXT,
           source_path TEXT
         );
         INSERT INTO epub_files_new
           (book_id,digest,content,cover,cover_mime,position,percentage,format)
           SELECT book_id,digest,content,cover,cover_mime,position,percentage,format
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
    // Backfill finished from progress for libraries that only had % / pages.
    let _ = db.execute_batch(
        "UPDATE books SET reading_status='finished'
         WHERE reading_status='open' AND (
           pages_read=total_pages
           OR id IN (SELECT book_id FROM epub_files WHERE percentage=1)
         )",
    );
    migrate_epub_files_formats(&db)?;
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
        .route("/api/v1/books/{id}", axum::routing::delete(epub::delete_book))
        .route("/api/v1/books/{id}/progress", post(update_progress))
        .route("/api/v1/books/{id}/finished", post(mark_finished))
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
        .with_state(AppState {
            data_dir: database
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_path_buf(),
            database,
            login_attempts: Arc::new(Mutex::new(VecDeque::new())),
        }))
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
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewBook {
    title: String,
    author: String,
    total_pages: i64,
}

async fn books(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Vec<Book>>> {
    state.db(move |db| {
        let user = authenticate(db, &headers)?;
        let mut query = db.prepare("SELECT b.id,b.title,b.author,CASE WHEN f.book_id IS NULL THEN b.total_pages END,b.pages_read,b.reading_status,f.book_id,f.percentage,f.position,f.cover IS NOT NULL,f.format,f.digest FROM books b LEFT JOIN epub_files f ON f.book_id=b.id WHERE b.user_id=? ORDER BY b.id DESC")?;
        let books = query.query_map([user], |r| {
            let imported = r.get::<_, Option<i64>>(6)?.is_some();
            let format = r.get::<_, Option<String>>(10)?;
            let digest = r.get::<_, Option<String>>(11)?;
            Ok(Book {
                id: r.get(0)?,
                title: r.get(1)?,
                author: r.get(2)?,
                total_pages: r.get(3)?,
                pages_read: r.get(4)?,
                reading_status: r.get(5)?,
                epub: if imported {
                    Some(EpubInfo {
                        format: match format.as_deref() {
                            Some("pdf") => "pdf",
                            Some("cbz") => "cbz",
                            Some("cbr") => "cbr",
                            Some("cb7") => "cb7",
                            Some("djvu") => "djvu",
                            _ => "epub",
                        },
                        digest: digest.unwrap_or_default(),
                        percentage: r.get(7)?,
                        position: r.get(8)?,
                        has_cover: r.get(9)?,
                    })
                } else {
                    None
                },
            })
        })?.collect::<Result<Vec<_>,_>>()?;
        Ok(Json(books))
    }).await
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

/// Set catalogue lifecycle status; finished also completes progress, reopen clears it.
fn set_reading_status(db: &Connection, user: i64, id: i64, status: &str) -> ApiResult<()> {
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
    Ok(())
}

async fn mark_finished(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            set_reading_status(db, user, id, "finished")?;
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
            set_reading_status(db, user, id, "unfinished")?;
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
            set_reading_status(db, user, id, "rereading")?;
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
        let values: (i64, i64, i64, i64, Option<f64>) = db.query_row(
            "SELECT count(*),
                    coalesce(sum(b.pages_read),0),
                    coalesce(sum(CASE WHEN b.reading_status='finished' THEN 1 ELSE 0 END),0),
                    coalesce(sum(CASE WHEN b.reading_status='rereading' OR (b.reading_status='open' AND CASE WHEN f.book_id IS NULL THEN b.pages_read>0 AND b.pages_read<b.total_pages ELSE f.position IS NOT NULL AND f.percentage<1 END) THEN 1 ELSE 0 END),0),
                    (SELECT avg(CASE WHEN f2.book_id IS NOT NULL THEN f2.percentage
                                     WHEN b2.total_pages>0 THEN cast(b2.pages_read as real)/b2.total_pages END)
                     FROM books b2 LEFT JOIN epub_files f2 ON f2.book_id=b2.id
                     WHERE b2.user_id=?1
                       AND CASE WHEN f2.book_id IS NOT NULL THEN f2.position IS NOT NULL ELSE b2.pages_read>0 END)
             FROM books b LEFT JOIN epub_files f ON f.book_id=b.id WHERE b.user_id=?1",
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
