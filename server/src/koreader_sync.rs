//! KOReader catalog + annotation sync (M12 slice 12).
//!
//! BO reference @ c77966cc: plugin match-check, hash links, annotation upload/
//! exchange, bulk progress. Shelf ships credentials + hash-link catalog subset
//! + annotation exchange lite.
//!
//! Full catalog sections, bookmark exchange, plugin package zip, and device
//! clock-frame minting stay deferred.

use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

const MD5_HEX_LEN: usize = 32;

pub(crate) fn migrate(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS koreader_credentials (
           user_id INTEGER PRIMARY KEY REFERENCES users(id),
           username TEXT NOT NULL DEFAULT '' COLLATE NOCASE,
           password_hash TEXT NOT NULL DEFAULT '',
           sync_enabled INTEGER NOT NULL DEFAULT 1,
           hide_audio_only INTEGER NOT NULL DEFAULT 0,
           last_sync_at TEXT,
           updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         );
         CREATE TABLE IF NOT EXISTS koreader_hash_links (
           user_id INTEGER NOT NULL REFERENCES users(id),
           hash TEXT NOT NULL COLLATE NOCASE,
           book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
           position_compatible INTEGER NOT NULL DEFAULT 1,
           created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
           updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
           PRIMARY KEY (user_id, hash)
         );
         CREATE INDEX IF NOT EXISTS koreader_hash_links_book
           ON koreader_hash_links(user_id, book_id);
         CREATE TABLE IF NOT EXISTS koreader_unmatched (
           user_id INTEGER NOT NULL REFERENCES users(id),
           hash TEXT NOT NULL COLLATE NOCASE,
           title TEXT,
           authors TEXT,
           last_open INTEGER,
           first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
           last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
           dismissed INTEGER NOT NULL DEFAULT 0,
           PRIMARY KEY (user_id, hash)
         );
         CREATE TABLE IF NOT EXISTS koreader_devices (
           user_id INTEGER NOT NULL REFERENCES users(id),
           device_id TEXT NOT NULL,
           device_model TEXT NOT NULL DEFAULT '',
           plugin_version TEXT NOT NULL DEFAULT '',
           last_sync_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
           PRIMARY KEY (user_id, device_id)
         );
         CREATE TABLE IF NOT EXISTS koreader_annotation_keys (
           user_id INTEGER NOT NULL REFERENCES users(id),
           book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
           hash TEXT NOT NULL COLLATE NOCASE,
           datetime TEXT NOT NULL,
           annotation_id INTEGER NOT NULL REFERENCES annotations(id) ON DELETE CASCADE,
           device_id TEXT,
           PRIMARY KEY (user_id, book_id, datetime)
         )",
    )?;
    Ok(())
}

fn is_md5_hex(s: &str) -> bool {
    s.len() == MD5_HEX_LEN && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn normalize_hash(s: &str) -> Option<String> {
    let t = s.trim().to_ascii_lowercase();
    if is_md5_hex(&t) {
        Some(t)
    } else {
        None
    }
}

fn drawer_to_style(drawer: &str) -> &'static str {
    match drawer {
        "underscore" => "underline",
        "strikeout" => "strikethrough",
        "invert" => "invert",
        _ => "highlight",
    }
}

fn ensure_credentials_row(db: &Connection, user: i64) -> ApiResult<()> {
    db.execute(
        "INSERT OR IGNORE INTO koreader_credentials(user_id) VALUES (?)",
        params![user],
    )?;
    Ok(())
}

struct KoreaderCreds {
    username: String,
    password_hash: String,
    sync_enabled: bool,
    hide_audio_only: bool,
    last_sync_at: Option<String>,
}

fn load_creds(db: &Connection, user: i64) -> ApiResult<KoreaderCreds> {
    ensure_credentials_row(db, user)?;
    db.query_row(
        "SELECT username, password_hash, sync_enabled, hide_audio_only, last_sync_at
         FROM koreader_credentials WHERE user_id=?",
        params![user],
        |r| {
            Ok(KoreaderCreds {
                username: r.get(0)?,
                password_hash: r.get(1)?,
                sync_enabled: r.get::<_, i64>(2)? != 0,
                hide_audio_only: r.get::<_, i64>(3)? != 0,
                last_sync_at: r.get(4)?,
            })
        },
    )
    .map_err(ApiError::from)
}

fn creds_configured(c: &KoreaderCreds) -> bool {
    !c.username.trim().is_empty() && !c.password_hash.is_empty()
}

fn settings_json(c: &KoreaderCreds) -> Value {
    let configured = creds_configured(c);
    json!({
        "configured": configured,
        "username": if configured { Value::String(c.username.clone()) } else { Value::Null },
        "syncEnabled": c.sync_enabled,
        "hideAudioOnlyBooks": c.hide_audio_only,
        "lastSyncAt": c.last_sync_at,
        "effectiveEnabled": configured && c.sync_enabled,
    })
}

fn touch_device(
    db: &Connection,
    user: i64,
    device_id: &str,
    device_model: &str,
    plugin_version: &str,
) -> ApiResult<()> {
    if device_id.is_empty() || device_id.len() > 128 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid device id."));
    }
    db.execute(
        "INSERT INTO koreader_devices(user_id, device_id, device_model, plugin_version)
         VALUES (?,?,?,?)
         ON CONFLICT(user_id, device_id) DO UPDATE SET
           device_model=excluded.device_model,
           plugin_version=excluded.plugin_version,
           last_sync_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')",
        params![user, device_id, device_model, plugin_version],
    )?;
    db.execute(
        "UPDATE koreader_credentials SET last_sync_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE user_id=?",
        params![user],
    )?;
    Ok(())
}

fn resolve_hash_book(db: &Connection, user: i64, hash: &str) -> ApiResult<Option<i64>> {
    Ok(db
        .query_row(
            "SELECT book_id FROM koreader_hash_links WHERE user_id=? AND hash=? COLLATE NOCASE",
            params![user, hash],
            |r| r.get(0),
        )
        .optional()?)
}

fn owned_book(db: &Connection, user: i64, book_id: i64) -> ApiResult<()> {
    let ok: bool = db
        .query_row(
            "SELECT 1 FROM books WHERE id=? AND user_id=? AND merged_into IS NULL",
            params![book_id, user],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, "Book not found."))
    }
}

fn upsert_hash_link(db: &Connection, user: i64, hash: &str, book_id: i64) -> ApiResult<()> {
    owned_book(db, user, book_id)?;
    db.execute(
        "INSERT INTO koreader_hash_links(user_id, hash, book_id)
         VALUES (?,?,?)
         ON CONFLICT(user_id, hash) DO UPDATE SET
           book_id=excluded.book_id,
           updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')",
        params![user, hash, book_id],
    )?;
    db.execute(
        "DELETE FROM koreader_unmatched WHERE user_id=? AND hash=? COLLATE NOCASE",
        params![user, hash],
    )?;
    Ok(())
}

fn record_unmatched(
    db: &Connection,
    user: i64,
    hash: &str,
    title: Option<&str>,
    authors: Option<&str>,
    last_open: Option<i64>,
) -> ApiResult<()> {
    db.execute(
        "INSERT INTO koreader_unmatched(user_id, hash, title, authors, last_open)
         VALUES (?,?,?,?,?)
         ON CONFLICT(user_id, hash) DO UPDATE SET
           title=COALESCE(excluded.title, koreader_unmatched.title),
           authors=COALESCE(excluded.authors, koreader_unmatched.authors),
           last_open=COALESCE(excluded.last_open, koreader_unmatched.last_open),
           last_seen_at=strftime('%Y-%m-%dT%H:%M:%SZ','now'),
           dismissed=0",
        params![user, hash, title, authors, last_open],
    )?;
    Ok(())
}

/// Plugin auth: Shelf Bearer, or Basic against koreader_credentials.
fn authenticate_plugin(db: &Connection, headers: &HeaderMap) -> ApiResult<i64> {
    if let Ok(user) = authenticate(db, headers) {
        let creds = load_creds(db, user)?;
        if !creds.sync_enabled && creds_configured(&creds) {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "KOReader sync is disabled.",
            ));
        }
        return Ok(user);
    }
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError(
            StatusCode::UNAUTHORIZED,
            "Authentication required",
        ))?;
    let encoded = auth.strip_prefix("Basic ").ok_or(ApiError(
        StatusCode::UNAUTHORIZED,
        "Authentication required",
    ))?;
    let decoded = STANDARD
        .decode(encoded.trim())
        .map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "Invalid credentials"))?;
    let pair = String::from_utf8(decoded)
        .map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "Invalid credentials"))?;
    let (username, password) = pair
        .split_once(':')
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "Invalid credentials"))?;
    if username.is_empty() || username.len() > 100 || password.len() > 200 {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "Invalid credentials"));
    }
    let row: Option<(i64, String, i64)> = db
        .query_row(
            "SELECT user_id, password_hash, sync_enabled FROM koreader_credentials
             WHERE username=? COLLATE NOCASE AND password_hash != ''",
            params![username.trim()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((user_id, hash, sync_enabled)) = row else {
        let dummy_salt = SaltString::encode_b64(b"shelf-koreader-pad")
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Authentication failed."))?;
        let dummy = Argon2::default()
            .hash_password(b"not-a-koreader-user", &dummy_salt)
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Authentication failed."))?
            .to_string();
        let parsed = PasswordHash::new(&dummy)
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Authentication failed."))?;
        let _ = Argon2::default().verify_password(password.as_bytes(), &parsed);
        return Err(ApiError(StatusCode::UNAUTHORIZED, "Invalid credentials"));
    };
    let parsed = PasswordHash::new(&hash)
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Authentication failed."))?;
    if Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_err()
    {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "Invalid credentials"));
    }
    if sync_enabled == 0 {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "KOReader sync is disabled.",
        ));
    }
    Ok(user_id)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KoreaderSettingsBody {
    username: Option<String>,
    password: Option<String>,
    sync_enabled: Option<bool>,
    hide_audio_only_books: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LinkUnmatchedBody {
    book_id: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MatchCheckBook {
    hash: String,
    title: Option<String>,
    authors: Option<String>,
    last_open: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MatchCheckBody {
    device_id: String,
    #[serde(default)]
    device_model: Option<String>,
    #[serde(default)]
    plugin_version: Option<String>,
    hashes: Vec<String>,
    #[serde(default)]
    books: Option<Vec<MatchCheckBook>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KoreaderAnnotationIn {
    datetime: String,
    drawer: Option<String>,
    color: Option<String>,
    text: Option<String>,
    note: Option<String>,
    chapter: Option<String>,
    pageno: Option<i64>,
    pos_format: Option<String>,
    pos0: String,
    pos1: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnnotationsBookIn {
    hash: String,
    annotations: Vec<KoreaderAnnotationIn>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnnotationsUploadBody {
    device_id: String,
    #[serde(default)]
    device_model: Option<String>,
    #[serde(default)]
    plugin_version: Option<String>,
    books: Vec<AnnotationsBookIn>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExchangeKey {
    #[allow(dead_code)]
    k: String,
    dt: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExchangeBookIn {
    hash: String,
    keys: Vec<ExchangeKey>,
    keys_complete: bool,
    #[serde(default)]
    changes: Vec<KoreaderAnnotationIn>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnnotationExchangeBody {
    device_id: String,
    #[serde(default)]
    device_model: Option<String>,
    #[serde(default)]
    plugin_version: Option<String>,
    books: Vec<ExchangeBookIn>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgressItem {
    hash: String,
    percentage: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BulkProgressBody {
    device_id: String,
    #[serde(default)]
    device_model: Option<String>,
    #[serde(default)]
    plugin_version: Option<String>,
    items: Vec<ProgressItem>,
}

fn upsert_annotation_from_device(
    db: &Connection,
    user: i64,
    book_id: i64,
    hash: &str,
    device_id: &str,
    ann: &KoreaderAnnotationIn,
) -> ApiResult<i64> {
    if ann.datetime.len() > 32 || ann.pos0.is_empty() || ann.pos0.len() > 4000 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid annotation."));
    }
    let text = ann.text.clone().unwrap_or_default();
    if text.len() > 16_384 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid annotation."));
    }
    let color = ann
        .color
        .clone()
        .unwrap_or_else(|| "#FACC15".into())
        .chars()
        .take(64)
        .collect::<String>();
    let style = drawer_to_style(ann.drawer.as_deref().unwrap_or("lighten")).to_string();
    let note = ann.note.clone();
    let chapter = ann.chapter.clone();
    // shortcut: store KOReader xpointer/pdf pos in cfi; CFI conversion deferred
    let cfi = if ann.pos_format.as_deref() == Some("pdf") {
        format!("koreader:pdf:{}:{}", ann.pageno.unwrap_or(0), ann.pos0)
    } else {
        format!("koreader:xp:{}", ann.pos0)
    };

    if let Some(existing) = db
        .query_row(
            "SELECT annotation_id FROM koreader_annotation_keys
             WHERE user_id=? AND book_id=? AND datetime=?",
            params![user, book_id, ann.datetime],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    {
        db.execute(
            "UPDATE annotations SET cfi=?, text=?, color=?, style=?, note=?, chapter_title=?
             WHERE id=? AND user_id=? AND book_id=?",
            params![cfi, text, color, style, note, chapter, existing, user, book_id],
        )?;
        return Ok(existing);
    }

    db.execute(
        "INSERT INTO annotations(user_id,book_id,cfi,text,color,style,note,chapter_title)
         VALUES (?,?,?,?,?,?,?,?)",
        params![user, book_id, cfi, text, color, style, note, chapter],
    )?;
    let id = db.last_insert_rowid();
    db.execute(
        "INSERT INTO koreader_annotation_keys(user_id, book_id, hash, datetime, annotation_id, device_id)
         VALUES (?,?,?,?,?,?)",
        params![user, book_id, hash, ann.datetime, id, device_id],
    )?;
    let _ = ann.pos1;
    Ok(id)
}

pub(super) async fn get_koreader_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let creds = load_creds(db, user)?;
            let linked: i64 = db.query_row(
                "SELECT COUNT(*) FROM koreader_hash_links WHERE user_id=?",
                params![user],
                |r| r.get(0),
            )?;
            let unmatched: i64 = db.query_row(
                "SELECT COUNT(*) FROM koreader_unmatched WHERE user_id=? AND dismissed=0",
                params![user],
                |r| r.get(0),
            )?;
            let mut out = settings_json(&creds);
            if let Some(obj) = out.as_object_mut() {
                obj.insert("linkedHashes".into(), json!(linked));
                obj.insert("unmatchedHashes".into(), json!(unmatched));
            }
            Ok(Json(out))
        })
        .await
}

pub(super) async fn put_koreader_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<KoreaderSettingsBody>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            ensure_credentials_row(db, user)?;
            let mut creds = load_creds(db, user)?;
            if let Some(username) = body.username {
                let u = username.trim().to_string();
                if u.len() > 100 {
                    return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid username."));
                }
                if !u.is_empty() {
                    let taken: bool = db
                        .query_row(
                            "SELECT 1 FROM koreader_credentials
                             WHERE username=? COLLATE NOCASE AND user_id!=?",
                            params![u, user],
                            |_| Ok(true),
                        )
                        .optional()?
                        .unwrap_or(false);
                    if taken {
                        return Err(ApiError(
                            StatusCode::CONFLICT,
                            "KOReader username already in use.",
                        ));
                    }
                }
                creds.username = u;
            }
            if let Some(password) = body.password {
                if password.len() < 8 || password.len() > 200 {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "Password must be 8–200 characters.",
                    ));
                }
                let salt = SaltString::generate(&mut OsRng);
                creds.password_hash = Argon2::default()
                    .hash_password(password.as_bytes(), &salt)
                    .map_err(|_| {
                        ApiError(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Could not save credentials.",
                        )
                    })?
                    .to_string();
            }
            if let Some(v) = body.sync_enabled {
                creds.sync_enabled = v;
            }
            if let Some(v) = body.hide_audio_only_books {
                creds.hide_audio_only = v;
            }
            if !creds.username.is_empty() && creds.password_hash.is_empty() {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Password is required when setting a username.",
                ));
            }
            db.execute(
                "UPDATE koreader_credentials
                 SET username=?, password_hash=?, sync_enabled=?, hide_audio_only=?,
                     updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE user_id=?",
                params![
                    creds.username,
                    creds.password_hash,
                    if creds.sync_enabled { 1 } else { 0 },
                    if creds.hide_audio_only { 1 } else { 0 },
                    user
                ],
            )?;
            let creds = load_creds(db, user)?;
            Ok(Json(settings_json(&creds)))
        })
        .await
}

pub(super) async fn delete_koreader_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            db.execute(
                "DELETE FROM koreader_annotation_keys WHERE user_id=?",
                params![user],
            )?;
            db.execute(
                "DELETE FROM koreader_hash_links WHERE user_id=?",
                params![user],
            )?;
            db.execute(
                "DELETE FROM koreader_unmatched WHERE user_id=?",
                params![user],
            )?;
            db.execute(
                "DELETE FROM koreader_devices WHERE user_id=?",
                params![user],
            )?;
            let n = db.execute(
                "DELETE FROM koreader_credentials WHERE user_id=?",
                params![user],
            )?;
            if n == 0 {
                return Err(ApiError(StatusCode::NOT_FOUND, "KOReader not configured."));
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn list_unmatched(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let mut stmt = db.prepare(
                "SELECT hash, title, authors, last_open, first_seen_at, last_seen_at
                 FROM koreader_unmatched
                 WHERE user_id=? AND dismissed=0
                 ORDER BY last_seen_at DESC LIMIT 200",
            )?;
            let rows = stmt
                .query_map(params![user], |r| {
                    Ok(json!({
                        "hash": r.get::<_, String>(0)?,
                        "title": r.get::<_, Option<String>>(1)?,
                        "authors": r.get::<_, Option<String>>(2)?,
                        "lastOpen": r.get::<_, Option<i64>>(3)?,
                        "firstSeenAt": r.get::<_, String>(4)?,
                        "lastSeenAt": r.get::<_, String>(5)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Json(json!({ "items": rows })))
        })
        .await
}

pub(super) async fn link_unmatched(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(hash): Path<String>,
    Json(body): Json<LinkUnmatchedBody>,
) -> ApiResult<Json<Value>> {
    let hash =
        normalize_hash(&hash).ok_or(ApiError(StatusCode::BAD_REQUEST, "Invalid KOReader hash."))?;
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            upsert_hash_link(db, user, &hash, body.book_id)?;
            Ok(Json(json!({
                "hash": hash,
                "bookId": body.book_id,
            })))
        })
        .await
}

pub(super) async fn dismiss_unmatched(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(hash): Path<String>,
) -> ApiResult<Json<Value>> {
    let hash =
        normalize_hash(&hash).ok_or(ApiError(StatusCode::BAD_REQUEST, "Invalid KOReader hash."))?;
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let n = db.execute(
                "UPDATE koreader_unmatched SET dismissed=1 WHERE user_id=? AND hash=? COLLATE NOCASE",
                params![user, hash],
            )?;
            if n == 0 {
                return Err(ApiError(
                    StatusCode::NOT_FOUND,
                    "Unmatched hash not found.",
                ));
            }
            Ok(Json(json!({ "hash": hash })))
        })
        .await
}

pub(super) async fn list_hash_links(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let mut stmt = db.prepare(
                "SELECT h.hash, h.book_id, b.title, coalesce(b.author,''), h.position_compatible, h.updated_at
                 FROM koreader_hash_links h
                 JOIN books b ON b.id=h.book_id
                 WHERE h.user_id=?
                 ORDER BY h.updated_at DESC LIMIT 500",
            )?;
            let rows = stmt
                .query_map(params![user], |r| {
                    Ok(json!({
                        "hash": r.get::<_, String>(0)?,
                        "bookId": r.get::<_, i64>(1)?,
                        "bookTitle": r.get::<_, String>(2)?,
                        "bookAuthors": r.get::<_, String>(3)?,
                        "positionCompatible": r.get::<_, i64>(4)? != 0,
                        "updatedAt": r.get::<_, String>(5)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Json(json!({ "items": rows })))
        })
        .await
}

pub(super) async fn unlink_hash(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(hash): Path<String>,
) -> ApiResult<StatusCode> {
    let hash =
        normalize_hash(&hash).ok_or(ApiError(StatusCode::BAD_REQUEST, "Invalid KOReader hash."))?;
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let n = db.execute(
                "DELETE FROM koreader_hash_links WHERE user_id=? AND hash=? COLLATE NOCASE",
                params![user, hash],
            )?;
            if n == 0 {
                return Err(ApiError(StatusCode::NOT_FOUND, "Hash link not found."));
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn plugin_match_check(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MatchCheckBody>,
) -> ApiResult<Json<Value>> {
    if body.hashes.is_empty() || body.hashes.len() > 500 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid hash list."));
    }
    state
        .db(move |db| {
            let user = authenticate_plugin(db, &headers)?;
            touch_device(
                db,
                user,
                &body.device_id,
                body.device_model.as_deref().unwrap_or(""),
                body.plugin_version.as_deref().unwrap_or(""),
            )?;
            let meta: std::collections::HashMap<String, MatchCheckBook> = body
                .books
                .unwrap_or_default()
                .into_iter()
                .filter_map(|b| normalize_hash(&b.hash).map(|h| (h, b)))
                .collect();
            let mut matched = Vec::new();
            let mut unmatched = Vec::new();
            for raw in &body.hashes {
                let Some(hash) = normalize_hash(raw) else {
                    continue;
                };
                if let Some(book_id) = resolve_hash_book(db, user, &hash)? {
                    matched.push(json!({
                        "hash": hash,
                        "bookId": book_id,
                        "matched": true,
                    }));
                } else {
                    let info = meta.get(&hash);
                    record_unmatched(
                        db,
                        user,
                        &hash,
                        info.and_then(|b| b.title.as_deref()),
                        info.and_then(|b| b.authors.as_deref()),
                        info.and_then(|b| b.last_open),
                    )?;
                    unmatched.push(json!({
                        "hash": hash,
                        "matched": false,
                    }));
                }
            }
            Ok(Json(json!({
                "matched": matched,
                "unmatched": unmatched,
                "matchedCount": matched.len(),
                "unmatchedCount": unmatched.len(),
            })))
        })
        .await
}

pub(super) async fn plugin_annotations_upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AnnotationsUploadBody>,
) -> ApiResult<Json<Value>> {
    if body.books.is_empty() || body.books.len() > 20 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid annotations payload.",
        ));
    }
    state
        .db(move |db| {
            let user = authenticate_plugin(db, &headers)?;
            touch_device(
                db,
                user,
                &body.device_id,
                body.device_model.as_deref().unwrap_or(""),
                body.plugin_version.as_deref().unwrap_or(""),
            )?;
            let mut upserted = 0i64;
            let mut skipped = 0i64;
            for book in &body.books {
                let Some(hash) = normalize_hash(&book.hash) else {
                    skipped += book.annotations.len() as i64;
                    continue;
                };
                let Some(book_id) = resolve_hash_book(db, user, &hash)? else {
                    skipped += book.annotations.len() as i64;
                    continue;
                };
                for ann in &book.annotations {
                    upsert_annotation_from_device(db, user, book_id, &hash, &body.device_id, ann)?;
                    upserted += 1;
                }
            }
            Ok(Json(json!({
                "upserted": upserted,
                "skipped": skipped,
            })))
        })
        .await
}

pub(super) async fn plugin_annotations_exchange(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AnnotationExchangeBody>,
) -> ApiResult<Json<Value>> {
    if body.books.is_empty() || body.books.len() > 20 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid exchange payload.",
        ));
    }
    state
        .db(move |db| {
            let user = authenticate_plugin(db, &headers)?;
            touch_device(
                db,
                user,
                &body.device_id,
                body.device_model.as_deref().unwrap_or(""),
                body.plugin_version.as_deref().unwrap_or(""),
            )?;
            let mut books_out = Vec::new();
            for book in &body.books {
                let Some(hash) = normalize_hash(&book.hash) else {
                    continue;
                };
                let Some(book_id) = resolve_hash_book(db, user, &hash)? else {
                    books_out.push(json!({
                        "hash": hash,
                        "skipped": "unmatched",
                        "add": [],
                        "delete": [],
                        "upserted": 0,
                    }));
                    continue;
                };
                let mut upserted = 0i64;
                for ann in &book.changes {
                    upsert_annotation_from_device(db, user, book_id, &hash, &body.device_id, ann)?;
                    upserted += 1;
                }
                let device_dts: std::collections::HashSet<String> =
                    book.keys.iter().map(|k| k.dt.clone()).collect();
                let mut add = Vec::new();
                let mut stmt = db.prepare(
                    "SELECT a.id, a.cfi, a.text, a.color, a.style, a.note, a.chapter_title,
                            k.datetime
                     FROM annotations a
                     LEFT JOIN koreader_annotation_keys k
                       ON k.annotation_id=a.id AND k.user_id=a.user_id
                     WHERE a.user_id=? AND a.book_id=?
                     ORDER BY a.id",
                )?;
                let rows = stmt
                    .query_map(params![user, book_id], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, Option<String>>(5)?,
                            r.get::<_, Option<String>>(6)?,
                            r.get::<_, Option<String>>(7)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                for (id, cfi, text, color, style, note, chapter, dt) in rows {
                    if let Some(ref d) = dt {
                        if device_dts.contains(d) {
                            continue;
                        }
                    }
                    let (pos_format, pos0) = match cfi.as_deref() {
                        Some(c) if c.starts_with("koreader:pdf:") => {
                            let rest = c.trim_start_matches("koreader:pdf:");
                            ("pdf", rest.to_string())
                        }
                        Some(c) if c.starts_with("koreader:xp:") => {
                            ("xpointer", c.trim_start_matches("koreader:xp:").to_string())
                        }
                        Some(c) => ("xpointer", c.to_string()),
                        None => continue,
                    };
                    let datetime = dt.unwrap_or_else(|| {
                        // Mint a stable device-frame key from server id when web-origin.
                        format!("1970-01-01 00:00:{:02}", id.min(59))
                    });
                    if device_dts.contains(&datetime) {
                        continue;
                    }
                    let drawer = match style.as_str() {
                        "underline" => "underscore",
                        "strikethrough" => "strikeout",
                        "invert" => "invert",
                        _ => "lighten",
                    };
                    add.push(json!({
                        "serverId": id,
                        "datetime": datetime,
                        "drawer": drawer,
                        "color": color,
                        "text": text,
                        "note": note,
                        "chapter": chapter,
                        "posFormat": pos_format,
                        "pos0": pos0,
                    }));
                }
                let mut delete = Vec::new();
                // shortcut: deletion only when keysComplete; full BO clock-skew handling deferred
                if book.keys_complete {
                    let mut stmt = db.prepare(
                        "SELECT annotation_id, datetime FROM koreader_annotation_keys
                         WHERE user_id=? AND book_id=? AND hash=? COLLATE NOCASE",
                    )?;
                    let keys = stmt
                        .query_map(params![user, book_id, hash], |r| {
                            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                        })?
                        .collect::<Result<Vec<_>, _>>()?;
                    for (ann_id, dt) in keys {
                        if !device_dts.contains(&dt) {
                            db.execute(
                                "DELETE FROM annotations WHERE id=? AND user_id=? AND book_id=?",
                                params![ann_id, user, book_id],
                            )?;
                            delete.push(json!({ "serverId": ann_id, "datetime": dt }));
                        }
                    }
                }
                books_out.push(json!({
                    "hash": hash,
                    "bookId": book_id,
                    "upserted": upserted,
                    "add": add,
                    "delete": delete,
                }));
            }
            Ok(Json(json!({ "books": books_out })))
        })
        .await
}

pub(super) async fn plugin_bulk_progress(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<BulkProgressBody>,
) -> ApiResult<Json<Value>> {
    if body.items.is_empty() || body.items.len() > 100 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid progress payload.",
        ));
    }
    state
        .db(move |db| {
            let user = authenticate_plugin(db, &headers)?;
            touch_device(
                db,
                user,
                &body.device_id,
                body.device_model.as_deref().unwrap_or(""),
                body.plugin_version.as_deref().unwrap_or(""),
            )?;
            let mut updated = 0i64;
            let mut skipped = 0i64;
            for item in &body.items {
                let Some(hash) = normalize_hash(&item.hash) else {
                    skipped += 1;
                    continue;
                };
                if !(0.0..=1.0).contains(&item.percentage) || !item.percentage.is_finite() {
                    skipped += 1;
                    continue;
                }
                let Some(book_id) = resolve_hash_book(db, user, &hash)? else {
                    skipped += 1;
                    continue;
                };
                session_injectors::apply_progress(
                    db,
                    user,
                    book_id,
                    item.percentage * 100.0,
                    "koreader",
                    Some(&body.device_id),
                )?;
                updated += 1;
            }
            Ok(Json(json!({
                "updated": updated,
                "skipped": skipped,
            })))
        })
        .await
}

/// Hash-link catalog subset: books the plugin can resolve via linked MD5 hashes.
pub(super) async fn plugin_catalog(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate_plugin(db, &headers)?;
            let hide_audio = load_creds(db, user)?.hide_audio_only;
            let mut stmt = db.prepare(
                "SELECT h.hash, b.id, b.title, coalesce(b.author,''), b.reading_status,
                        coalesce(f.percentage, 0), f.format,
                        CASE WHEN f.cover IS NOT NULL OR f.cover_mime IS NOT NULL THEN 1 ELSE 0 END
                 FROM koreader_hash_links h
                 JOIN books b ON b.id=h.book_id
                 LEFT JOIN epub_files f ON f.book_id=b.id
                 WHERE h.user_id=? AND b.merged_into IS NULL
                 ORDER BY b.title COLLATE NOCASE
                 LIMIT 2000",
            )?;
            let rows = stmt
                .query_map(params![user], |r| {
                    Ok(json!({
                        "hash": r.get::<_, String>(0)?,
                        "id": r.get::<_, i64>(1)?,
                        "title": r.get::<_, String>(2)?,
                        "authors": vec![r.get::<_, String>(3)?],
                        "readStatus": r.get::<_, String>(4)?,
                        "progressPercentage": r.get::<_, f64>(5)?,
                        "format": r.get::<_, Option<String>>(6)?,
                        "hasCover": r.get::<_, i64>(7)? != 0,
                        "detailUrl": format!("/api/v1/books/{}", r.get::<_, i64>(1)?),
                        "downloadUrl": format!("/api/v1/opds/{}/file", r.get::<_, i64>(1)?),
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let items: Vec<Value> = if hide_audio {
                rows.into_iter()
                    .filter(|row| {
                        let fmt = row
                            .get("format")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_ascii_lowercase();
                        !matches!(
                            fmt.as_str(),
                            "m4b" | "audiozip" | "aax" | "mp3" | "m4a" | "ogg" | "flac"
                        )
                    })
                    .collect()
            } else {
                rows
            };
            Ok(Json(json!({
                "section": "hash-links",
                "total": items.len(),
                "items": items,
            })))
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_hex_rejects_sha256() {
        assert!(normalize_hash("0123456789abcdef0123456789abcdef").is_some());
        assert!(
            normalize_hash("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
                .is_none()
        );
        assert!(normalize_hash("not-a-hash").is_none());
    }

    #[test]
    fn drawer_mapping() {
        assert_eq!(drawer_to_style("lighten"), "highlight");
        assert_eq!(drawer_to_style("underscore"), "underline");
        assert_eq!(drawer_to_style("strikeout"), "strikethrough");
    }
}
