//! Readwise highlight push (M12 slice 10).
//!
//! BO reference @ c77966cc: create-only highlight batches from annotations.
//! Auto-sync on annotation create stays deferred; Settings + sync-now ship here.

use super::*;
use serde_json::{json, Value};

const READWISE_HIGHLIGHTS_URL: &str = "https://readwise.io/api/v2/highlights/";
const READWISE_AUTH_URL: &str = "https://readwise.io/api/v2/auth/";
const READWISE_BATCH_SIZE: usize = 100;
const READWISE_SOURCE_TYPE: &str = "shelf";
const READWISE_CATEGORY: &str = "books";

const MAX_TEXT: usize = 8191;
const MAX_TITLE: usize = 511;
const MAX_AUTHOR: usize = 1024;
const MAX_NOTE: usize = 8191;
const MAX_URL: usize = 4095;

pub(crate) fn migrate(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS readwise_settings (
           user_id INTEGER PRIMARY KEY REFERENCES users(id),
           api_token TEXT NOT NULL DEFAULT '',
           enabled INTEGER NOT NULL DEFAULT 0,
           last_synced_annotation_id INTEGER NOT NULL DEFAULT 0,
           last_synced_at TEXT,
           updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         )",
    )?;
    Ok(())
}

fn stub_mode() -> bool {
    std::env::var("SHELF_READWISE_STUB").as_deref() == Ok("1")
}

fn strip_token(token: &str) -> String {
    let t = token.trim();
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("token ") {
        t[6..].trim().to_string()
    } else if lower.starts_with("bearer ") {
        t[7..].trim().to_string()
    } else {
        t.to_string()
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        value.chars().take(max).collect()
    }
}

#[derive(Clone, Debug)]
struct ReadwiseSettings {
    api_token: String,
    enabled: bool,
    last_synced_annotation_id: i64,
    last_synced_at: Option<String>,
}

fn ensure_settings_row(db: &Connection, user: i64) -> ApiResult<()> {
    db.execute(
        "INSERT OR IGNORE INTO readwise_settings(user_id) VALUES (?)",
        params![user],
    )?;
    Ok(())
}

fn load_settings(db: &Connection, user: i64) -> ApiResult<ReadwiseSettings> {
    ensure_settings_row(db, user)?;
    db.query_row(
        "SELECT api_token,enabled,last_synced_annotation_id,last_synced_at
         FROM readwise_settings WHERE user_id=?",
        params![user],
        |r| {
            Ok(ReadwiseSettings {
                api_token: r.get(0)?,
                enabled: r.get::<_, i64>(1)? != 0,
                last_synced_annotation_id: r.get(2)?,
                last_synced_at: r.get(3)?,
            })
        },
    )
    .map_err(ApiError::from)
}

fn settings_json(s: &ReadwiseSettings) -> Value {
    let token_configured = !s.api_token.is_empty();
    let disabled_reason = if !token_configured {
        Some("missing_token")
    } else if !s.enabled {
        Some("user_disabled")
    } else {
        None
    };
    json!({
        "tokenConfigured": token_configured,
        "enabled": s.enabled,
        "effectiveEnabled": token_configured && s.enabled,
        "disabledReason": disabled_reason,
        "lastSyncedAnnotationId": s.last_synced_annotation_id,
        "lastSyncedAt": s.last_synced_at,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReadwiseSettingsBody {
    #[serde(default)]
    api_token: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReadwiseValidateBody {
    #[serde(default)]
    api_token: Option<String>,
}

#[derive(Clone, Debug)]
struct HighlightRow {
    annotation_id: i64,
    book_id: i64,
    text: String,
    note: Option<String>,
    title: String,
    author: String,
    isbn10: Option<String>,
    isbn13: Option<String>,
    created_at: String,
}

fn load_new_highlights(
    db: &Connection,
    user: i64,
    after_id: i64,
    limit: usize,
) -> ApiResult<Vec<HighlightRow>> {
    let mut stmt = db.prepare(
        "SELECT a.id, a.book_id, coalesce(a.text,''), a.note,
                b.title, coalesce(b.author,''),
                m.isbn10, m.isbn13, a.created_at
         FROM annotations a
         JOIN books b ON b.id=a.book_id AND b.user_id=a.user_id
         LEFT JOIN book_metadata m ON m.book_id=b.id
         WHERE a.user_id=? AND a.id>? AND length(trim(coalesce(a.text,'')))>0
           AND b.merged_into IS NULL
         ORDER BY a.id
         LIMIT ?",
    )?;
    let rows = stmt.query_map(params![user, after_id, limit as i64], |r| {
        Ok(HighlightRow {
            annotation_id: r.get(0)?,
            book_id: r.get(1)?,
            text: r.get(2)?,
            note: r.get(3)?,
            title: r.get(4)?,
            author: r.get(5)?,
            isbn10: r.get(6)?,
            isbn13: r.get(7)?,
            created_at: r.get(8)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn to_payload(row: &HighlightRow) -> Value {
    let mut obj = json!({
        "text": truncate(&row.text, MAX_TEXT),
        "highlighted_at": row.created_at,
        "source_type": READWISE_SOURCE_TYPE,
        "category": READWISE_CATEGORY,
        "highlight_url": truncate(
            &format!("/library?book={}&annotation={}", row.book_id, row.annotation_id),
            MAX_URL,
        ),
    });
    if !row.title.trim().is_empty() {
        obj["title"] = json!(truncate(&row.title, MAX_TITLE));
    }
    if !row.author.trim().is_empty() {
        obj["author"] = json!(truncate(&row.author, MAX_AUTHOR));
    }
    if let Some(note) = row.note.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        obj["note"] = json!(truncate(note, MAX_NOTE));
    }
    let isbn = row
        .isbn13
        .as_deref()
        .or(row.isbn10.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(isbn) = isbn {
        obj["image_url"] = json!(format!(
            "https://covers.openlibrary.org/b/isbn/{isbn}-L.jpg?default=false"
        ));
    }
    obj
}

async fn auth_check(token: &str) -> Result<(), String> {
    if stub_mode() {
        return Ok(());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client
        .get(READWISE_AUTH_URL)
        .header("Authorization", format!("Token {token}"))
        .header(
            "User-Agent",
            "Shelf Readwise Sync (https://github.com/rjyates/Shelf)",
        )
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("unauthorized".into());
    }
    if !res.status().is_success() {
        return Err(format!("Readwise auth error: {}", res.status()));
    }
    Ok(())
}

async fn push_highlights(token: &str, highlights: &[Value]) -> Result<(), String> {
    if highlights.is_empty() {
        return Ok(());
    }
    if stub_mode() {
        return Ok(());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client
        .post(READWISE_HIGHLIGHTS_URL)
        .header("Authorization", format!("Token {token}"))
        .header("Content-Type", "application/json")
        .header(
            "User-Agent",
            "Shelf Readwise Sync (https://github.com/rjyates/Shelf)",
        )
        .json(&json!({ "highlights": highlights }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("unauthorized".into());
    }
    if !res.status().is_success() {
        return Err(format!("Readwise API error: {}", res.status()));
    }
    Ok(())
}

pub(super) async fn get_readwise_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let settings = load_settings(db, user)?;
            Ok(Json(settings_json(&settings)))
        })
        .await
}

pub(super) async fn put_readwise_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ReadwiseSettingsBody>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let mut settings = load_settings(db, user)?;
            if let Some(token) = body.api_token {
                let token = strip_token(&token);
                if token.is_empty() && settings.api_token.is_empty() {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "API token is required to connect Readwise.",
                    ));
                }
                if !token.is_empty() {
                    // shortcut: token stored plaintext in SQLite (single-user);
                    // encrypt at rest when outbound secrets get a shared vault.
                    settings.api_token = token;
                }
            } else if settings.api_token.is_empty() {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "API token is required to connect Readwise.",
                ));
            }
            if let Some(v) = body.enabled {
                settings.enabled = v;
            }
            db.execute(
                "UPDATE readwise_settings SET
                   api_token=?, enabled=?,
                   updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE user_id=?",
                params![
                    settings.api_token,
                    if settings.enabled { 1 } else { 0 },
                    user,
                ],
            )?;
            Ok(Json(settings_json(&settings)))
        })
        .await
}

pub(super) async fn delete_readwise_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let n = db.execute(
                "DELETE FROM readwise_settings WHERE user_id=?",
                params![user],
            )?;
            if n == 0 {
                return Err(ApiError(
                    StatusCode::NOT_FOUND,
                    "Readwise integration not configured.",
                ));
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn validate_readwise_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ReadwiseValidateBody>,
) -> ApiResult<Json<Value>> {
    let token = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                let settings = load_settings(db, user)?;
                let token = body
                    .api_token
                    .as_deref()
                    .map(strip_token)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(settings.api_token);
                if token.is_empty() {
                    return Err(ApiError(StatusCode::BAD_REQUEST, "API token is required."));
                }
                Ok(token)
            }
        })
        .await?;
    match auth_check(&token).await {
        Ok(()) => Ok(Json(json!({ "valid": true }))),
        Err(_) => Ok(Json(json!({ "valid": false }))),
    }
}

pub(super) async fn sync_readwise(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let (token, mut watermark) = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                let settings = load_settings(db, user)?;
                if settings.api_token.is_empty() || !settings.enabled {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "Enable Readwise and save an API token first.",
                    ));
                }
                Ok((settings.api_token, settings.last_synced_annotation_id))
            }
        })
        .await?;

    let mut synced = 0i64;
    let mut batches = 0i64;
    loop {
        let rows = state
            .db({
                let headers = headers.clone();
                let after = watermark;
                move |db| {
                    let user = authenticate(db, &headers)?;
                    load_new_highlights(db, user, after, READWISE_BATCH_SIZE)
                }
            })
            .await?;
        if rows.is_empty() {
            break;
        }
        let payload: Vec<Value> = rows.iter().map(to_payload).collect();
        if let Err(err) = push_highlights(&token, &payload).await {
            if err == "unauthorized" {
                let _ = state
                    .db({
                        let headers = headers.clone();
                        move |db| {
                            let user = authenticate(db, &headers)?;
                            db.execute(
                                "UPDATE readwise_settings SET enabled=0,
                                   updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
                                 WHERE user_id=?",
                                params![user],
                            )?;
                            Ok(())
                        }
                    })
                    .await;
                return Err(ApiError(
                    StatusCode::UNAUTHORIZED,
                    "Readwise rejected the API token; sync disabled.",
                ));
            }
            return Err(ApiError(
                StatusCode::BAD_GATEWAY,
                "Readwise highlight push failed.",
            ));
        }
        watermark = rows.last().map(|r| r.annotation_id).unwrap_or(watermark);
        let mark = watermark;
        let _ = state
            .db({
                let headers = headers.clone();
                move |db| {
                    let user = authenticate(db, &headers)?;
                    db.execute(
                        "UPDATE readwise_settings SET
                           last_synced_annotation_id=?,
                           last_synced_at=strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                           updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
                         WHERE user_id=?",
                        params![mark, user],
                    )?;
                    Ok(())
                }
            })
            .await;
        synced += rows.len() as i64;
        batches += 1;
        if rows.len() < READWISE_BATCH_SIZE {
            break;
        }
    }

    Ok(Json(json!({
        "status": "completed",
        "syncedHighlights": synced,
        "batches": batches,
        "lastSyncedAnnotationId": watermark,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_respects_char_count() {
        assert_eq!(truncate("hi", 2), "hi");
        assert_eq!(truncate("hello", 3), "hel");
    }

    #[test]
    fn strip_token_prefixes() {
        assert_eq!(strip_token("Token abc"), "abc");
        assert_eq!(strip_token("Bearer abc"), "abc");
        assert_eq!(strip_token("abc"), "abc");
    }
}
