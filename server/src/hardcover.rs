//! Hardcover account sync (M12 slice 9).
//!
//! BO reference @ c77966cc: settings + GraphQL match/sync of shelf status/progress.
//! Shelf ships settings, validate, sync-now (all or one book). Import-from-Hardcover
//! and queue/event auto-sync stay deferred.

use super::*;
use serde_json::{json, Value};

const HARDCOVER_GRAPHQL_URL: &str = "https://api.hardcover.app/v1/graphql";
const HARDCOVER_STATUS_WANT: i64 = 1;
const HARDCOVER_STATUS_READING: i64 = 2;
const HARDCOVER_STATUS_READ: i64 = 3;

const FIND_BY_ISBN13: &str = r#"
query FindBookByISBN13($isbn: String!) {
  books(where: { editions: { isbn_13: { _eq: $isbn } } }, limit: 1) {
    id
    editions(where: { isbn_13: { _eq: $isbn } }, limit: 1) { id }
  }
}"#;

const FIND_BY_ISBN10: &str = r#"
query FindBookByISBN10($isbn: String!) {
  books(where: { editions: { isbn_10: { _eq: $isbn } } }, limit: 1) {
    id
    editions(where: { isbn_10: { _eq: $isbn } }, limit: 1) { id }
  }
}"#;

const FIND_BY_ID: &str = r#"
query FindBookById($id: Int!) {
  books(where: { id: { _eq: $id } }, limit: 1) {
    id
    editions(limit: 1) { id }
  }
}"#;

const SEARCH_BOOKS: &str = r#"
query SearchBooks($query: String!) {
  search(query: $query, query_type: "Book", per_page: 5, page: 1) {
    error
    ids
  }
}"#;

const FIND_BOOKS_BY_IDS: &str = r#"
query FindBooksByIds($ids: [Int!]!) {
  books(where: { id: { _in: $ids } }, limit: 5) {
    id
    title
    cached_contributors
    editions(limit: 1) { id }
  }
}"#;

const ME_QUERY: &str = r#"query { me { username } }"#;

const FIND_USER_BOOK: &str = r#"
query FindUserBook($bookId: Int!) {
  me {
    user_books(where: { book_id: { _eq: $bookId } }, limit: 1) { id }
  }
}"#;

const INSERT_USER_BOOK: &str = r#"
mutation InsertUserBook($object: UserBookCreateInput!) {
  insert_user_book(object: $object) {
    user_book { id }
    error
  }
}"#;

const UPDATE_USER_BOOK: &str = r#"
mutation UpdateUserBook($id: Int!, $object: UserBookUpdateInput!) {
  update_user_book(id: $id, object: $object) {
    user_book { id }
    error
  }
}"#;

pub(crate) fn migrate(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS hardcover_settings (
           user_id INTEGER PRIMARY KEY REFERENCES users(id),
           api_token TEXT NOT NULL DEFAULT '',
           enabled INTEGER NOT NULL DEFAULT 0,
           book_sync_mode TEXT NOT NULL DEFAULT 'all_eligible',
           auto_sync_on_status_change INTEGER NOT NULL DEFAULT 1,
           auto_sync_on_progress_update INTEGER NOT NULL DEFAULT 1,
           auto_sync_on_rating_change INTEGER NOT NULL DEFAULT 1,
           privacy_setting_id INTEGER NOT NULL DEFAULT 3,
           last_synced_at TEXT,
           updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         );
         CREATE TABLE IF NOT EXISTS hardcover_book_state (
           user_id INTEGER NOT NULL REFERENCES users(id),
           book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
           hardcover_book_id INTEGER,
           hardcover_edition_id INTEGER,
           user_book_id INTEGER,
           match_method TEXT,
           match_error TEXT,
           sync_override TEXT,
           last_synced_at TEXT,
           sync_error TEXT,
           PRIMARY KEY (user_id, book_id)
         )",
    )?;
    Ok(())
}

fn stub_mode() -> bool {
    std::env::var("SHELF_HARDCOVER_STUB").as_deref() == Ok("1")
}

fn strip_bearer(token: &str) -> String {
    let t = token.trim();
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("bearer ") {
        t[7..].trim().to_string()
    } else {
        t.to_string()
    }
}

fn bare_isbn(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x')
        .collect::<String>()
        .to_ascii_uppercase()
}

#[derive(Clone, Debug)]
struct HardcoverSettings {
    api_token: String,
    enabled: bool,
    book_sync_mode: String,
    auto_sync_on_status_change: bool,
    auto_sync_on_progress_update: bool,
    auto_sync_on_rating_change: bool,
    privacy_setting_id: i64,
    last_synced_at: Option<String>,
}

fn ensure_settings_row(db: &Connection, user: i64) -> ApiResult<()> {
    db.execute(
        "INSERT OR IGNORE INTO hardcover_settings(user_id) VALUES (?)",
        params![user],
    )?;
    Ok(())
}

fn load_settings(db: &Connection, user: i64) -> ApiResult<HardcoverSettings> {
    ensure_settings_row(db, user)?;
    db.query_row(
        "SELECT api_token,enabled,book_sync_mode,
                auto_sync_on_status_change,auto_sync_on_progress_update,
                auto_sync_on_rating_change,privacy_setting_id,last_synced_at
         FROM hardcover_settings WHERE user_id=?",
        params![user],
        |r| {
            Ok(HardcoverSettings {
                api_token: r.get(0)?,
                enabled: r.get::<_, i64>(1)? != 0,
                book_sync_mode: r.get(2)?,
                auto_sync_on_status_change: r.get::<_, i64>(3)? != 0,
                auto_sync_on_progress_update: r.get::<_, i64>(4)? != 0,
                auto_sync_on_rating_change: r.get::<_, i64>(5)? != 0,
                privacy_setting_id: r.get(6)?,
                last_synced_at: r.get(7)?,
            })
        },
    )
    .map_err(ApiError::from)
}

fn settings_json(s: &HardcoverSettings) -> Value {
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
        "bookSyncMode": s.book_sync_mode,
        "autoSyncOnStatusChange": s.auto_sync_on_status_change,
        "autoSyncOnProgressUpdate": s.auto_sync_on_progress_update,
        "autoSyncOnRatingChange": s.auto_sync_on_rating_change,
        "privacySettingId": s.privacy_setting_id,
        "lastSyncedAt": s.last_synced_at,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HardcoverSettingsBody {
    #[serde(default)]
    api_token: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    book_sync_mode: Option<String>,
    #[serde(default)]
    auto_sync_on_status_change: Option<bool>,
    #[serde(default)]
    auto_sync_on_progress_update: Option<bool>,
    #[serde(default)]
    auto_sync_on_rating_change: Option<bool>,
    #[serde(default)]
    privacy_setting_id: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HardcoverValidateBody {
    #[serde(default)]
    api_token: Option<String>,
}

#[derive(Clone, Debug)]
struct ShelfBookForSync {
    book_id: i64,
    title: String,
    author: String,
    reading_status: String,
    percentage: f64,
    isbn10: Option<String>,
    isbn13: Option<String>,
    hardcover_id: Option<String>,
    sync_override: Option<String>,
}

fn map_status(reading_status: &str, percentage: f64) -> Option<i64> {
    match reading_status {
        "finished" => Some(HARDCOVER_STATUS_READ),
        "rereading" => Some(HARDCOVER_STATUS_READING),
        "unfinished" => Some(HARDCOVER_STATUS_WANT),
        "open" if percentage > 0.0 && percentage < 1.0 => Some(HARDCOVER_STATUS_READING),
        "open" if percentage >= 1.0 => Some(HARDCOVER_STATUS_READ),
        "open" => None, // unread — skip unless selected
        _ => None,
    }
}

fn load_book_for_sync(db: &Connection, user: i64, book_id: i64) -> ApiResult<ShelfBookForSync> {
    db.query_row(
        "SELECT b.id, b.title, coalesce(b.author,''), b.reading_status,
                coalesce(f.percentage, CASE WHEN b.total_pages>0
                  THEN CAST(b.pages_read AS REAL)/b.total_pages ELSE 0 END, 0),
                m.isbn10, m.isbn13, m.hardcover_id, s.sync_override
         FROM books b
         LEFT JOIN epub_files f ON f.book_id=b.id
         LEFT JOIN book_metadata m ON m.book_id=b.id
         LEFT JOIN hardcover_book_state s ON s.book_id=b.id AND s.user_id=?
         WHERE b.id=? AND b.user_id=? AND b.merged_into IS NULL",
        params![user, book_id, user],
        |r| {
            Ok(ShelfBookForSync {
                book_id: r.get(0)?,
                title: r.get(1)?,
                author: r.get(2)?,
                reading_status: r.get(3)?,
                percentage: r.get(4)?,
                isbn10: r.get(5)?,
                isbn13: r.get(6)?,
                hardcover_id: r.get(7)?,
                sync_override: r.get(8)?,
            })
        },
    )
    .map_err(|_| ApiError(StatusCode::NOT_FOUND, "Book not found."))
}

fn list_eligible_books(db: &Connection, user: i64, mode: &str) -> ApiResult<Vec<ShelfBookForSync>> {
    let mut stmt = db.prepare(
        "SELECT b.id, b.title, coalesce(b.author,''), b.reading_status,
                coalesce(f.percentage, CASE WHEN b.total_pages>0
                  THEN CAST(b.pages_read AS REAL)/b.total_pages ELSE 0 END, 0),
                m.isbn10, m.isbn13, m.hardcover_id, s.sync_override
         FROM books b
         LEFT JOIN epub_files f ON f.book_id=b.id
         LEFT JOIN book_metadata m ON m.book_id=b.id
         LEFT JOIN hardcover_book_state s ON s.book_id=b.id AND s.user_id=?
         WHERE b.user_id=? AND b.merged_into IS NULL
         ORDER BY b.id",
    )?;
    let rows = stmt.query_map(params![user, user], |r| {
        Ok(ShelfBookForSync {
            book_id: r.get(0)?,
            title: r.get(1)?,
            author: r.get(2)?,
            reading_status: r.get(3)?,
            percentage: r.get(4)?,
            isbn10: r.get(5)?,
            isbn13: r.get(6)?,
            hardcover_id: r.get(7)?,
            sync_override: r.get(8)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        let book = row?;
        if book.sync_override.as_deref() == Some("excluded") {
            continue;
        }
        let selected = book.sync_override.as_deref() == Some("included");
        if mode == "selected_only" && !selected {
            continue;
        }
        if selected || map_status(&book.reading_status, book.percentage).is_some() {
            out.push(book);
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)] // mirrors hardcover_book_state column set
fn upsert_book_state(
    db: &Connection,
    user: i64,
    book_id: i64,
    hardcover_book_id: Option<i64>,
    hardcover_edition_id: Option<i64>,
    user_book_id: Option<i64>,
    match_method: Option<&str>,
    match_error: Option<&str>,
    sync_error: Option<&str>,
    synced: bool,
) -> ApiResult<()> {
    let last_synced: Option<String> = if synced {
        db.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')", [], |r| {
            r.get(0)
        })?
    } else {
        None
    };
    db.execute(
        "INSERT INTO hardcover_book_state(
           user_id,book_id,hardcover_book_id,hardcover_edition_id,user_book_id,
           match_method,match_error,last_synced_at,sync_error
         ) VALUES (?,?,?,?,?,?,?,?,?)
         ON CONFLICT(user_id,book_id) DO UPDATE SET
           hardcover_book_id=excluded.hardcover_book_id,
           hardcover_edition_id=excluded.hardcover_edition_id,
           user_book_id=excluded.user_book_id,
           match_method=COALESCE(excluded.match_method, hardcover_book_state.match_method),
           match_error=excluded.match_error,
           last_synced_at=COALESCE(excluded.last_synced_at, hardcover_book_state.last_synced_at),
           sync_error=excluded.sync_error",
        params![
            user,
            book_id,
            hardcover_book_id,
            hardcover_edition_id,
            user_book_id,
            match_method,
            match_error,
            last_synced,
            sync_error,
        ],
    )?;
    Ok(())
}

async fn graphql(token: &str, query: &str, variables: Value) -> Result<Value, String> {
    if stub_mode() {
        return stub_graphql(query, &variables);
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client
        .post(HARDCOVER_GRAPHQL_URL)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .header(
            "User-Agent",
            "Shelf Hardcover Sync (https://github.com/rjyates/Shelf)",
        )
        .json(&json!({ "query": query, "variables": variables }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("Hardcover rate limit exceeded".into());
    }
    if !res.status().is_success() {
        return Err(format!("Hardcover API error: {}", res.status()));
    }
    let body: Value = res.json().await.map_err(|e| e.to_string())?;
    if let Some(errs) = body.get("errors").and_then(|e| e.as_array()) {
        if let Some(msg) = errs
            .first()
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
        {
            return Err(format!("Hardcover GraphQL error: {msg}"));
        }
    }
    Ok(body.get("data").cloned().unwrap_or(Value::Null))
}

fn stub_graphql(query: &str, variables: &Value) -> Result<Value, String> {
    let q = query.to_ascii_lowercase();
    if q.contains("me {") && q.contains("username") {
        return Ok(json!({ "me": [{ "username": "stub-user" }] }));
    }
    if q.contains("findbookbyisbn") || q.contains("isbn_13") || q.contains("isbn_10") {
        return Ok(json!({ "books": [{ "id": 9001, "editions": [{ "id": 8001 }] }] }));
    }
    if q.contains("findbookbyid") {
        let id = variables.get("id").and_then(|v| v.as_i64()).unwrap_or(9001);
        return Ok(json!({ "books": [{ "id": id, "editions": [{ "id": 8001 }] }] }));
    }
    if q.contains("search(") {
        return Ok(json!({ "search": { "error": null, "ids": [9001] } }));
    }
    if q.contains("findbooksbyids") {
        return Ok(json!({
            "books": [{
                "id": 9001,
                "title": "Stub Book",
                "cached_contributors": "Stub Author",
                "editions": [{ "id": 8001 }]
            }]
        }));
    }
    if q.contains("finduserbook") {
        return Ok(json!({ "me": [{ "user_books": [] }] }));
    }
    if q.contains("insert_user_book") {
        return Ok(json!({ "insert_user_book": { "user_book": { "id": 7001 }, "error": null } }));
    }
    if q.contains("update_user_book") {
        return Ok(json!({ "update_user_book": { "user_book": { "id": 7001 }, "error": null } }));
    }
    Err(format!(
        "Unhandled stub GraphQL query: {}",
        &query[..query.len().min(80)]
    ))
}

#[derive(Clone, Debug)]
struct MatchResult {
    hardcover_book_id: i64,
    hardcover_edition_id: Option<i64>,
    method: String,
}

async fn match_book(token: &str, book: &ShelfBookForSync) -> Result<MatchResult, String> {
    if let Some(raw) = book
        .hardcover_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Ok(id) = raw.parse::<i64>() {
            let data = graphql(token, FIND_BY_ID, json!({ "id": id })).await?;
            if data["books"].as_array().and_then(|a| a.first()).is_some() {
                let edition = data["books"][0]["editions"][0]["id"].as_i64();
                return Ok(MatchResult {
                    hardcover_book_id: id,
                    hardcover_edition_id: edition,
                    method: "hardcover_id".into(),
                });
            }
        }
    }
    if let Some(isbn13) = book
        .isbn13
        .as_deref()
        .map(bare_isbn)
        .filter(|s| s.len() == 13)
    {
        let data = graphql(token, FIND_BY_ISBN13, json!({ "isbn": isbn13 })).await?;
        if let Some(row) = data["books"].as_array().and_then(|a| a.first()) {
            if let Some(id) = row["id"].as_i64() {
                return Ok(MatchResult {
                    hardcover_book_id: id,
                    hardcover_edition_id: row["editions"][0]["id"].as_i64(),
                    method: "isbn".into(),
                });
            }
        }
    }
    if let Some(isbn10) = book
        .isbn10
        .as_deref()
        .map(bare_isbn)
        .filter(|s| s.len() == 10)
    {
        let data = graphql(token, FIND_BY_ISBN10, json!({ "isbn": isbn10 })).await?;
        if let Some(row) = data["books"].as_array().and_then(|a| a.first()) {
            if let Some(id) = row["id"].as_i64() {
                return Ok(MatchResult {
                    hardcover_book_id: id,
                    hardcover_edition_id: row["editions"][0]["id"].as_i64(),
                    method: "isbn".into(),
                });
            }
        }
    }
    let query = format!("{} {}", book.title.trim(), book.author.trim())
        .trim()
        .to_string();
    if query.is_empty() {
        return Err("no_match".into());
    }
    let search = graphql(token, SEARCH_BOOKS, json!({ "query": query })).await?;
    let ids: Vec<i64> = search["search"]["ids"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_i64()).take(5).collect())
        .unwrap_or_default();
    if ids.is_empty() {
        return Err("no_match".into());
    }
    let data = graphql(token, FIND_BOOKS_BY_IDS, json!({ "ids": ids })).await?;
    let want_title = book.title.trim().to_ascii_lowercase();
    let want_author = book.author.trim().to_ascii_lowercase();
    let books = data["books"].as_array().cloned().unwrap_or_default();
    let chosen = books.into_iter().find(|cand| {
        let title = cand["title"].as_str().unwrap_or("").to_ascii_lowercase();
        let contributors = cand["cached_contributors"]
            .as_str()
            .unwrap_or("")
            .to_ascii_lowercase();
        title.contains(&want_title)
            || want_title.contains(&title)
            || (!want_author.is_empty() && contributors.contains(&want_author))
    });
    let Some(row) = chosen.or_else(|| {
        // Prefer first search hit when stub/title soft-match fails.
        data["books"].as_array().and_then(|a| a.first()).cloned()
    }) else {
        return Err("no_match".into());
    };
    let id = row["id"].as_i64().ok_or_else(|| "no_match".to_string())?;
    Ok(MatchResult {
        hardcover_book_id: id,
        hardcover_edition_id: row["editions"][0]["id"].as_i64(),
        method: "title_author".into(),
    })
}

async fn push_user_book(
    token: &str,
    hardcover_book_id: i64,
    edition_id: Option<i64>,
    status_id: i64,
    privacy_setting_id: i64,
    progress: f64,
) -> Result<i64, String> {
    let existing = graphql(
        token,
        FIND_USER_BOOK,
        json!({ "bookId": hardcover_book_id }),
    )
    .await?;
    let existing_id = existing["me"][0]["user_books"][0]["id"].as_i64();
    let mut object = json!({
        "book_id": hardcover_book_id,
        "status_id": status_id,
        "privacy_setting_id": privacy_setting_id,
    });
    if let Some(ed) = edition_id {
        object["edition_id"] = json!(ed);
    }
    if (0.0..1.0).contains(&progress) && status_id == HARDCOVER_STATUS_READING {
        object["progress"] = json!((progress * 100.0).round() as i64);
        object["progress_type"] = json!("percentage");
    }
    if let Some(id) = existing_id {
        let data = graphql(
            token,
            UPDATE_USER_BOOK,
            json!({ "id": id, "object": object }),
        )
        .await?;
        if let Some(err) = data["update_user_book"]["error"].as_str() {
            if !err.is_empty() {
                return Err(err.into());
            }
        }
        return Ok(data["update_user_book"]["user_book"]["id"]
            .as_i64()
            .unwrap_or(id));
    }
    let data = graphql(token, INSERT_USER_BOOK, json!({ "object": object })).await?;
    if let Some(err) = data["insert_user_book"]["error"].as_str() {
        if !err.is_empty() {
            return Err(err.into());
        }
    }
    data["insert_user_book"]["user_book"]["id"]
        .as_i64()
        .ok_or_else(|| "insert_user_book missing id".into())
}

async fn sync_one_book(
    token: &str,
    privacy: i64,
    book: &ShelfBookForSync,
) -> Result<(MatchResult, i64), String> {
    let status = map_status(&book.reading_status, book.percentage).unwrap_or(HARDCOVER_STATUS_WANT);
    let matched = match_book(token, book).await?;
    let user_book_id = push_user_book(
        token,
        matched.hardcover_book_id,
        matched.hardcover_edition_id,
        status,
        privacy,
        book.percentage,
    )
    .await?;
    Ok((matched, user_book_id))
}

pub(super) async fn get_hardcover_settings(
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

pub(super) async fn put_hardcover_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<HardcoverSettingsBody>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let mut settings = load_settings(db, user)?;
            if let Some(token) = body.api_token {
                let token = strip_bearer(&token);
                if token.is_empty() && settings.api_token.is_empty() {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "API token is required to connect Hardcover.",
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
                    "API token is required to connect Hardcover.",
                ));
            }
            if let Some(v) = body.enabled {
                settings.enabled = v;
            }
            if let Some(mode) = body.book_sync_mode {
                if mode != "all_eligible" && mode != "selected_only" {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "bookSyncMode must be all_eligible or selected_only.",
                    ));
                }
                settings.book_sync_mode = mode;
            }
            if let Some(v) = body.auto_sync_on_status_change {
                settings.auto_sync_on_status_change = v;
            }
            if let Some(v) = body.auto_sync_on_progress_update {
                settings.auto_sync_on_progress_update = v;
            }
            if let Some(v) = body.auto_sync_on_rating_change {
                settings.auto_sync_on_rating_change = v;
            }
            if let Some(v) = body.privacy_setting_id {
                if !(1..=3).contains(&v) {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "privacySettingId must be 1, 2, or 3.",
                    ));
                }
                settings.privacy_setting_id = v;
            }
            db.execute(
                "UPDATE hardcover_settings SET
                   api_token=?, enabled=?, book_sync_mode=?,
                   auto_sync_on_status_change=?, auto_sync_on_progress_update=?,
                   auto_sync_on_rating_change=?, privacy_setting_id=?,
                   updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE user_id=?",
                params![
                    settings.api_token,
                    if settings.enabled { 1 } else { 0 },
                    settings.book_sync_mode,
                    if settings.auto_sync_on_status_change {
                        1
                    } else {
                        0
                    },
                    if settings.auto_sync_on_progress_update {
                        1
                    } else {
                        0
                    },
                    if settings.auto_sync_on_rating_change {
                        1
                    } else {
                        0
                    },
                    settings.privacy_setting_id,
                    user,
                ],
            )?;
            Ok(Json(settings_json(&settings)))
        })
        .await
}

pub(super) async fn delete_hardcover_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let n = db.execute(
                "DELETE FROM hardcover_settings WHERE user_id=?",
                params![user],
            )?;
            db.execute(
                "DELETE FROM hardcover_book_state WHERE user_id=?",
                params![user],
            )?;
            if n == 0 {
                return Err(ApiError(
                    StatusCode::NOT_FOUND,
                    "Hardcover integration not configured.",
                ));
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn validate_hardcover_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<HardcoverValidateBody>,
) -> ApiResult<Json<Value>> {
    let (token, _user) = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                let settings = load_settings(db, user)?;
                let token = body
                    .api_token
                    .as_deref()
                    .map(strip_bearer)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(settings.api_token);
                if token.is_empty() {
                    return Err(ApiError(StatusCode::BAD_REQUEST, "API token is required."));
                }
                Ok((token, user))
            }
        })
        .await?;
    match graphql(&token, ME_QUERY, json!({})).await {
        Ok(data) => {
            let username = data["me"][0]["username"].as_str().map(str::to_string);
            if let Some(username) = username {
                Ok(Json(
                    json!({ "valid": true, "hardcoverUsername": username }),
                ))
            } else {
                Ok(Json(json!({ "valid": false })))
            }
        }
        Err(_) => Ok(Json(json!({ "valid": false }))),
    }
}

pub(super) async fn sync_hardcover_all(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let (token, privacy, mode, books) = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                let settings = load_settings(db, user)?;
                if settings.api_token.is_empty() || !settings.enabled {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "Enable Hardcover and save an API token first.",
                    ));
                }
                let books = list_eligible_books(db, user, &settings.book_sync_mode)?;
                Ok((
                    settings.api_token,
                    settings.privacy_setting_id,
                    settings.book_sync_mode,
                    books,
                ))
            }
        })
        .await?;

    let mut synced = 0i64;
    let mut skipped = 0i64;
    let mut failed = 0i64;
    let total = books.len() as i64;
    for book in &books {
        match sync_one_book(&token, privacy, book).await {
            Ok((matched, user_book_id)) => {
                let book_id = book.book_id;
                let method = matched.method.clone();
                let headers = headers.clone();
                let _ = state
                    .db(move |db| {
                        let user = authenticate(db, &headers)?;
                        upsert_book_state(
                            db,
                            user,
                            book_id,
                            Some(matched.hardcover_book_id),
                            matched.hardcover_edition_id,
                            Some(user_book_id),
                            Some(&method),
                            None,
                            None,
                            true,
                        )?;
                        Ok(())
                    })
                    .await;
                synced += 1;
            }
            Err(err) if err == "no_match" => {
                let book_id = book.book_id;
                let headers = headers.clone();
                let _ = state
                    .db(move |db| {
                        let user = authenticate(db, &headers)?;
                        upsert_book_state(
                            db,
                            user,
                            book_id,
                            None,
                            None,
                            None,
                            None,
                            Some("no_match"),
                            Some("no_match"),
                            false,
                        )?;
                        Ok(())
                    })
                    .await;
                skipped += 1;
            }
            Err(err) => {
                let book_id = book.book_id;
                let msg = err.chars().take(500).collect::<String>();
                let headers = headers.clone();
                let _ = state
                    .db(move |db| {
                        let user = authenticate(db, &headers)?;
                        upsert_book_state(
                            db,
                            user,
                            book_id,
                            None,
                            None,
                            None,
                            None,
                            None,
                            Some(&msg),
                            false,
                        )?;
                        Ok(())
                    })
                    .await;
                failed += 1;
            }
        }
    }

    let _ = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                db.execute(
                    "UPDATE hardcover_settings SET
                       last_synced_at=strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                       updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
                     WHERE user_id=?",
                    params![user],
                )?;
                Ok(())
            }
        })
        .await;

    Ok(Json(json!({
        "status": "completed",
        "mode": mode,
        "totalBooks": total,
        "syncedBooks": synced,
        "skippedBooks": skipped,
        "failedBooks": failed,
    })))
}

pub(super) async fn sync_hardcover_book(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(book_id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let (token, privacy, book) = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                let settings = load_settings(db, user)?;
                if settings.api_token.is_empty() || !settings.enabled {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "Enable Hardcover and save an API token first.",
                    ));
                }
                let book = load_book_for_sync(db, user, book_id)?;
                Ok((settings.api_token, settings.privacy_setting_id, book))
            }
        })
        .await?;

    match sync_one_book(&token, privacy, &book).await {
        Ok((matched, user_book_id)) => {
            let method = matched.method.clone();
            state
                .db({
                    let headers = headers.clone();
                    move |db| {
                        let user = authenticate(db, &headers)?;
                        upsert_book_state(
                            db,
                            user,
                            book_id,
                            Some(matched.hardcover_book_id),
                            matched.hardcover_edition_id,
                            Some(user_book_id),
                            Some(&method),
                            None,
                            None,
                            true,
                        )?;
                        db.execute(
                            "UPDATE hardcover_settings SET
                               last_synced_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
                             WHERE user_id=?",
                            params![user],
                        )?;
                        Ok(Json(json!({
                            "result": "synced",
                            "state": {
                                "bookId": book_id,
                                "hardcoverBookId": matched.hardcover_book_id,
                                "hardcoverEditionId": matched.hardcover_edition_id,
                                "matchMethod": method,
                                "syncError": Value::Null,
                            }
                        })))
                    }
                })
                .await
        }
        Err(err) if err == "no_match" => {
            state
                .db({
                    let headers = headers.clone();
                    move |db| {
                        let user = authenticate(db, &headers)?;
                        upsert_book_state(
                            db,
                            user,
                            book_id,
                            None,
                            None,
                            None,
                            None,
                            Some("no_match"),
                            Some("no_match"),
                            false,
                        )?;
                        Ok(Json(json!({
                            "result": "skipped",
                            "state": {
                                "bookId": book_id,
                                "hardcoverBookId": Value::Null,
                                "matchMethod": Value::Null,
                                "syncError": "no_match",
                            }
                        })))
                    }
                })
                .await
        }
        Err(err) => {
            let msg = err.chars().take(500).collect::<String>();
            state
                .db({
                    let headers = headers.clone();
                    let msg = msg.clone();
                    move |db| {
                        let user = authenticate(db, &headers)?;
                        upsert_book_state(
                            db,
                            user,
                            book_id,
                            None,
                            None,
                            None,
                            None,
                            None,
                            Some(&msg),
                            false,
                        )?;
                        Ok(Json(json!({
                            "result": "failed",
                            "state": {
                                "bookId": book_id,
                                "syncError": msg,
                            }
                        })))
                    }
                })
                .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_map_skips_unread_open() {
        assert_eq!(map_status("open", 0.0), None);
        assert_eq!(map_status("open", 0.4), Some(HARDCOVER_STATUS_READING));
        assert_eq!(map_status("finished", 1.0), Some(HARDCOVER_STATUS_READ));
        assert_eq!(map_status("unfinished", 0.0), Some(HARDCOVER_STATUS_WANT));
    }

    #[test]
    fn bare_isbn_strips_hyphens() {
        assert_eq!(bare_isbn("978-0-261-10237-8"), "9780261102378");
    }

    #[test]
    fn strip_bearer_prefix() {
        assert_eq!(strip_bearer("Bearer abc"), "abc");
        assert_eq!(strip_bearer("abc"), "abc");
    }
}
