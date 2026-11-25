//! External reading-session injectors (M12 slices 5–6).
//!
//! KOReader page-stat events → `reading_sessions` (`source=koreader`).
//! Kobo SpentReadingMinutes counter → `source=kobo` sessions.
//! Algorithms follow Book Orbit @ c77966cc (Rust rewrite — do not vendor Nest).

use super::*;
use reading_stats::{format_rfc3339, parse_rfc3339, SessionBody};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const KOREADER_SESSION_GAP_SECONDS: i64 = 1800;
const KOREADER_MAX_EVENT_DURATION_SECONDS: i64 = 86400;
const KOREADER_MIN_SESSION_SECONDS: i64 = 10;
const KOREADER_MAX_EVENTS: usize = 10_000;

const KOBO_STATISTICS_CURSOR_SOURCE: &str = "kobo-statistics";
const KOBO_STATISTICS_SESSION_ID_PREFIX: &str = "kst:";

#[derive(Clone, Debug)]
struct KoreaderPageEvent {
    page: i64,
    start_time: i64,
    duration_seconds: i64,
    total_pages: i64,
}

#[derive(Clone, Debug)]
struct DerivedKoreaderSession {
    session_id: String,
    started_at: i64,
    ended_at: i64,
    duration_seconds: i64,
    progress_delta: Option<f64>,
    end_progress: Option<f64>,
    pages_delta: Option<i64>,
}

fn koreader_device_key(device_id: &str) -> String {
    let dig = Sha256::digest(device_id.as_bytes());
    let mut out = String::from("v2-");
    for b in dig.iter().take(16) {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn build_koreader_session_id(device_id: &str, book_id: i64, cluster_start: i64) -> String {
    format!(
        "kor:{}:{}:{}",
        koreader_device_key(device_id),
        book_id,
        cluster_start
    )
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn cluster_page_stats(
    events: &[KoreaderPageEvent],
    gap_seconds: i64,
) -> Vec<Vec<KoreaderPageEvent>> {
    let mut sorted = events.to_vec();
    sorted.sort_by(|a, b| {
        a.start_time
            .cmp(&b.start_time)
            .then_with(|| a.page.cmp(&b.page))
    });
    let mut clusters: Vec<Vec<KoreaderPageEvent>> = Vec::new();
    let mut current: Vec<KoreaderPageEvent> = Vec::new();
    let mut cluster_end = 0_i64;
    for event in sorted {
        if !current.is_empty() && event.start_time - cluster_end > gap_seconds {
            clusters.push(std::mem::take(&mut current));
        }
        if current.is_empty() {
            cluster_end = event.start_time + event.duration_seconds;
        } else {
            cluster_end = cluster_end.max(event.start_time + event.duration_seconds);
        }
        current.push(event);
    }
    if !current.is_empty() {
        clusters.push(current);
    }
    clusters
}

fn compute_cluster_metrics(
    cluster: &[KoreaderPageEvent],
    device_id: &str,
    book_id: i64,
) -> DerivedKoreaderSession {
    let first = &cluster[0];
    let mut end_epoch = first.start_time;
    let mut duration_sum = 0_i64;
    let mut last = first;
    for event in cluster {
        duration_sum += event.duration_seconds;
        end_epoch = end_epoch.max(event.start_time + event.duration_seconds);
        if event.start_time >= last.start_time {
            last = event;
        }
    }
    let wall = (end_epoch - first.start_time).max(0);
    let duration_seconds = duration_sum.min(wall);
    let end_progress = if last.total_pages > 0 {
        Some(round2((last.page as f64 / last.total_pages as f64) * 100.0).clamp(0.0, 100.0))
    } else {
        None
    };
    let progress_delta = if last.total_pages > 0 {
        Some(
            round2(((last.page - first.page) as f64 / last.total_pages as f64) * 100.0)
                .clamp(-100.0, 100.0),
        )
    } else {
        None
    };
    DerivedKoreaderSession {
        session_id: build_koreader_session_id(device_id, book_id, first.start_time),
        started_at: first.start_time,
        ended_at: end_epoch,
        duration_seconds,
        progress_delta,
        end_progress,
        pages_delta: Some(last.page - first.page),
    }
}

fn derive_koreader_sessions(
    events: &[KoreaderPageEvent],
    device_id: &str,
    book_id: i64,
) -> Vec<DerivedKoreaderSession> {
    cluster_page_stats(events, KOREADER_SESSION_GAP_SECONDS)
        .into_iter()
        .map(|c| compute_cluster_metrics(&c, device_id, book_id))
        .filter(|s| s.duration_seconds >= KOREADER_MIN_SESSION_SECONDS)
        .collect()
}

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

fn apply_progress(
    db: &Connection,
    user: i64,
    book_id: i64,
    end_progress_0_100: f64,
    source: &str,
    device_id: Option<&str>,
) -> ApiResult<()> {
    let pct = (end_progress_0_100 / 100.0).clamp(0.0, 1.0);
    let changed = db.execute(
        "UPDATE epub_files SET percentage=? WHERE book_id=? AND book_id IN (SELECT id FROM books WHERE user_id=?)",
        params![pct, book_id, user],
    )?;
    if changed == 0 {
        return Ok(());
    }
    let finished = (pct - 1.0).abs() < f64::EPSILON;
    if finished {
        db.execute(
            "UPDATE books SET reading_status='finished' WHERE id=? AND user_id=?",
            params![book_id, user],
        )?;
        reading_stats::record_event(
            db,
            user,
            book_id,
            "finished",
            source,
            json!({ "progression": pct }),
        )?;
    }
    let format: String = db
        .query_row(
            "SELECT format FROM epub_files WHERE book_id=?",
            params![book_id],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| "epub".into());
    reading_stats::record_progression(db, user, book_id, pct, None, &format, source, device_id)?;
    Ok(())
}

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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageStatEventBody {
    page: i64,
    start_time: i64,
    duration_seconds: i64,
    total_pages: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KoreaderPageStatsBody {
    device_id: String,
    #[serde(default)]
    device_name: Option<String>,
    events: Vec<PageStatEventBody>,
    /// Optional explicit progress (0–1 or 0–100). Else last cluster endProgress.
    #[serde(default)]
    percentage: Option<f64>,
    #[serde(default)]
    timezone_offset_minutes: Option<i32>,
}

pub(super) async fn post_koreader_page_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<KoreaderPageStatsBody>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, id)?;
            let device_id = body.device_id.trim();
            if device_id.is_empty() || device_id.len() > 128 {
                return Err(ApiError(StatusCode::BAD_REQUEST, "deviceId is required."));
            }
            if body.events.is_empty() {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "events must not be empty.",
                ));
            }
            if body.events.len() > KOREADER_MAX_EVENTS {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Too many page-stat events.",
                ));
            }
            let mut events = Vec::with_capacity(body.events.len());
            for e in &body.events {
                if e.page < 0
                    || e.start_time < 0
                    || e.duration_seconds < 0
                    || e.duration_seconds > KOREADER_MAX_EVENT_DURATION_SECONDS
                    || e.total_pages < 1
                {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "Invalid page-stat event.",
                    ));
                }
                events.push(KoreaderPageEvent {
                    page: e.page,
                    start_time: e.start_time,
                    duration_seconds: e.duration_seconds,
                    total_pages: e.total_pages,
                });
            }
            let derived = derive_koreader_sessions(&events, device_id, id);
            let mut imported = 0_usize;
            let mut skipped = 0_usize;
            let mut session_ids = Vec::new();
            let mut last_end_progress: Option<f64> = None;
            for session in &derived {
                if session.session_id.len() > 80 {
                    skipped += 1;
                    continue;
                }
                let session_body = SessionBody {
                    session_id: session.session_id.clone(),
                    started_at: format_rfc3339(session.started_at),
                    ended_at: format_rfc3339(session.ended_at),
                    duration_seconds: session.duration_seconds,
                    progress_delta: session.progress_delta,
                    end_progress: session.end_progress,
                    start_progress: match (session.end_progress, session.progress_delta) {
                        (Some(e), Some(d)) => Some(e - d),
                        _ => None,
                    },
                    session_type: Some("read".into()),
                    source: Some("koreader".into()),
                    format: None,
                    chars_delta: None,
                    pages_delta: session.pages_delta,
                    device_id: Some(device_id.to_string()),
                    device_name: body.device_name.clone(),
                    timezone_offset_minutes: body.timezone_offset_minutes,
                };
                let existed: bool = db
                    .query_row(
                        "SELECT 1 FROM reading_sessions WHERE user_id=? AND client_session_id=?",
                        params![user, session.session_id],
                        |_| Ok(true),
                    )
                    .optional()?
                    .unwrap_or(false);
                reading_stats::upsert_session(db, user, id, &session_body)?;
                if existed {
                    skipped += 1;
                } else {
                    imported += 1;
                }
                session_ids.push(session.session_id.clone());
                if session.end_progress.is_some() {
                    last_end_progress = session.end_progress;
                }
            }

            let progress = as_percent_0_100(body.percentage).or(last_end_progress);
            if let Some(p) = progress {
                apply_progress(db, user, id, p, "koreader", Some(device_id))?;
            }

            Ok(Json(json!({
                "imported": imported,
                "skipped": skipped,
                "sessionIds": session_ids,
                "source": "koreader",
            })))
        })
        .await
}

fn kobo_device_key(device_id: &str) -> String {
    let dig = Sha256::digest(device_id.as_bytes());
    let mut out = String::new();
    for b in dig.iter().take(8) {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn kobo_statistics_session_id(
    device_key: &str,
    book_id: i64,
    generation: i64,
    spent_reading_minutes: i64,
) -> String {
    format!(
        "{KOBO_STATISTICS_SESSION_ID_PREFIX}{device_key}:{book_id}:{generation}:{spent_reading_minutes}"
    )
}

fn upsert_kobo_cursor(
    db: &Connection,
    user: i64,
    book_id: i64,
    device_key: &str,
    generation: i64,
    spent: i64,
    last_modified: Option<&str>,
) -> ApiResult<()> {
    db.execute(
        "INSERT INTO reading_session_sync_cursors(
            user_id, book_id, source, device_key, generation,
            spent_reading_minutes, last_modified, updated_at
         ) VALUES (?,?,?,?,?,?,?,strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(user_id, book_id, source, device_key) DO UPDATE SET
            generation=excluded.generation,
            spent_reading_minutes=excluded.spent_reading_minutes,
            last_modified=excluded.last_modified,
            updated_at=excluded.updated_at",
        params![
            user,
            book_id,
            KOBO_STATISTICS_CURSOR_SOURCE,
            device_key,
            generation,
            spent,
            last_modified
        ],
    )?;
    Ok(())
}

pub(crate) fn migrate(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS reading_session_sync_cursors (
            user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
            source TEXT NOT NULL,
            device_key TEXT NOT NULL,
            generation INTEGER NOT NULL DEFAULT 0,
            spent_reading_minutes INTEGER NOT NULL DEFAULT 0,
            last_modified TEXT,
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
            PRIMARY KEY (user_id, book_id, source, device_key)
        );",
    )?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KoboStatisticsBody {
    device_id: String,
    #[serde(default)]
    device_name: Option<String>,
    spent_reading_minutes: i64,
    #[serde(default)]
    last_modified: Option<String>,
    #[serde(default)]
    end_progress: Option<f64>,
    #[serde(default)]
    timezone_offset_minutes: Option<i32>,
}

pub(super) async fn post_kobo_statistics(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<KoboStatisticsBody>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, id)?;
            let device_id = body.device_id.trim();
            if device_id.is_empty() || device_id.len() > 128 {
                return Err(ApiError(StatusCode::BAD_REQUEST, "deviceId is required."));
            }
            if body.spent_reading_minutes < 0 {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "spentReadingMinutes must be >= 0.",
                ));
            }
            let device_key = kobo_device_key(device_id);
            let (mut generation, prev_minutes): (i64, i64) = db
                .query_row(
                    "SELECT generation, spent_reading_minutes
                     FROM reading_session_sync_cursors
                     WHERE user_id=? AND book_id=? AND source=? AND device_key=?",
                    params![user, id, KOBO_STATISTICS_CURSOR_SOURCE, device_key],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .unwrap_or((0, 0));

            let next = body.spent_reading_minutes;
            if next == prev_minutes {
                return Ok(Json(json!({
                    "imported": 0,
                    "skipped": 1,
                    "sessionIds": [],
                    "source": "kobo",
                    "spentReadingMinutes": next,
                    "generation": generation,
                })));
            }

            // Counter reset / re-read → new generation; delta from 0.
            let baseline = if next < prev_minutes {
                generation += 1;
                0_i64
            } else {
                prev_minutes
            };
            let delta_minutes = next - baseline;
            let last_mod = body
                .last_modified
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());

            if delta_minutes < 1 {
                upsert_kobo_cursor(db, user, id, &device_key, generation, next, last_mod)?;
                return Ok(Json(json!({
                    "imported": 0,
                    "skipped": 1,
                    "sessionIds": [],
                    "source": "kobo",
                    "spentReadingMinutes": next,
                    "generation": generation,
                })));
            }

            let duration_seconds = delta_minutes * 60;
            let ended_unix = last_mod
                .and_then(|s| parse_rfc3339(s).ok())
                .unwrap_or_else(|| {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0)
                });
            let started_unix = ended_unix - duration_seconds;
            let session_id = kobo_statistics_session_id(&device_key, id, generation, next);
            if session_id.len() > 80 {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Derived session id too long.",
                ));
            }

            let session_body = SessionBody {
                session_id: session_id.clone(),
                started_at: format_rfc3339(started_unix),
                ended_at: format_rfc3339(ended_unix),
                duration_seconds,
                progress_delta: None,
                end_progress: body.end_progress,
                start_progress: None,
                session_type: Some("read".into()),
                source: Some("kobo".into()),
                format: None,
                chars_delta: None,
                pages_delta: None,
                device_id: Some(device_id.to_string()),
                device_name: body.device_name.clone(),
                timezone_offset_minutes: body.timezone_offset_minutes,
            };

            let existed: bool = db
                .query_row(
                    "SELECT 1 FROM reading_sessions WHERE user_id=? AND client_session_id=?",
                    params![user, session_id],
                    |_| Ok(true),
                )
                .optional()?
                .unwrap_or(false);
            reading_stats::upsert_session(db, user, id, &session_body)?;
            upsert_kobo_cursor(
                db,
                user,
                id,
                &device_key,
                generation,
                next,
                Some(&format_rfc3339(ended_unix)),
            )?;

            if let Some(p) = as_percent_0_100(body.end_progress) {
                apply_progress(db, user, id, p, "kobo", Some(device_id))?;
            }

            Ok(Json(json!({
                "imported": if existed { 0 } else { 1 },
                "skipped": if existed { 1 } else { 0 },
                "sessionIds": [session_id],
                "source": "kobo",
                "spentReadingMinutes": next,
                "generation": generation,
                "durationSeconds": duration_seconds,
            })))
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clusters_by_gap_and_caps_duration() {
        let events = vec![
            KoreaderPageEvent {
                page: 1,
                start_time: 1_000,
                duration_seconds: 60,
                total_pages: 100,
            },
            KoreaderPageEvent {
                page: 2,
                start_time: 1_060,
                duration_seconds: 60,
                total_pages: 100,
            },
            // Gap > 1800 → new cluster
            KoreaderPageEvent {
                page: 3,
                start_time: 4_000,
                duration_seconds: 120,
                total_pages: 100,
            },
        ];
        let sessions = derive_koreader_sessions(&events, "dev-1", 42);
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].duration_seconds, 120);
        assert_eq!(sessions[0].end_progress, Some(2.0));
        assert_eq!(sessions[1].duration_seconds, 120);
        assert!(sessions[0].session_id.starts_with("kor:v2-"));
        assert!(sessions[0].session_id.contains(":42:1000"));
        assert!(sessions[0].session_id.len() <= 80);
    }

    #[test]
    fn drops_under_min_session() {
        let events = vec![KoreaderPageEvent {
            page: 1,
            start_time: 100,
            duration_seconds: 5,
            total_pages: 10,
        }];
        assert!(derive_koreader_sessions(&events, "d", 1).is_empty());
    }

    #[test]
    fn exact_gap_stays_in_cluster() {
        // gap == 1800 stays together (BO: break only when > gap)
        let events = vec![
            KoreaderPageEvent {
                page: 1,
                start_time: 0,
                duration_seconds: 60,
                total_pages: 50,
            },
            KoreaderPageEvent {
                page: 2,
                start_time: 60 + 1800,
                duration_seconds: 60,
                total_pages: 50,
            },
        ];
        let sessions = derive_koreader_sessions(&events, "dev", 9);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].duration_seconds, 120);
    }

    #[test]
    fn kobo_session_id_is_stable() {
        let key = kobo_device_key("kobo-abc");
        let a = kobo_statistics_session_id(&key, 7, 0, 860);
        let b = kobo_statistics_session_id(&key, 7, 0, 860);
        assert_eq!(a, b);
        assert!(a.starts_with("kst:"));
        assert!(a.len() <= 80);
    }
}
