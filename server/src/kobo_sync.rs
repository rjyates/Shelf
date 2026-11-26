//! Kobo store sync (M12 slice 13).
//!
//! BO reference @ c77966cc: device tokens, initialization, library/sync,
//! reading state, download. Shelf ships that core path for EPUB/PDF.
//! Kepub conversion, store-proxy merge, annotation push/pull, sync history,
//! and thumbnail templates stay deferred.

use super::*;
use serde_json::{json, Map, Value};

const TOKEN_BYTES: usize = 24;
const SYNC_PAGE_SIZE: usize = 50;

pub(crate) fn migrate(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS kobo_settings (
           user_id INTEGER PRIMARY KEY REFERENCES users(id),
           reading_threshold REAL NOT NULL DEFAULT 0.05,
           finished_threshold REAL NOT NULL DEFAULT 0.95,
           two_way_progress_sync INTEGER NOT NULL DEFAULT 1,
           store_sync INTEGER NOT NULL DEFAULT 0,
           updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         );
         CREATE TABLE IF NOT EXISTS kobo_devices (
           id INTEGER PRIMARY KEY,
           user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
           name TEXT NOT NULL,
           token TEXT NOT NULL UNIQUE,
           last_seen_at TEXT,
           created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         );
         CREATE INDEX IF NOT EXISTS kobo_devices_user ON kobo_devices(user_id);
         CREATE TABLE IF NOT EXISTS kobo_sync_cursor (
           user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
           device_id INTEGER NOT NULL REFERENCES kobo_devices(id) ON DELETE CASCADE,
           next_offset INTEGER NOT NULL DEFAULT 0,
           updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
           PRIMARY KEY (user_id, device_id)
         );
         CREATE TABLE IF NOT EXISTS kobo_reading_state (
           user_id INTEGER NOT NULL REFERENCES users(id),
           device_id INTEGER NOT NULL REFERENCES kobo_devices(id) ON DELETE CASCADE,
           book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
           current_bookmark TEXT,
           status_info TEXT,
           statistics TEXT,
           updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
           PRIMARY KEY (user_id, device_id, book_id)
         )",
    )?;
    Ok(())
}

fn ensure_settings(db: &Connection, user: i64) -> ApiResult<()> {
    db.execute(
        "INSERT OR IGNORE INTO kobo_settings(user_id) VALUES (?)",
        params![user],
    )?;
    Ok(())
}

fn random_token() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Deterministic UUID-shaped entitlement id from a shelf book id.
fn entitlement_id(book_id: i64) -> String {
    format!("00000000-0000-4000-8000-{book_id:012x}")
}

fn parse_entitlement_id(id: &str) -> Option<i64> {
    let id = id.trim().to_ascii_lowercase();
    let suffix = id.strip_prefix("00000000-0000-4000-8000-")?;
    i64::from_str_radix(suffix, 16).ok()
}

struct DeviceRow {
    id: i64,
    user_id: i64,
    #[allow(dead_code)]
    name: String,
    token: String,
}

fn load_device_by_token(db: &Connection, token: &str) -> ApiResult<DeviceRow> {
    if token.is_empty() || token.len() > 128 {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "Invalid device token."));
    }
    db.query_row(
        "SELECT id, user_id, name, token FROM kobo_devices WHERE token=?",
        params![token],
        |r| {
            Ok(DeviceRow {
                id: r.get(0)?,
                user_id: r.get(1)?,
                name: r.get(2)?,
                token: r.get(3)?,
            })
        },
    )
    .optional()?
    .ok_or(ApiError(StatusCode::UNAUTHORIZED, "Invalid device token."))
}

fn touch_device(db: &Connection, device_id: i64) -> ApiResult<()> {
    db.execute(
        "UPDATE kobo_devices SET last_seen_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=?",
        params![device_id],
    )?;
    Ok(())
}

fn settings_json(db: &Connection, user: i64) -> ApiResult<Value> {
    ensure_settings(db, user)?;
    db.query_row(
        "SELECT reading_threshold, finished_threshold, two_way_progress_sync, store_sync
         FROM kobo_settings WHERE user_id=?",
        params![user],
        |r| {
            Ok(json!({
                "readingThreshold": r.get::<_, f64>(0)?,
                "finishedThreshold": r.get::<_, f64>(1)?,
                "twoWayProgressSync": r.get::<_, i64>(2)? != 0,
                "storeSync": r.get::<_, i64>(3)? != 0,
                // shortcut: kepub + annotation sync deferred
                "convertToKepub": false,
                "syncBookOrbitAnnotationsToKobo": false,
            }))
        },
    )
    .map_err(ApiError::from)
}

fn list_devices_json(db: &Connection, user: i64) -> ApiResult<Vec<Value>> {
    let mut stmt = db.prepare(
        "SELECT id, name, token, last_seen_at, created_at FROM kobo_devices
         WHERE user_id=? ORDER BY id",
    )?;
    let rows = stmt
        .query_map(params![user], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "token": r.get::<_, String>(2)?,
                "lastSeenAt": r.get::<_, Option<String>>(3)?,
                "createdAt": r.get::<_, String>(4)?,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn request_base(headers: &HeaderMap) -> String {
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("127.0.0.1:8080");
    format!("{proto}://{host}")
}

#[allow(clippy::too_many_arguments)] // Kobo entitlement payload fields stay positional
fn build_entitlement(
    book_id: i64,
    title: &str,
    author: &str,
    format: &str,
    percentage: f64,
    reading_status: &str,
    created_at: &str,
    device_token: &str,
    base: &str,
) -> Value {
    let id = entitlement_id(book_id);
    let mime = match format.to_ascii_lowercase().as_str() {
        "pdf" => "application/pdf",
        _ => "application/epub+zip",
    };
    let download = format!("{base}/api/v1/kobo/{device_token}/v1/library/{id}/download");
    let status = if reading_status == "finished" {
        "Finished"
    } else if percentage > 0.0 {
        "Reading"
    } else {
        "ReadyToRead"
    };
    json!({
        "NewEntitlement": {
            "BookEntitlement": {
                "Accessibility": "Full",
                "ActivePeriod": { "From": created_at },
                "Created": created_at,
                "CrossRevisionId": id,
                "Id": id,
                "IsRemoved": false,
                "IsHiddenFromArchive": false,
                "IsLocked": false,
                "LastModified": created_at,
                "OriginCategory": "Imported",
                "RevisionId": id,
                "Status": "Active"
            },
            "BookMetadata": {
                "CrossRevisionId": id,
                "RevisionId": id,
                "EntitlementId": id,
                "WorkId": id,
                "Title": title,
                "Description": "",
                "Language": "en",
                "Contributors": if author.is_empty() { Vec::<String>::new() } else { vec![author.to_string()] },
                "ContributorRoles": if author.is_empty() {
                    Vec::<Value>::new()
                } else {
                    vec![json!({ "Name": author, "Role": "Author" })]
                },
                "CoverImageId": id,
                "Categories": ["00000000-0000-0000-0000-000000000001"],
                "DownloadUrls": [{
                    "Format": if format.eq_ignore_ascii_case("pdf") { "PDF" } else { "EPUB3" },
                    "Size": 0,
                    "Url": download,
                    "Platform": "Generic",
                    "DrmType": "None"
                }],
                "IsSocialEnabled": false,
                "IsDownloadable": true,
                "MimeType": mime
            },
            "ReadingState": {
                "EntitlementId": id,
                "Created": created_at,
                "LastModified": created_at,
                "PriorityTimestamp": created_at,
                "StatusInfo": {
                    "LastModified": created_at,
                    "Status": status,
                    "TimesStartedReading": if percentage > 0.0 { 1 } else { 0 }
                },
                "Statistics": {
                    "LastModified": created_at,
                    "SpentReadingMinutes": 0,
                    "RemainingTimeMinutes": 0
                },
                "CurrentBookmark": {
                    "LastModified": created_at,
                    "ProgressPercent": (percentage * 100.0).clamp(0.0, 100.0),
                    "ContentSourceProgressPercent": (percentage * 100.0).clamp(0.0, 100.0)
                }
            }
        }
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KoboSettingsBody {
    reading_threshold: Option<f64>,
    finished_threshold: Option<f64>,
    two_way_progress_sync: Option<bool>,
    store_sync: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateDeviceBody {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenameDeviceBody {
    name: String,
}

pub(super) async fn get_kobo_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let settings = settings_json(db, user)?;
            let devices = list_devices_json(db, user)?;
            Ok(Json(json!({
                "settings": settings,
                "devices": devices,
            })))
        })
        .await
}

pub(super) async fn put_kobo_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<KoboSettingsBody>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            ensure_settings(db, user)?;
            let mut settings = settings_json(db, user)?;
            if let Some(v) = body.reading_threshold {
                if !(0.0..=1.0).contains(&v) {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "readingThreshold must be 0–1.",
                    ));
                }
                settings["readingThreshold"] = json!(v);
            }
            if let Some(v) = body.finished_threshold {
                if !(0.0..=1.0).contains(&v) {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "finishedThreshold must be 0–1.",
                    ));
                }
                settings["finishedThreshold"] = json!(v);
            }
            if let Some(v) = body.two_way_progress_sync {
                settings["twoWayProgressSync"] = json!(v);
            }
            if let Some(v) = body.store_sync {
                // shortcut: store proxy merge not implemented; flag stored for UI
                settings["storeSync"] = json!(v);
            }
            db.execute(
                "UPDATE kobo_settings SET
                   reading_threshold=?,
                   finished_threshold=?,
                   two_way_progress_sync=?,
                   store_sync=?,
                   updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE user_id=?",
                params![
                    settings["readingThreshold"].as_f64().unwrap_or(0.05),
                    settings["finishedThreshold"].as_f64().unwrap_or(0.95),
                    if settings["twoWayProgressSync"].as_bool().unwrap_or(true) {
                        1
                    } else {
                        0
                    },
                    if settings["storeSync"].as_bool().unwrap_or(false) {
                        1
                    } else {
                        0
                    },
                    user
                ],
            )?;
            Ok(Json(settings_json(db, user)?))
        })
        .await
}

pub(super) async fn create_kobo_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateDeviceBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let name = body.name.trim().to_string();
    if name.is_empty() || name.len() > 100 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid device name."));
    }
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let token = random_token();
            db.execute(
                "INSERT INTO kobo_devices(user_id, name, token) VALUES (?,?,?)",
                params![user, name, token],
            )?;
            let id = db.last_insert_rowid();
            let row = db.query_row(
                "SELECT id, name, token, last_seen_at, created_at FROM kobo_devices WHERE id=?",
                params![id],
                |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "token": r.get::<_, String>(2)?,
                        "lastSeenAt": r.get::<_, Option<String>>(3)?,
                        "createdAt": r.get::<_, String>(4)?,
                    }))
                },
            )?;
            Ok((StatusCode::CREATED, Json(row)))
        })
        .await
}

pub(super) async fn rename_kobo_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<RenameDeviceBody>,
) -> ApiResult<Json<Value>> {
    let name = body.name.trim().to_string();
    if name.is_empty() || name.len() > 100 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid device name."));
    }
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let n = db.execute(
                "UPDATE kobo_devices SET name=? WHERE id=? AND user_id=?",
                params![name, id, user],
            )?;
            if n == 0 {
                return Err(ApiError(StatusCode::NOT_FOUND, "Device not found."));
            }
            Ok(Json(json!({ "id": id, "name": name })))
        })
        .await
}

pub(super) async fn delete_kobo_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let n = db.execute(
                "DELETE FROM kobo_devices WHERE id=? AND user_id=?",
                params![id, user],
            )?;
            if n == 0 {
                return Err(ApiError(StatusCode::NOT_FOUND, "Device not found."));
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn device_initialization(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(device_token): Path<String>,
) -> ApiResult<Json<Value>> {
    let base = request_base(&headers);
    state
        .db(move |db| {
            let device = load_device_by_token(db, &device_token)?;
            touch_device(db, device.id)?;
            let t = &device.token;
            let mut resources = Map::new();
            // Keep device auth pointed at Kobo so optional store sync can use real credentials later.
            resources.insert(
                "device_auth".into(),
                json!("https://storeapi.kobo.com/v1/auth/device"),
            );
            resources.insert(
                "device_refresh".into(),
                json!("https://storeapi.kobo.com/v1/auth/refresh"),
            );
            resources.insert(
                "library_sync".into(),
                json!(format!("{base}/api/v1/kobo/{t}/v1/library/sync")),
            );
            resources.insert(
                "reading_state".into(),
                json!(format!("{base}/api/v1/kobo/{t}/v1/library/{{Ids}}/state")),
            );
            resources.insert(
                "content_access_book".into(),
                json!(format!(
                    "{base}/api/v1/kobo/{t}/v1/products/books/{{ProductId}}/access"
                )),
            );
            resources.insert("image_host".into(), json!(base.clone()));
            resources.insert(
                "image_url_template".into(),
                json!(format!(
                    "{base}/api/v1/kobo/{t}/v1/books/{{ImageId}}/thumbnail/{{Width}}/{{Height}}/false/image.jpg"
                )),
            );
            Ok(Json(json!({
                "Resources": resources,
            })))
        })
        .await
}

pub(super) async fn device_library_sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(device_token): Path<String>,
) -> ApiResult<Response> {
    let base = request_base(&headers);
    state
        .db(move |db| {
            let device = load_device_by_token(db, &device_token)?;
            touch_device(db, device.id)?;
            db.execute(
                "INSERT OR IGNORE INTO kobo_sync_cursor(user_id, device_id) VALUES (?,?)",
                params![device.user_id, device.id],
            )?;
            let offset: i64 = db.query_row(
                "SELECT next_offset FROM kobo_sync_cursor WHERE user_id=? AND device_id=?",
                params![device.user_id, device.id],
                |r| r.get(0),
            )?;
            let mut stmt = db.prepare(
                "SELECT b.id, b.title, coalesce(b.author,''), b.reading_status,
                        coalesce(f.percentage, 0), coalesce(f.format,'epub')
                 FROM books b
                 JOIN epub_files f ON f.book_id=b.id
                 WHERE b.user_id=? AND b.merged_into IS NULL
                   AND lower(f.format) IN ('epub','pdf')
                 ORDER BY b.id
                 LIMIT ? OFFSET ?",
            )?;
            let now: String = db.query_row(
                "SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')",
                [],
                |r| r.get(0),
            )?;
            let rows = stmt
                .query_map(
                    params![device.user_id, SYNC_PAGE_SIZE as i64, offset],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, f64>(4)?,
                            r.get::<_, String>(5)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            let page_len = rows.len();
            let entitlements: Vec<Value> = rows
                .into_iter()
                .map(|(id, title, author, status, pct, format)| {
                    build_entitlement(
                        id,
                        &title,
                        &author,
                        &format,
                        pct,
                        &status,
                        &now,
                        &device.token,
                        &base,
                    )
                })
                .collect();
            let has_more = page_len == SYNC_PAGE_SIZE;
            let next_offset = if has_more {
                offset + page_len as i64
            } else {
                0
            };
            db.execute(
                "UPDATE kobo_sync_cursor SET next_offset=?, updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE user_id=? AND device_id=?",
                params![next_offset, device.user_id, device.id],
            )?;
            let mut response = Json(json!(entitlements)).into_response();
            response.headers_mut().insert(
                "x-kobo-sync",
                header::HeaderValue::from_static(if has_more { "continue" } else { "done" }),
            );
            // shortcut: opaque sync token / store merge deferred
            Ok(response)
        })
        .await
}

pub(super) async fn device_get_reading_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((device_token, book_key)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let _ = headers;
    state
        .db(move |db| {
            let device = load_device_by_token(db, &device_token)?;
            touch_device(db, device.id)?;
            let book_id = parse_entitlement_id(&book_key)
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Book not found."))?;
            let owned: bool = db
                .query_row(
                    "SELECT 1 FROM books WHERE id=? AND user_id=?",
                    params![book_id, device.user_id],
                    |_| Ok(true),
                )
                .optional()?
                .unwrap_or(false);
            if !owned {
                return Err(ApiError(StatusCode::NOT_FOUND, "Book not found."));
            }
            let row = db
                .query_row(
                    "SELECT current_bookmark, status_info, statistics, updated_at
                     FROM kobo_reading_state
                     WHERE user_id=? AND device_id=? AND book_id=?",
                    params![device.user_id, device.id, book_id],
                    |r| {
                        Ok(json!({
                            "EntitlementId": entitlement_id(book_id),
                            "CurrentBookmark": serde_json::from_str::<Value>(&r.get::<_, String>(0)?).unwrap_or(json!({})),
                            "StatusInfo": serde_json::from_str::<Value>(&r.get::<_, String>(1)?).unwrap_or(json!({})),
                            "Statistics": serde_json::from_str::<Value>(&r.get::<_, String>(2)?).unwrap_or(json!({})),
                            "LastModified": r.get::<_, String>(3)?,
                        }))
                    },
                )
                .optional()?;
            Ok(Json(json!(row.map(|r| vec![r]).unwrap_or_default())))
        })
        .await
}

pub(super) async fn device_put_reading_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((device_token, book_key)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    let _ = headers;
    state
        .db(move |db| {
            let device = load_device_by_token(db, &device_token)?;
            touch_device(db, device.id)?;
            let book_id = parse_entitlement_id(&book_key)
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Book not found."))?;
            let owned: bool = db
                .query_row(
                    "SELECT 1 FROM books WHERE id=? AND user_id=?",
                    params![book_id, device.user_id],
                    |_| Ok(true),
                )
                .optional()?
                .unwrap_or(false);
            if !owned {
                return Err(ApiError(StatusCode::NOT_FOUND, "Book not found."));
            }
            let state_obj = body
                .get("ReadingStates")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .cloned()
                .unwrap_or(body.clone());
            let bookmark = state_obj
                .get("CurrentBookmark")
                .cloned()
                .unwrap_or(json!({}));
            let status_info = state_obj.get("StatusInfo").cloned().unwrap_or(json!({}));
            let statistics = state_obj.get("Statistics").cloned().unwrap_or(json!({}));
            let bookmark_s = serde_json::to_string(&bookmark).unwrap_or_else(|_| "{}".into());
            let status_s = serde_json::to_string(&status_info).unwrap_or_else(|_| "{}".into());
            let stats_s = serde_json::to_string(&statistics).unwrap_or_else(|_| "{}".into());
            db.execute(
                "INSERT INTO kobo_reading_state(user_id, device_id, book_id, current_bookmark, status_info, statistics)
                 VALUES (?,?,?,?,?,?)
                 ON CONFLICT(user_id, device_id, book_id) DO UPDATE SET
                   current_bookmark=excluded.current_bookmark,
                   status_info=excluded.status_info,
                   statistics=excluded.statistics,
                   updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')",
                params![device.user_id, device.id, book_id, bookmark_s, status_s, stats_s],
            )?;

            let settings = settings_json(db, device.user_id)?;
            if settings["twoWayProgressSync"].as_bool().unwrap_or(true) {
                let pct = bookmark
                    .get("ProgressPercent")
                    .or_else(|| bookmark.get("ContentSourceProgressPercent"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(-1.0);
                if (0.0..=100.0).contains(&pct) {
                    session_injectors::apply_progress(
                        db,
                        device.user_id,
                        book_id,
                        pct,
                        "kobo",
                        Some(&device.token),
                    )?;
                }
            }
            Ok(Json(json!({
                "EntitlementId": entitlement_id(book_id),
                "CurrentBookmark": bookmark,
                "StatusInfo": status_info,
                "Statistics": statistics,
            })))
        })
        .await
}

pub(super) async fn device_download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((device_token, book_key)): Path<(String, String)>,
) -> Response {
    let data_dir = state.data_dir.clone();
    let row = match state
        .db({
            let headers = headers.clone();
            move |db| {
                let _ = headers;
                let device = load_device_by_token(db, &device_token)?;
                touch_device(db, device.id)?;
                let book_id = parse_entitlement_id(&book_key)
                    .ok_or(ApiError(StatusCode::NOT_FOUND, "Book not found."))?;
                let row: (Option<Vec<u8>>, Option<String>, String) = db
                    .query_row(
                        "SELECT f.content, f.storage_path, f.format
                         FROM books b JOIN epub_files f ON f.book_id=b.id
                         WHERE b.id=? AND b.user_id=?",
                        params![book_id, device.user_id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional()?
                    .ok_or(ApiError(StatusCode::NOT_FOUND, "Book not found."))?;
                Ok(row)
            }
        })
        .await
    {
        Ok(row) => row,
        Err(e) => return e.into_response(),
    };
    let (inline, storage_path, format) = row;
    let (mime, disposition) = match format.to_ascii_lowercase().as_str() {
        "pdf" => ("application/pdf", "attachment; filename=book.pdf"),
        _ => ("application/epub+zip", "attachment; filename=book.epub"),
    };
    if let Some(relative) = storage_path.filter(|p| !p.is_empty()) {
        if relative.contains("..") || PathBuf::from(&relative).is_absolute() {
            return ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid on-disk book path.",
            )
            .into_response();
        }
        return match stream_book_file(
            data_dir.join(relative),
            mime,
            disposition,
            headers.get(header::RANGE),
        )
        .await
        {
            Ok(resp) => resp,
            Err(e) => e.into_response(),
        };
    }
    let bytes = inline.unwrap_or_default();
    let mut response = (
        [
            (header::CONTENT_TYPE, mime),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        bytes,
    )
        .into_response();
    response.headers_mut().insert(
        header::ACCEPT_RANGES,
        header::HeaderValue::from_static("bytes"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entitlement_round_trip() {
        let id = entitlement_id(42);
        assert_eq!(parse_entitlement_id(&id), Some(42));
        assert!(parse_entitlement_id("not-a-uuid").is_none());
    }
}
