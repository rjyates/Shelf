//! M12 Insights: session-derived reading charts (heatmap, source distribution,
//! session timeline). Widget/achievement/library charts land in later commits.

use super::*;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
pub(super) struct InsightsRangeQuery {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
}

fn default_range(from: Option<String>, to: Option<String>) -> (String, String) {
    let to = to
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| chrono_today_utc());
    let from = from.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| {
        // ~26 weeks of heatmap columns (GitHub-style).
        add_days(&to, -(7 * 26 - 1)).unwrap_or_else(|| "1970-01-01".into())
    });
    (from, to)
}

fn chrono_today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    epoch_to_date_key(secs)
}

fn epoch_to_date_key(secs: i64) -> String {
    // Civil date from Unix days (proleptic Gregorian), UTC.
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let y = y + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Add `delta` days to a `YYYY-MM-DD` key. Returns None on bad input.
pub(crate) fn add_days(date: &str, delta: i64) -> Option<String> {
    let parts: Vec<_> = date.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i64 = parts[0].parse().ok()?;
    let m: i64 = parts[1].parse().ok()?;
    let d: i64 = parts[2].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Convert to days since epoch-ish via civil_from_days inverse.
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = m + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let z = era * 146_097 + doe - 719_468 + delta;
    Some(epoch_to_date_key(z * 86_400))
}

pub(crate) fn compute_longest_streak(read_days: &BTreeSet<String>) -> i64 {
    if read_days.is_empty() {
        return 0;
    }
    let sorted: Vec<_> = read_days.iter().cloned().collect();
    let mut longest = 1_i64;
    let mut current = 1_i64;
    for i in 1..sorted.len() {
        let Some(prev_next) = add_days(&sorted[i - 1], 1) else {
            current = 1;
            continue;
        };
        if prev_next == sorted[i] {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 1;
        }
    }
    longest
}

/// Book Orbit `computeStreakData`: allow yesterday if today is empty.
pub(crate) fn compute_streak_data(
    read_days: &BTreeSet<String>,
    today: &str,
) -> (i64, i64, Vec<bool>) {
    let mut last_seven = Vec::with_capacity(7);
    for i in (0..7).rev() {
        let day = add_days(today, -i).unwrap_or_else(|| today.to_string());
        last_seven.push(read_days.contains(&day));
    }
    if read_days.is_empty() {
        return (0, 0, last_seven);
    }
    let longest = compute_longest_streak(read_days);
    let mut cursor = today.to_string();
    if !read_days.contains(&cursor) {
        cursor = add_days(today, -1).unwrap_or(cursor);
        if !read_days.contains(&cursor) {
            return (0, longest, last_seven);
        }
    }
    let mut current = 0_i64;
    while read_days.contains(&cursor) {
        current += 1;
        cursor = match add_days(&cursor, -1) {
            Some(d) => d,
            None => break,
        };
    }
    (current, longest, last_seven)
}

fn aggregate_daily(
    db: &Connection,
    user: i64,
    from: &str,
    to: &str,
) -> ApiResult<(Vec<Value>, Value, BTreeSet<String>)> {
    let mut stmt = db.prepare(
        "SELECT day_local, source, reading_seconds, session_count
         FROM reading_daily_stats
         WHERE user_id=? AND day_local >= ? AND day_local <= ?
         ORDER BY day_local ASC, source ASC",
    )?;
    let rows = stmt
        .query_map(params![user, from, to], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut by_day: BTreeMap<String, (i64, i64, Map<String, Value>)> = BTreeMap::new();
    let mut source_totals: BTreeMap<String, i64> = BTreeMap::new();
    let mut read_days = BTreeSet::new();

    for (day, source, secs, sessions) in rows {
        if secs > 0 || sessions > 0 {
            read_days.insert(day.clone());
        }
        let entry = by_day.entry(day).or_insert_with(|| (0, 0, Map::new()));
        entry.0 += secs;
        entry.1 += sessions;
        entry.2.insert(source.clone(), json!(secs));
        *source_totals.entry(source).or_insert(0) += secs;
    }

    let heatmap: Vec<Value> = by_day
        .into_iter()
        .map(|(day, (reading_seconds, session_count, by_source))| {
            json!({
                "day": day,
                "readingSeconds": reading_seconds,
                "sessionCount": session_count,
                "bySource": Value::Object(by_source),
            })
        })
        .collect();

    let total_seconds: i64 = source_totals.values().sum();
    let slices: Vec<Value> = source_totals
        .into_iter()
        .map(|(bucket, reading_seconds)| {
            json!({
                "bucket": bucket,
                "readingSeconds": reading_seconds,
            })
        })
        .collect();
    let source_distribution = json!({
        "totalSeconds": total_seconds,
        "slices": slices,
    });

    Ok((heatmap, source_distribution, read_days))
}

fn session_timeline(db: &Connection, user: i64, limit: i64) -> ApiResult<Vec<Value>> {
    let mut stmt = db.prepare(
        "SELECT s.id, s.book_id, b.title, coalesce(f.format, 'manual'), s.client_session_id,
                s.started_at, s.ended_at, s.duration_seconds, s.source
         FROM reading_sessions s
         JOIN books b ON b.id = s.book_id
         LEFT JOIN epub_files f ON f.book_id = s.book_id
         WHERE s.user_id=?
         ORDER BY s.ended_at DESC
         LIMIT ?",
    )?;
    let rows = stmt
        .query_map(params![user, limit], |r| {
            Ok(json!({
                "sessionId": r.get::<_, i64>(0)?,
                "bookId": r.get::<_, i64>(1)?,
                "bookTitle": r.get::<_, String>(2)?,
                "bookFormat": r.get::<_, String>(3)?,
                "clientSessionId": r.get::<_, String>(4)?,
                "startedAt": r.get::<_, String>(5)?,
                "endedAt": r.get::<_, String>(6)?,
                "durationSeconds": r.get::<_, i64>(7)?,
                "bookSource": r.get::<_, String>(8)?,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// `GET /api/v1/insights/reading` — heatmap + source distribution + recent sessions.
pub(super) async fn reading_insights(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<InsightsRangeQuery>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let (from, to) = default_range(q.from, q.to);
            let (heatmap, source_distribution, read_days) =
                aggregate_daily(db, user, &from, &to)?;
            // Streak uses all-time read days so a short heatmap window cannot truncate it.
            let mut all_days = BTreeSet::new();
            {
                let mut stmt = db.prepare(
                    "SELECT DISTINCT day_local FROM reading_daily_stats
                     WHERE user_id=? AND (reading_seconds > 0 OR session_count > 0)",
                )?;
                for day in stmt.query_map(params![user], |r| r.get::<_, String>(0))? {
                    all_days.insert(day?);
                }
            }
            let _ = read_days;
            let today = chrono_today_utc();
            let (current_streak, longest_streak, last_seven_days) =
                compute_streak_data(&all_days, &today);
            let session_timeline = session_timeline(db, user, 25)?;
            Ok(Json(json!({
                "from": from,
                "to": to,
                "heatmap": heatmap,
                "sourceDistribution": source_distribution,
                "sessionTimeline": session_timeline,
                "streak": {
                    "currentStreak": current_streak,
                    "longestStreak": longest_streak,
                    "lastSevenDays": last_seven_days,
                },
            })))
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_days_crosses_month_boundary() {
        assert_eq!(add_days("2026-03-01", -1).as_deref(), Some("2026-02-28"));
        assert_eq!(add_days("2026-03-15", 0).as_deref(), Some("2026-03-15"));
        assert_eq!(add_days("2026-12-31", 1).as_deref(), Some("2027-01-01"));
    }

    #[test]
    fn streak_allows_yesterday_when_today_empty() {
        let mut days = BTreeSet::new();
        days.insert("2026-03-13".into());
        days.insert("2026-03-14".into());
        let (current, longest, last7) = compute_streak_data(&days, "2026-03-15");
        assert_eq!(current, 2);
        assert_eq!(longest, 2);
        assert_eq!(last7.len(), 7);
        assert!(!last7[6]); // today empty
        assert!(last7[5]); // yesterday
    }

    #[test]
    fn streak_breaks_on_gap() {
        let mut days = BTreeSet::new();
        days.insert("2026-03-10".into());
        days.insert("2026-03-12".into());
        let (current, longest, _) = compute_streak_data(&days, "2026-03-12");
        assert_eq!(current, 1);
        assert_eq!(longest, 1);
    }
}
