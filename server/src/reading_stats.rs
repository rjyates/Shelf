//! Reading session capture (M12 foundation): sessions, daily rollups, progression
//! history, lifecycle events, and reading prefs/goals. Charts stay in a later slice.

use super::*;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

const MIN_SESSION_SECS: i64 = 10;
const PROGRESSION_THROTTLE_SECS: i64 = 5 * 60;
const MAX_WALL_SKEW_SECS: i64 = 120;

fn owned_book(db: &Connection, user: i64, book_id: i64) -> ApiResult<()> {
    let ok: bool = db
        .query_row(
            "SELECT 1 FROM books WHERE id=? AND user_id=?",
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

fn ensure_defaults(db: &Connection, user: i64) -> ApiResult<()> {
    db.execute(
        "INSERT OR IGNORE INTO user_reading_defaults(user_id) VALUES (?)",
        params![user],
    )?;
    Ok(())
}

fn user_offset_minutes(db: &Connection, user: i64) -> ApiResult<i32> {
    ensure_defaults(db, user)?;
    Ok(db.query_row(
        "SELECT timezone_offset_minutes FROM user_reading_defaults WHERE user_id=?",
        params![user],
        |r| r.get(0),
    )?)
}

fn user_timezone_name(db: &Connection, user: i64) -> ApiResult<String> {
    ensure_defaults(db, user)?;
    Ok(db.query_row(
        "SELECT timezone FROM user_reading_defaults WHERE user_id=?",
        params![user],
        |r| r.get(0),
    )?)
}

/// Parse RFC3339 / ISO-8601 to unix seconds (UTC).
fn parse_rfc3339(s: &str) -> ApiResult<i64> {
    let s = s.trim();
    // Accept `YYYY-MM-DDTHH:MM:SS[.frac]Z` or with ±HH:MM offset.
    let (datetime, offset_secs) =
        if let Some(rest) = s.strip_suffix('Z').or_else(|| s.strip_suffix('z')) {
            (rest, 0_i64)
        } else if let Some(idx) = s.rfind(['+', '-']).filter(|&i| i > 10) {
            let (head, off) = s.split_at(idx);
            let sign = if off.starts_with('-') { -1_i64 } else { 1 };
            let digits = off[1..].replace(':', "");
            if digits.len() < 2 {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Invalid session timestamp.",
                ));
            }
            let hh: i64 = digits
                .get(0..2)
                .and_then(|x| x.parse().ok())
                .ok_or(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Invalid session timestamp.",
                ))?;
            let mm: i64 = if digits.len() >= 4 {
                digits.get(2..4).and_then(|x| x.parse().ok()).unwrap_or(0)
            } else {
                0
            };
            (head, sign * (hh * 3600 + mm * 60))
        } else {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "Invalid session timestamp.",
            ));
        };
    let datetime = datetime.split('.').next().unwrap_or(datetime);
    let parts: Vec<&str> = datetime.split('T').collect();
    if parts.len() != 2 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid session timestamp.",
        ));
    }
    let d: Vec<&str> = parts[0].split('-').collect();
    let t: Vec<&str> = parts[1].split(':').collect();
    if d.len() != 3 || t.len() < 2 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid session timestamp.",
        ));
    }
    let year: i64 = d[0]
        .parse()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Invalid session timestamp."))?;
    let month: u32 = d[1]
        .parse()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Invalid session timestamp."))?;
    let day: u32 = d[2]
        .parse()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Invalid session timestamp."))?;
    let hour: u32 = t[0]
        .parse()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Invalid session timestamp."))?;
    let minute: u32 = t[1]
        .parse()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Invalid session timestamp."))?;
    let second: u32 = t.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let days = days_from_civil(year, month, day).ok_or(ApiError(
        StatusCode::BAD_REQUEST,
        "Invalid session timestamp.",
    ))?;
    let secs = days * 86400 + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second);
    Ok(secs - offset_secs)
}

/// Howard Hinnant civil-from-days inverse (proleptic Gregorian → days since 1970-01-01).
fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || day == 0 || day > 31 {
        return None;
    }
    let mut y = year;
    let m = month as i64;
    if m <= 2 {
        y -= 1;
    }
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

fn civil_from_days(mut z: i64) -> (i64, u32, u32) {
    z += 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn local_day(unix_secs: i64, offset_minutes: i32) -> String {
    let local = unix_secs - i64::from(offset_minutes) * 60;
    let (y, m, d) = civil_from_days(local.div_euclid(86400));
    format!("{y:04}-{m:02}-{d:02}")
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn format_rfc3339(unix_secs: i64) -> String {
    let (y, m, d) = civil_from_days(unix_secs.div_euclid(86400));
    let rem = unix_secs.rem_euclid(86400) as u32;
    let h = rem / 3600;
    let mi = (rem % 3600) / 60;
    let s = rem % 60;
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn current_year() -> i32 {
    civil_from_days(now_unix().div_euclid(86400)).0 as i32
}

pub(crate) fn normalize_source(raw: Option<&str>) -> ApiResult<&'static str> {
    match raw.unwrap_or("web").trim().to_ascii_lowercase().as_str() {
        "web" => Ok("web"),
        "android" => Ok("android"),
        "ios" => Ok("ios"),
        "manual" => Ok("manual"),
        "koreader" => Ok("koreader"),
        "kobo" => Ok("kobo"),
        _ => Err(ApiError(StatusCode::BAD_REQUEST, "Invalid reading source.")),
    }
}

fn normalize_session_type(raw: Option<&str>) -> ApiResult<&'static str> {
    match raw.unwrap_or("read").trim().to_ascii_lowercase().as_str() {
        "read" => Ok("read"),
        "tts" => Ok("tts"),
        "listen" => Ok("listen"),
        _ => Err(ApiError(StatusCode::BAD_REQUEST, "Invalid session type.")),
    }
}

/// Book Orbit posts endProgress / progressDelta on a 0–100 scale.
fn as_percent_0_100(v: Option<f64>) -> Option<f64> {
    let n = v?;
    if !n.is_finite() {
        return None;
    }
    if (0.0..=1.0).contains(&n) {
        Some((n * 100.0).clamp(0.0, 100.0))
    } else {
        Some(n.clamp(0.0, 100.0))
    }
}

fn book_format(db: &Connection, book_id: i64) -> Option<String> {
    db.query_row(
        "SELECT format FROM epub_files WHERE book_id=?",
        params![book_id],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
}

pub(crate) fn record_event(
    db: &Connection,
    user: i64,
    book_id: i64,
    event_type: &str,
    source: &str,
    payload: Value,
) -> ApiResult<()> {
    db.execute(
        "INSERT INTO reading_events(user_id, book_id, event_type, source, payload_json)
         VALUES (?,?,?,?,?)",
        params![user, book_id, event_type, source, payload.to_string()],
    )?;
    Ok(())
}

/// Throttled progression history (≤1 row / 5 min / user+book+format).
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_progression(
    db: &Connection,
    user: i64,
    book_id: i64,
    progression: f64,
    position: Option<&str>,
    format: &str,
    source: &str,
    device_id: Option<&str>,
) -> ApiResult<()> {
    if !(0.0..=1.0).contains(&progression) || !progression.is_finite() {
        return Ok(());
    }
    let recent: Option<String> = db
        .query_row(
            "SELECT modified_at FROM progression_history
             WHERE user_id=? AND book_id=? AND format=?
             ORDER BY modified_at DESC LIMIT 1",
            params![user, book_id, format],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(prev) = recent {
        if let Ok(prev_secs) = parse_rfc3339(&prev) {
            if now_unix() - prev_secs < PROGRESSION_THROTTLE_SECS {
                return Ok(());
            }
        }
    }
    db.execute(
        "INSERT INTO progression_history(user_id, book_id, progression, position, format, source, device_id)
         VALUES (?,?,?,?,?,?,?)",
        params![user, book_id, progression, position, format, source, device_id],
    )?;

    // First meaningful open (≥1%).
    if progression >= 0.01 {
        let prior: i64 = db.query_row(
            "SELECT COUNT(*) FROM reading_events
             WHERE user_id=? AND book_id=? AND event_type='opened'",
            params![user, book_id],
            |r| r.get(0),
        )?;
        if prior == 0 {
            record_event(
                db,
                user,
                book_id,
                "opened",
                source,
                json!({ "progression": progression }),
            )?;
        }
    }
    Ok(())
}

fn emit_milestones(
    db: &Connection,
    user: i64,
    book_id: i64,
    source: &str,
    start_pct: Option<f64>,
    end_pct: Option<f64>,
) -> ApiResult<()> {
    let Some(end) = end_pct else {
        return Ok(());
    };
    let start = start_pct.unwrap_or(0.0);
    for threshold in [25.0_f64, 50.0, 75.0, 100.0] {
        if start < threshold && end >= threshold {
            record_event(
                db,
                user,
                book_id,
                "progress_milestone",
                source,
                json!({ "progress": threshold / 100.0 }),
            )?;
        }
    }
    Ok(())
}

fn upsert_daily(
    db: &Connection,
    user: i64,
    day_local: &str,
    source: &str,
    timezone: &str,
    duration_seconds: i64,
    progress_delta: f64,
) -> ApiResult<()> {
    db.execute(
        "INSERT INTO reading_daily_stats(
            user_id, day_local, source, reading_seconds, progress_delta_sum, session_count, timezone
         ) VALUES (?,?,?,?,?,?,?)
         ON CONFLICT(user_id, day_local, source) DO UPDATE SET
           reading_seconds = reading_seconds + excluded.reading_seconds,
           progress_delta_sum = progress_delta_sum + excluded.progress_delta_sum,
           session_count = session_count + 1,
           timezone = excluded.timezone",
        params![
            user,
            day_local,
            source,
            duration_seconds,
            progress_delta,
            1_i64,
            timezone
        ],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn recompute_daily_for_session(
    db: &Connection,
    user: i64,
    source: &str,
    timezone: &str,
    offset_minutes: i32,
    started_unix: i64,
    duration_seconds: i64,
    progress_delta: f64,
) -> ApiResult<()> {
    let day = local_day(started_unix, offset_minutes);
    upsert_daily(
        db,
        user,
        &day,
        source,
        timezone,
        duration_seconds,
        progress_delta,
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SessionBody {
    session_id: String,
    started_at: String,
    ended_at: String,
    duration_seconds: i64,
    #[serde(default)]
    progress_delta: Option<f64>,
    #[serde(default)]
    end_progress: Option<f64>,
    #[serde(default)]
    start_progress: Option<f64>,
    #[serde(default)]
    session_type: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    chars_delta: Option<i64>,
    #[serde(default)]
    pages_delta: Option<i64>,
    #[serde(default)]
    device_id: Option<String>,
    #[serde(default)]
    device_name: Option<String>,
    #[serde(default)]
    timezone_offset_minutes: Option<i32>,
}

fn save_session(
    db: &Connection,
    user: i64,
    book_id: i64,
    body: &SessionBody,
) -> ApiResult<StatusCode> {
    owned_book(db, user, book_id)?;
    let client_id = body.session_id.trim();
    if client_id.is_empty() || client_id.len() > 80 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "sessionId is required."));
    }
    if body.duration_seconds < MIN_SESSION_SECS {
        return Ok(StatusCode::NO_CONTENT);
    }
    let started = parse_rfc3339(&body.started_at)?;
    let ended = parse_rfc3339(&body.ended_at)?;
    if ended < started {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "endedAt must be after startedAt.",
        ));
    }
    let wall = (ended - started).max(0);
    if body.duration_seconds > wall + MAX_WALL_SKEW_SECS {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "durationSeconds exceeds wall-clock span.",
        ));
    }
    let session_type = normalize_session_type(body.session_type.as_deref())?;
    let source = normalize_source(body.source.as_deref())?;
    let end_pct = as_percent_0_100(body.end_progress);
    let delta = body.progress_delta.filter(|n| n.is_finite());
    let start_pct = as_percent_0_100(body.start_progress).or_else(|| match (end_pct, delta) {
        (Some(e), Some(d)) => Some((e - d).clamp(0.0, 100.0)),
        _ => None,
    });
    let progress_delta = delta.or(match (end_pct, start_pct) {
        (Some(e), Some(s)) => Some(e - s),
        _ => None,
    });
    let offset = body
        .timezone_offset_minutes
        .unwrap_or(user_offset_minutes(db, user)?);
    if body.timezone_offset_minutes.is_some() {
        ensure_defaults(db, user)?;
        db.execute(
            "UPDATE user_reading_defaults
             SET timezone_offset_minutes=?, updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE user_id=?",
            params![offset, user],
        )?;
    }
    let timezone = user_timezone_name(db, user)?;
    let format = body
        .format
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| book_format(db, book_id));

    let existing: Option<i64> = db
        .query_row(
            "SELECT id FROM reading_sessions WHERE user_id=? AND client_session_id=?",
            params![user, client_id],
            |r| r.get(0),
        )
        .optional()?;

    if let Some(id) = existing {
        db.execute(
            "UPDATE reading_sessions SET
                book_id=?, started_at=?, ended_at=?, duration_seconds=?,
                progress_start=?, progress_end=?, progress_delta=?,
                session_type=?, source=?, format=?,
                chars_delta=?, pages_delta=?, device_id=?, device_name=?,
                timezone_offset_minutes=?
             WHERE id=?",
            params![
                book_id,
                format_rfc3339(started),
                format_rfc3339(ended),
                body.duration_seconds,
                start_pct,
                end_pct,
                progress_delta,
                session_type,
                source,
                format,
                body.chars_delta,
                body.pages_delta,
                body.device_id,
                body.device_name,
                offset,
                id
            ],
        )?;
    } else {
        db.execute(
            "INSERT INTO reading_sessions(
                user_id, book_id, client_session_id, started_at, ended_at, duration_seconds,
                progress_start, progress_end, progress_delta, session_type, source, format,
                chars_delta, pages_delta, device_id, device_name, timezone_offset_minutes
             ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                user,
                book_id,
                client_id,
                format_rfc3339(started),
                format_rfc3339(ended),
                body.duration_seconds,
                start_pct,
                end_pct,
                progress_delta,
                session_type,
                source,
                format,
                body.chars_delta,
                body.pages_delta,
                body.device_id,
                body.device_name,
                offset
            ],
        )?;
        recompute_daily_for_session(
            db,
            user,
            source,
            &timezone,
            offset,
            started,
            body.duration_seconds,
            progress_delta.unwrap_or(0.0),
        )?;
        emit_milestones(db, user, book_id, source, start_pct, end_pct)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

fn session_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_, i64>(0)?,
        "bookId": row.get::<_, i64>(1)?,
        "sessionId": row.get::<_, String>(2)?,
        "startedAt": row.get::<_, String>(3)?,
        "endedAt": row.get::<_, String>(4)?,
        "durationSeconds": row.get::<_, i64>(5)?,
        "progressStart": row.get::<_, Option<f64>>(6)?,
        "progressEnd": row.get::<_, Option<f64>>(7)?,
        "progressDelta": row.get::<_, Option<f64>>(8)?,
        "sessionType": row.get::<_, String>(9)?,
        "source": row.get::<_, String>(10)?,
        "format": row.get::<_, Option<String>>(11)?,
        "charsDelta": row.get::<_, Option<i64>>(12)?,
        "pagesDelta": row.get::<_, Option<i64>>(13)?,
        "deviceId": row.get::<_, Option<String>>(14)?,
        "deviceName": row.get::<_, Option<String>>(15)?,
        "timezoneOffsetMinutes": row.get::<_, i32>(16)?,
        "createdAt": row.get::<_, String>(17)?,
    }))
}

const SESSION_SELECT: &str = "SELECT id, book_id, client_session_id, started_at, ended_at,
    duration_seconds, progress_start, progress_end, progress_delta, session_type, source, format,
    chars_delta, pages_delta, device_id, device_name, timezone_offset_minutes, created_at
    FROM reading_sessions";

pub(super) async fn post_file_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<SessionBody>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            save_session(db, user, id, &body)
        })
        .await
}

pub(super) async fn post_book_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<SessionBody>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            save_session(db, user, id, &body)
        })
        .await
}

pub(super) async fn list_book_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, id)?;
            let mut stmt = db.prepare(&format!(
                "{SESSION_SELECT} WHERE user_id=? AND book_id=? ORDER BY ended_at DESC LIMIT 100"
            ))?;
            let rows = stmt
                .query_map(params![user, id], session_json)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Json(json!(rows)))
        })
        .await
}

pub(super) async fn delete_book_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, client_session_id)): Path<(i64, String)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, id)?;
            let changed = db.execute(
                "DELETE FROM reading_sessions WHERE user_id=? AND book_id=? AND client_session_id=?",
                params![user, id, client_session_id.trim()],
            )?;
            if changed == 0 {
                return Err(ApiError(StatusCode::NOT_FOUND, "Session not found."));
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

#[derive(Deserialize)]
pub(super) struct ListQuery {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    before: Option<String>,
}

fn default_limit() -> i64 {
    50
}

pub(super) async fn list_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let limit = q.limit.clamp(1, 200);
            let rows = if let Some(before) = q.before.as_deref().map(str::trim).filter(|s| !s.is_empty())
            {
                let mut stmt = db.prepare(&format!(
                    "{SESSION_SELECT} WHERE user_id=? AND ended_at < ? ORDER BY ended_at DESC LIMIT ?"
                ))?;
                let mapped = stmt.query_map(params![user, before, limit], session_json)?;
                mapped.collect::<Result<Vec<_>, _>>()?
            } else {
                let mut stmt = db.prepare(&format!(
                    "{SESSION_SELECT} WHERE user_id=? ORDER BY ended_at DESC LIMIT ?"
                ))?;
                let mapped = stmt.query_map(params![user, limit], session_json)?;
                mapped.collect::<Result<Vec<_>, _>>()?
            };
            Ok(Json(json!(rows)))
        })
        .await
}

#[derive(Deserialize)]
pub(super) struct DailyQuery {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
}

pub(super) async fn list_daily(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<DailyQuery>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let from = q.from.unwrap_or_else(|| "1970-01-01".into());
            let to = q.to.unwrap_or_else(|| "2100-12-31".into());
            let mut stmt = db.prepare(
                "SELECT day_local, source, reading_seconds, progress_delta_sum, session_count, timezone
                 FROM reading_daily_stats
                 WHERE user_id=? AND day_local >= ? AND day_local <= ?
                 ORDER BY day_local ASC, source ASC",
            )?;
            let rows = stmt
                .query_map(params![user, from, to], |r| {
                    Ok(json!({
                        "day": r.get::<_, String>(0)?,
                        "source": r.get::<_, String>(1)?,
                        "readingSeconds": r.get::<_, i64>(2)?,
                        "progressDeltaSum": r.get::<_, f64>(3)?,
                        "sessionCount": r.get::<_, i64>(4)?,
                        "timezone": r.get::<_, String>(5)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Json(json!(rows)))
        })
        .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PrefsBody {
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    timezone_offset_minutes: Option<i32>,
    #[serde(default)]
    annual_goal_books: Option<i64>,
    #[serde(default)]
    year: Option<i32>,
}

pub(super) async fn get_prefs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            ensure_defaults(db, user)?;
            let (tz, offset): (String, i32) = db.query_row(
                "SELECT timezone, timezone_offset_minutes FROM user_reading_defaults WHERE user_id=?",
                params![user],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let year = current_year();
            let goal: Option<i64> = db
                .query_row(
                    "SELECT target_books FROM reading_goals WHERE user_id=? AND year=?",
                    params![user, year],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(Json(json!({
                "timezone": tz,
                "timezoneOffsetMinutes": offset,
                "year": year,
                "annualGoalBooks": goal,
            })))
        })
        .await
}

pub(super) async fn put_prefs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PrefsBody>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            ensure_defaults(db, user)?;
            if let Some(tz) = body.timezone.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                if tz.len() > 64 {
                    return Err(ApiError(StatusCode::BAD_REQUEST, "timezone is too long."));
                }
                db.execute(
                    "UPDATE user_reading_defaults
                     SET timezone=?, updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
                     WHERE user_id=?",
                    params![tz, user],
                )?;
            }
            if let Some(offset) = body.timezone_offset_minutes {
                if !(-14 * 60..=14 * 60).contains(&offset) {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "timezoneOffsetMinutes out of range.",
                    ));
                }
                db.execute(
                    "UPDATE user_reading_defaults
                     SET timezone_offset_minutes=?, updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
                     WHERE user_id=?",
                    params![offset, user],
                )?;
            }
            if let Some(target) = body.annual_goal_books {
                if !(0..=10_000).contains(&target) {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "annualGoalBooks out of range.",
                    ));
                }
                let year = body.year.unwrap_or_else(current_year);
                if !(1970..=2100).contains(&year) {
                    return Err(ApiError(StatusCode::BAD_REQUEST, "year out of range."));
                }
                db.execute(
                    "INSERT INTO reading_goals(user_id, year, target_books) VALUES (?,?,?)
                     ON CONFLICT(user_id, year) DO UPDATE SET target_books=excluded.target_books",
                    params![user, year, target],
                )?;
            }
            let (tz, offset): (String, i32) = db.query_row(
                "SELECT timezone, timezone_offset_minutes FROM user_reading_defaults WHERE user_id=?",
                params![user],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let year = body.year.unwrap_or_else(current_year);
            let goal: Option<i64> = db
                .query_row(
                    "SELECT target_books FROM reading_goals WHERE user_id=? AND year=?",
                    params![user, year],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(Json(json!({
                "timezone": tz,
                "timezoneOffsetMinutes": offset,
                "year": year,
                "annualGoalBooks": goal,
            })))
        })
        .await
}

pub(crate) fn migrate(db: &Connection) -> rusqlite::Result<()> {
    let _ = db
        .execute_batch("CREATE TABLE IF NOT EXISTS schema_flags(flag TEXT PRIMARY KEY NOT NULL);");
    let _ = db.execute(
        "INSERT OR IGNORE INTO schema_flags(flag) VALUES ('reading_stats_capture_v1')",
        [],
    );
    Ok(())
}
