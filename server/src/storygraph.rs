//! StoryGraph account sync (M12 slice 11).
//!
//! BO reference @ c77966cc: cookie auth + browse match + status/progress push.
//! Shelf ships settings, validate, sync-now (all or one book), manual link.
//! Import-from-StoryGraph, edition switcher, and event auto-sync stay deferred.

use super::*;
use serde_json::{json, Value};
use std::collections::HashMap;

const STORYGRAPH_BASE: &str = "https://app.thestorygraph.com";
const SESSION_COOKIE: &str = "_storygraph_session";
const REMEMBER_COOKIE: &str = "remember_user_token";
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

pub(crate) fn migrate(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS storygraph_settings (
           user_id INTEGER PRIMARY KEY REFERENCES users(id),
           session_cookie TEXT NOT NULL DEFAULT '',
           remember_token TEXT NOT NULL DEFAULT '',
           enabled INTEGER NOT NULL DEFAULT 0,
           book_sync_mode TEXT NOT NULL DEFAULT 'all_eligible',
           auto_sync_on_status_change INTEGER NOT NULL DEFAULT 1,
           auto_sync_on_progress_update INTEGER NOT NULL DEFAULT 1,
           last_synced_at TEXT,
           updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         );
         CREATE TABLE IF NOT EXISTS storygraph_book_state (
           user_id INTEGER NOT NULL REFERENCES users(id),
           book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
           storygraph_book_id TEXT,
           match_method TEXT,
           match_error TEXT,
           sync_override TEXT,
           last_synced_at TEXT,
           last_synced_status TEXT,
           last_synced_progress REAL,
           sync_error TEXT,
           PRIMARY KEY (user_id, book_id)
         )",
    )?;
    Ok(())
}

fn stub_mode() -> bool {
    std::env::var("SHELF_STORYGRAPH_STUB").as_deref() == Ok("1")
}

#[derive(Clone, Debug)]
struct StorygraphSettings {
    session_cookie: String,
    remember_token: String,
    enabled: bool,
    book_sync_mode: String,
    auto_sync_on_status_change: bool,
    auto_sync_on_progress_update: bool,
    last_synced_at: Option<String>,
}

fn ensure_settings_row(db: &Connection, user: i64) -> ApiResult<()> {
    db.execute(
        "INSERT OR IGNORE INTO storygraph_settings(user_id) VALUES (?)",
        params![user],
    )?;
    Ok(())
}

fn load_settings(db: &Connection, user: i64) -> ApiResult<StorygraphSettings> {
    ensure_settings_row(db, user)?;
    db.query_row(
        "SELECT session_cookie,remember_token,enabled,book_sync_mode,
                auto_sync_on_status_change,auto_sync_on_progress_update,last_synced_at
         FROM storygraph_settings WHERE user_id=?",
        params![user],
        |r| {
            Ok(StorygraphSettings {
                session_cookie: r.get(0)?,
                remember_token: r.get(1)?,
                enabled: r.get::<_, i64>(2)? != 0,
                book_sync_mode: r.get(3)?,
                auto_sync_on_status_change: r.get::<_, i64>(4)? != 0,
                auto_sync_on_progress_update: r.get::<_, i64>(5)? != 0,
                last_synced_at: r.get(6)?,
            })
        },
    )
    .map_err(ApiError::from)
}

fn cookies_configured(s: &StorygraphSettings) -> bool {
    !s.session_cookie.trim().is_empty() && !s.remember_token.trim().is_empty()
}

fn settings_json(s: &StorygraphSettings) -> Value {
    let configured = cookies_configured(s);
    let disabled_reason = if !configured {
        Some("missing_cookies")
    } else if !s.enabled {
        Some("user_disabled")
    } else {
        None
    };
    json!({
        "cookiesConfigured": configured,
        "enabled": s.enabled,
        "effectiveEnabled": configured && s.enabled,
        "disabledReason": disabled_reason,
        "bookSyncMode": s.book_sync_mode,
        "autoSyncOnStatusChange": s.auto_sync_on_status_change,
        "autoSyncOnProgressUpdate": s.auto_sync_on_progress_update,
        "lastSyncedAt": s.last_synced_at,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StorygraphSettingsBody {
    #[serde(default)]
    session_cookie: Option<String>,
    #[serde(default)]
    remember_token: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    book_sync_mode: Option<String>,
    #[serde(default)]
    auto_sync_on_status_change: Option<bool>,
    #[serde(default)]
    auto_sync_on_progress_update: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StorygraphValidateBody {
    #[serde(default)]
    session_cookie: Option<String>,
    #[serde(default)]
    remember_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StorygraphLinkBody {
    input: String,
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
    sync_override: Option<String>,
    storygraph_book_id: Option<String>,
}

fn map_status(reading_status: &str, percentage: f64) -> Option<&'static str> {
    match reading_status {
        "finished" => Some("read"),
        "rereading" => Some("rereading"),
        "unfinished" => Some("to-read"),
        "open" if percentage > 0.0 && percentage < 1.0 => Some("currently-reading"),
        "open" if percentage >= 1.0 => Some("read"),
        "open" => None,
        _ => None,
    }
}

fn list_eligible_books(db: &Connection, user: i64, mode: &str) -> ApiResult<Vec<ShelfBookForSync>> {
    let mut stmt = db.prepare(
        "SELECT b.id, b.title, coalesce(b.author,''), b.reading_status,
                coalesce(f.percentage, CASE WHEN b.total_pages>0
                  THEN CAST(b.pages_read AS REAL)/b.total_pages ELSE 0 END, 0),
                m.isbn10, m.isbn13, s.sync_override, s.storygraph_book_id
         FROM books b
         LEFT JOIN epub_files f ON f.book_id=b.id
         LEFT JOIN book_metadata m ON m.book_id=b.id
         LEFT JOIN storygraph_book_state s ON s.book_id=b.id AND s.user_id=b.user_id
         WHERE b.user_id=? AND b.merged_into IS NULL
         ORDER BY b.id",
    )?;
    let rows = stmt.query_map(params![user], |r| {
        Ok(ShelfBookForSync {
            book_id: r.get(0)?,
            title: r.get(1)?,
            author: r.get(2)?,
            reading_status: r.get(3)?,
            percentage: r.get(4)?,
            isbn10: r.get(5)?,
            isbn13: r.get(6)?,
            sync_override: r.get(7)?,
            storygraph_book_id: r.get(8)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        let book = row?;
        let override_ = book.sync_override.as_deref();
        if override_ == Some("excluded") {
            continue;
        }
        let mapped = map_status(&book.reading_status, book.percentage);
        let included = override_ == Some("included")
            || (mode == "all_eligible" && mapped.is_some())
            || (mode == "selected_only" && override_ == Some("included"));
        if included && mapped.is_some() {
            out.push(book);
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)] // mirrors storygraph_book_state column set
fn upsert_book_state(
    db: &Connection,
    user: i64,
    book_id: i64,
    storygraph_book_id: Option<&str>,
    match_method: Option<&str>,
    match_error: Option<&str>,
    last_synced_status: Option<&str>,
    last_synced_progress: Option<f64>,
    sync_error: Option<&str>,
    mark_synced: bool,
) -> ApiResult<()> {
    db.execute(
        "INSERT INTO storygraph_book_state(
            user_id, book_id, storygraph_book_id, match_method, match_error,
            last_synced_at, last_synced_status, last_synced_progress, sync_error
         ) VALUES (?,?,?,?,?,
            CASE WHEN ? THEN strftime('%Y-%m-%dT%H:%M:%SZ','now') ELSE NULL END,
            ?,?,?)
         ON CONFLICT(user_id, book_id) DO UPDATE SET
            storygraph_book_id=COALESCE(excluded.storygraph_book_id, storygraph_book_state.storygraph_book_id),
            match_method=COALESCE(excluded.match_method, storygraph_book_state.match_method),
            match_error=excluded.match_error,
            last_synced_at=CASE WHEN ? THEN strftime('%Y-%m-%dT%H:%M:%SZ','now')
                               ELSE storygraph_book_state.last_synced_at END,
            last_synced_status=COALESCE(excluded.last_synced_status, storygraph_book_state.last_synced_status),
            last_synced_progress=COALESCE(excluded.last_synced_progress, storygraph_book_state.last_synced_progress),
            sync_error=excluded.sync_error",
        params![
            user,
            book_id,
            storygraph_book_id,
            match_method,
            match_error,
            if mark_synced { 1 } else { 0 },
            last_synced_status,
            last_synced_progress,
            sync_error,
            if mark_synced { 1 } else { 0 },
        ],
    )?;
    Ok(())
}

fn cookie_header(session: &str, remember: &str) -> String {
    format!("{REMEMBER_COOKIE}={remember}; {SESSION_COOKIE}={session}")
}

fn encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn extract_csrf(html: &str) -> Option<String> {
    if let Some(cap) =
        regex::Regex::new(r#"meta\s+name=["']csrf-token["']\s+content=["']([^"']+)["']"#)
            .ok()
            .and_then(|re| re.captures(html))
    {
        return Some(cap[1].to_string());
    }
    regex::Regex::new(r#"name=["']authenticity_token["']\s+value=["']([^"']+)["']"#)
        .ok()
        .and_then(|re| re.captures(html))
        .map(|cap| cap[1].to_string())
}

fn parse_book_ids(html: &str, limit: usize) -> Vec<String> {
    let Ok(re) = regex::Regex::new(r#"/books/([^/?"'\s]+)"#) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seen = HashMap::new();
    for cap in re.captures_iter(html) {
        let id = cap[1].to_string();
        if seen.insert(id.clone(), ()).is_none() {
            out.push(id);
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

fn resolve_manual_id(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(re) = regex::Regex::new(r#"/books/([^/?#]+)"#) {
        if let Some(cap) = re.captures(trimmed) {
            return Some(cap[1].to_string());
        }
    }
    Some(trimmed.to_string())
}

struct SgResponse {
    status: u16,
    html: String,
    redirected_to_sign_in: bool,
}

async fn sg_get(session: &str, remember: &str, path: &str) -> Result<SgResponse, String> {
    if stub_mode() {
        return Ok(SgResponse {
            status: 200,
            html: r#"<meta name="csrf-token" content="stub-csrf"><a href="/books/stub-book-1">Stub</a>"#
                .into(),
            redirected_to_sign_in: false,
        });
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| e.to_string())?;
    let url = if path.starts_with("http") {
        path.to_string()
    } else {
        format!("{STORYGRAPH_BASE}{path}")
    };
    let res = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Cookie", cookie_header(session, remember))
        .header(
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status().as_u16();
    let final_url = res.url().to_string();
    let redirected_to_sign_in =
        final_url.contains("/users/sign_in") || final_url.contains("/users/sign-in");
    let html = res.text().await.map_err(|e| e.to_string())?;
    Ok(SgResponse {
        status,
        html,
        redirected_to_sign_in,
    })
}

async fn sg_post(
    session: &str,
    remember: &str,
    path: &str,
    form: &[(&str, &str)],
    csrf: &str,
) -> Result<SgResponse, String> {
    if stub_mode() {
        return Ok(SgResponse {
            status: 200,
            html: String::new(),
            redirected_to_sign_in: false,
        });
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| e.to_string())?;
    let url = if path.starts_with("http") {
        path.to_string()
    } else {
        format!("{STORYGRAPH_BASE}{path}")
    };
    let mut params: Vec<(&str, &str)> = form.to_vec();
    params.push(("authenticity_token", csrf));
    let res = client
        .post(&url)
        .header("User-Agent", USER_AGENT)
        .header("Cookie", cookie_header(session, remember))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("X-CSRF-Token", csrf)
        .header("X-Requested-With", "XMLHttpRequest")
        .form(&params)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status().as_u16();
    let final_url = res.url().to_string();
    let redirected_to_sign_in =
        final_url.contains("/users/sign_in") || final_url.contains("/users/sign-in");
    let html = res.text().await.unwrap_or_default();
    Ok(SgResponse {
        status,
        html,
        redirected_to_sign_in,
    })
}

fn success_status(status: u16) -> bool {
    (200..300).contains(&status) || status == 302 || status == 303
}

async fn validate_cookies(session: &str, remember: &str) -> bool {
    match sg_get(session, remember, "/journal").await {
        Ok(res) => res.status == 200 && !res.redirected_to_sign_in,
        Err(_) => false,
    }
}

async fn match_book(
    session: &str,
    remember: &str,
    book: &ShelfBookForSync,
) -> Result<(String, &'static str), String> {
    if let Some(id) = book.storygraph_book_id.as_deref().filter(|s| !s.is_empty()) {
        return Ok((id.to_string(), "cached"));
    }
    let mut terms: Vec<(String, &'static str)> = Vec::new();
    if let Some(isbn) = book
        .isbn13
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        terms.push((isbn.to_string(), "isbn"));
    }
    if let Some(isbn) = book
        .isbn10
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        terms.push((isbn.to_string(), "isbn"));
    }
    let title_q = format!("{} {}", book.title.trim(), book.author.trim())
        .trim()
        .to_string();
    if !title_q.is_empty() {
        terms.push((title_q, "title"));
    }
    for (term, method) in terms {
        let path = format!("/browse?search_term={}", encode_query(&term));
        let res = sg_get(session, remember, &path).await?;
        if res.redirected_to_sign_in || res.status != 200 {
            continue;
        }
        let ids = parse_book_ids(&res.html, 3);
        if let Some(id) = ids.into_iter().next() {
            return Ok((id, method));
        }
    }
    Err("no_match".into())
}

async fn update_status(
    session: &str,
    remember: &str,
    storygraph_book_id: &str,
    status: &str,
) -> Result<(), String> {
    let page = sg_get(session, remember, &format!("/books/{storygraph_book_id}")).await?;
    if page.redirected_to_sign_in || page.status != 200 {
        return Err("book_page_failed".into());
    }
    let csrf = extract_csrf(&page.html).ok_or_else(|| "missing_csrf_token".to_string())?;
    let path = format!("/update-status.js?book_id={storygraph_book_id}&status={status}");
    let res = sg_post(session, remember, &path, &[], &csrf).await?;
    if success_status(res.status) && !res.redirected_to_sign_in {
        return Ok(());
    }
    if status == "currently-reading" {
        let retry_path = format!("/update-status.js?book_id={storygraph_book_id}&status=rereading");
        let retry = sg_post(session, remember, &retry_path, &[], &csrf).await?;
        if success_status(retry.status) && !retry.redirected_to_sign_in {
            return Ok(());
        }
        return Err(format!("status_update_failed:{}", retry.status));
    }
    Err(format!("status_update_failed:{}", res.status))
}

async fn update_progress(
    session: &str,
    remember: &str,
    storygraph_book_id: &str,
    progress_pct: f64,
) -> Result<(), String> {
    let page = sg_get(session, remember, &format!("/books/{storygraph_book_id}")).await?;
    if page.redirected_to_sign_in || page.status != 200 {
        return Err("book_page_failed".into());
    }
    let csrf = extract_csrf(&page.html).ok_or_else(|| "missing_csrf_token".to_string())?;
    let pages = regex::Regex::new(
        r#"name=["']read_status\[book_num_of_pages\]["']\s+value=["']([^"']+)["']"#,
    )
    .ok()
    .and_then(|re| re.captures(&page.html))
    .map(|c| c[1].to_string())
    .unwrap_or_else(|| "0".into());
    let pct = format!(
        "{}",
        (progress_pct * 100.0).round().clamp(0.0, 100.0) as i64
    );
    let res = sg_post(
        session,
        remember,
        "/update-progress",
        &[
            ("read_status[progress_number]", pct.as_str()),
            ("read_status[progress_type]", "percentage"),
            ("read_status[book_num_of_pages]", pages.as_str()),
        ],
        &csrf,
    )
    .await?;
    if success_status(res.status) && !res.redirected_to_sign_in {
        Ok(())
    } else {
        Err(format!("progress_update_failed:{}", res.status))
    }
}

async fn sync_one_book(
    session: &str,
    remember: &str,
    book: &ShelfBookForSync,
) -> Result<(String, &'static str), String> {
    let status = map_status(&book.reading_status, book.percentage)
        .ok_or_else(|| "unsupported_status".to_string())?;
    let (sg_id, method) = match_book(session, remember, book).await?;
    update_status(session, remember, &sg_id, status).await?;
    if status != "read" && book.percentage > 0.0 {
        let _ = update_progress(session, remember, &sg_id, book.percentage).await;
    }
    Ok((sg_id, method))
}

pub(super) async fn get_storygraph_settings(
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

pub(super) async fn put_storygraph_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<StorygraphSettingsBody>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let mut settings = load_settings(db, user)?;
            if let Some(cookie) = body.session_cookie {
                let cookie = cookie.trim().to_string();
                if cookie.is_empty() {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "Cookie values cannot be empty.",
                    ));
                }
                // shortcut: cookies stored plaintext in SQLite (single-user);
                // encrypt at rest when outbound secrets get a shared vault.
                settings.session_cookie = cookie;
            }
            if let Some(token) = body.remember_token {
                let token = token.trim().to_string();
                if token.is_empty() {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "Cookie values cannot be empty.",
                    ));
                }
                settings.remember_token = token;
            }
            if !cookies_configured(&settings) {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Both the session cookie and remember token are required to connect StoryGraph.",
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
            db.execute(
                "UPDATE storygraph_settings SET
                   session_cookie=?, remember_token=?, enabled=?, book_sync_mode=?,
                   auto_sync_on_status_change=?, auto_sync_on_progress_update=?,
                   updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE user_id=?",
                params![
                    settings.session_cookie,
                    settings.remember_token,
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
                    user,
                ],
            )?;
            Ok(Json(settings_json(&settings)))
        })
        .await
}

pub(super) async fn delete_storygraph_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let n = db.execute(
                "DELETE FROM storygraph_settings WHERE user_id=?",
                params![user],
            )?;
            db.execute(
                "DELETE FROM storygraph_book_state WHERE user_id=?",
                params![user],
            )?;
            if n == 0 {
                return Err(ApiError(
                    StatusCode::NOT_FOUND,
                    "StoryGraph integration not configured.",
                ));
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn validate_storygraph_cookies(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<StorygraphValidateBody>,
) -> ApiResult<Json<Value>> {
    let (session, remember) = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                let settings = load_settings(db, user)?;
                let session = body
                    .session_cookie
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(settings.session_cookie.as_str())
                    .to_string();
                let remember = body
                    .remember_token
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(settings.remember_token.as_str())
                    .to_string();
                if session.is_empty() || remember.is_empty() {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "Session cookie and remember token are required.",
                    ));
                }
                Ok((session, remember))
            }
        })
        .await?;
    Ok(Json(
        json!({ "valid": validate_cookies(&session, &remember).await }),
    ))
}

pub(super) async fn sync_storygraph_all(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let (session, remember, books) = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                let settings = load_settings(db, user)?;
                if !cookies_configured(&settings) || !settings.enabled {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "Enable StoryGraph and save cookies first.",
                    ));
                }
                let books = list_eligible_books(db, user, &settings.book_sync_mode)?;
                Ok((settings.session_cookie, settings.remember_token, books))
            }
        })
        .await?;

    let mut synced = 0i64;
    let mut skipped = 0i64;
    let mut failed = 0i64;
    for book in books {
        match sync_one_book(&session, &remember, &book).await {
            Ok((sg_id, method)) => {
                let status = map_status(&book.reading_status, book.percentage);
                let _ = state
                    .db({
                        let headers = headers.clone();
                        let sg_id = sg_id.clone();
                        let book_id = book.book_id;
                        let pct = book.percentage;
                        move |db| {
                            let user = authenticate(db, &headers)?;
                            upsert_book_state(
                                db,
                                user,
                                book_id,
                                Some(&sg_id),
                                Some(method),
                                None,
                                status,
                                Some(pct),
                                None,
                                true,
                            )?;
                            Ok(())
                        }
                    })
                    .await;
                synced += 1;
            }
            Err(err) => {
                let _ = state
                    .db({
                        let headers = headers.clone();
                        let book_id = book.book_id;
                        let err = err.clone();
                        move |db| {
                            let user = authenticate(db, &headers)?;
                            upsert_book_state(
                                db,
                                user,
                                book_id,
                                None,
                                None,
                                if err == "no_match" {
                                    Some("no_match")
                                } else {
                                    None
                                },
                                None,
                                None,
                                Some(&err),
                                false,
                            )?;
                            Ok(())
                        }
                    })
                    .await;
                if err == "unsupported_status" {
                    skipped += 1;
                } else {
                    failed += 1;
                }
            }
        }
    }

    let _ = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                db.execute(
                    "UPDATE storygraph_settings SET
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
        "syncedBooks": synced,
        "skippedBooks": skipped,
        "failedBooks": failed,
    })))
}

pub(super) async fn sync_storygraph_book(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let (session, remember, book) = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                let settings = load_settings(db, user)?;
                if !cookies_configured(&settings) || !settings.enabled {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "Enable StoryGraph and save cookies first.",
                    ));
                }
                let books = list_eligible_books(db, user, "all_eligible")?;
                // still allow sync when selected_only would hide it: load raw row
                let book = books.into_iter().find(|b| b.book_id == id).ok_or(ApiError(
                    StatusCode::NOT_FOUND,
                    "Book not found or not eligible.",
                ))?;
                Ok((settings.session_cookie, settings.remember_token, book))
            }
        })
        .await?;

    match sync_one_book(&session, &remember, &book).await {
        Ok((sg_id, method)) => {
            let status = map_status(&book.reading_status, book.percentage);
            let _ = state
                .db({
                    let headers = headers.clone();
                    let sg_id = sg_id.clone();
                    let book_id = book.book_id;
                    let pct = book.percentage;
                    move |db| {
                        let user = authenticate(db, &headers)?;
                        upsert_book_state(
                            db,
                            user,
                            book_id,
                            Some(&sg_id),
                            Some(method),
                            None,
                            status,
                            Some(pct),
                            None,
                            true,
                        )?;
                        Ok(())
                    }
                })
                .await;
            Ok(Json(
                json!({ "result": "synced", "storygraphBookId": sg_id }),
            ))
        }
        Err(err) => {
            let _ = state
                .db({
                    let headers = headers.clone();
                    let book_id = book.book_id;
                    let err = err.clone();
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
                            Some(&err),
                            false,
                        )?;
                        Ok(())
                    }
                })
                .await;
            Ok(Json(json!({ "result": "failed", "error": err })))
        }
    }
}

pub(super) async fn link_storygraph_book(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<StorygraphLinkBody>,
) -> ApiResult<Json<Value>> {
    let sg_id = resolve_manual_id(&body.input).ok_or(ApiError(
        StatusCode::BAD_REQUEST,
        "Provide a StoryGraph book URL or id.",
    ))?;
    let (session, remember, book) = state
        .db({
            let headers = headers.clone();
            let sg_id = sg_id.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                let settings = load_settings(db, user)?;
                if !cookies_configured(&settings) || !settings.enabled {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "Enable StoryGraph and save cookies first.",
                    ));
                }
                // ensure book belongs to user
                let exists: i64 = db.query_row(
                    "SELECT COUNT(*) FROM books WHERE id=? AND user_id=? AND merged_into IS NULL",
                    params![id, user],
                    |r| r.get(0),
                )?;
                if exists == 0 {
                    return Err(ApiError(StatusCode::NOT_FOUND, "Book not found."));
                }
                upsert_book_state(
                    db,
                    user,
                    id,
                    Some(&sg_id),
                    Some("manual"),
                    None,
                    None,
                    None,
                    None,
                    false,
                )?;
                let mut books = list_eligible_books(db, user, "all_eligible")?;
                let book = books
                    .drain(..)
                    .find(|b| b.book_id == id)
                    .unwrap_or(ShelfBookForSync {
                        book_id: id,
                        title: String::new(),
                        author: String::new(),
                        reading_status: "finished".into(),
                        percentage: 1.0,
                        isbn10: None,
                        isbn13: None,
                        sync_override: Some("included".into()),
                        storygraph_book_id: Some(sg_id.clone()),
                    });
                Ok((settings.session_cookie, settings.remember_token, book))
            }
        })
        .await?;

    match sync_one_book(&session, &remember, &book).await {
        Ok((sg_id, method)) => {
            let status = map_status(&book.reading_status, book.percentage);
            let _ = state
                .db({
                    let headers = headers.clone();
                    let sg_id = sg_id.clone();
                    move |db| {
                        let user = authenticate(db, &headers)?;
                        upsert_book_state(
                            db,
                            user,
                            id,
                            Some(&sg_id),
                            Some(method),
                            None,
                            status,
                            Some(book.percentage),
                            None,
                            true,
                        )?;
                        Ok(())
                    }
                })
                .await;
            Ok(Json(json!({
                "success": true,
                "storygraphBookId": sg_id,
            })))
        }
        Err(_) => Ok(Json(json!({
            "success": true,
            "storygraphBookId": sg_id,
            "synced": false,
        }))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_status_finished() {
        assert_eq!(map_status("finished", 1.0), Some("read"));
        assert_eq!(map_status("open", 0.0), None);
        assert_eq!(map_status("open", 0.4), Some("currently-reading"));
    }

    #[test]
    fn parse_book_ids_dedupes() {
        let html =
            r#"<a href="/books/abc">A</a><a href="/books/abc">A</a><a href="/books/def">B</a>"#;
        assert_eq!(parse_book_ids(html, 5), vec!["abc", "def"]);
    }

    #[test]
    fn resolve_manual_from_url() {
        assert_eq!(
            resolve_manual_id("https://app.thestorygraph.com/books/my-book-id"),
            Some("my-book-id".into())
        );
    }
}
