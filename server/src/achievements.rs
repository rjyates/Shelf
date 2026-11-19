//! M12 achievements: compact seed + evaluate against sessions/catalogue.

use super::*;
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Clone, Copy)]
struct Seed {
    key: &'static str,
    category: &'static str,
    name: &'static str,
    description: &'static str,
    rarity: &'static str,
    threshold: Option<i64>,
    sort_order: i64,
}

const SEED: &[Seed] = &[
    Seed {
        key: "first_session",
        category: "reading",
        name: "First Sitting",
        description: "Record your first reading session.",
        rarity: "common",
        threshold: Some(1),
        sort_order: 10,
    },
    Seed {
        key: "sessions_10",
        category: "reading",
        name: "Ten Sittings",
        description: "Log ten reading sessions.",
        rarity: "common",
        threshold: Some(10),
        sort_order: 20,
    },
    Seed {
        key: "streak_7",
        category: "dedication",
        name: "Week of Pages",
        description: "Reach a seven-day reading streak.",
        rarity: "rare",
        threshold: Some(7),
        sort_order: 30,
    },
    Seed {
        key: "finish_1",
        category: "reading",
        name: "Closed Cover",
        description: "Finish your first book.",
        rarity: "common",
        threshold: Some(1),
        sort_order: 40,
    },
    Seed {
        key: "finish_5",
        category: "reading",
        name: "Shelf Clearer",
        description: "Finish five books.",
        rarity: "rare",
        threshold: Some(5),
        sort_order: 50,
    },
    Seed {
        key: "library_25",
        category: "library",
        name: "Growing Stack",
        description: "Own twenty-five books on your shelf.",
        rarity: "common",
        threshold: Some(25),
        sort_order: 60,
    },
    Seed {
        key: "library_100",
        category: "library",
        name: "Hundred Strong",
        description: "Reach one hundred books.",
        rarity: "epic",
        threshold: Some(100),
        sort_order: 70,
    },
    Seed {
        key: "multi_source",
        category: "devices",
        name: "Wherever You Are",
        description: "Read from two different sources (web, mobile, KOReader…).",
        rarity: "rare",
        threshold: Some(2),
        sort_order: 80,
    },
    Seed {
        key: "hours_10",
        category: "dedication",
        name: "Ten Hours In",
        description: "Accumulate ten hours of recorded reading.",
        rarity: "rare",
        threshold: Some(10 * 3600),
        sort_order: 90,
    },
    Seed {
        key: "formats_3",
        category: "exploration",
        name: "Format Explorer",
        description: "Read across three file formats.",
        rarity: "rare",
        threshold: Some(3),
        sort_order: 100,
    },
];

pub(crate) fn migrate(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_flags(flag TEXT PRIMARY KEY NOT NULL);
         CREATE TABLE IF NOT EXISTS user_achievements (
           user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
           achievement_key TEXT NOT NULL,
           awarded_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
           PRIMARY KEY (user_id, achievement_key)
         );",
    )?;
    let _ = db.execute(
        "INSERT OR IGNORE INTO schema_flags(flag) VALUES ('achievements_v1')",
        [],
    );
    Ok(())
}

struct Progress {
    sessions: i64,
    finished: i64,
    library: i64,
    sources: i64,
    reading_seconds: i64,
    formats: i64,
    longest_streak: i64,
}

fn progress(db: &Connection, user: i64) -> ApiResult<Progress> {
    let sessions: i64 = db.query_row(
        "SELECT count(*) FROM reading_sessions WHERE user_id=?",
        params![user],
        |r| r.get(0),
    )?;
    let finished: i64 = db.query_row(
        "SELECT count(*) FROM books WHERE user_id=? AND reading_status='finished'",
        params![user],
        |r| r.get(0),
    )?;
    let library: i64 = db.query_row(
        "SELECT count(*) FROM books WHERE user_id=?",
        params![user],
        |r| r.get(0),
    )?;
    let sources: i64 = db.query_row(
        "SELECT count(DISTINCT source) FROM reading_sessions WHERE user_id=?",
        params![user],
        |r| r.get(0),
    )?;
    let reading_seconds: i64 = db.query_row(
        "SELECT coalesce(sum(reading_seconds),0) FROM reading_daily_stats WHERE user_id=?",
        params![user],
        |r| r.get(0),
    )?;
    let formats: i64 = db.query_row(
        "SELECT count(DISTINCT coalesce(f.format,'manual'))
         FROM reading_sessions s
         LEFT JOIN epub_files f ON f.book_id=s.book_id
         WHERE s.user_id=?",
        params![user],
        |r| r.get(0),
    )?;
    let mut days = BTreeSet::new();
    {
        let mut stmt = db.prepare(
            "SELECT DISTINCT day_local FROM reading_daily_stats
             WHERE user_id=? AND (reading_seconds > 0 OR session_count > 0)",
        )?;
        for day in stmt.query_map(params![user], |r| r.get::<_, String>(0))? {
            days.insert(day?);
        }
    }
    let longest_streak = insights::compute_longest_streak(&days);
    Ok(Progress {
        sessions,
        finished,
        library,
        sources,
        reading_seconds,
        formats,
        longest_streak,
    })
}

fn current_for(seed: &Seed, p: &Progress) -> i64 {
    match seed.key {
        "first_session" | "sessions_10" => p.sessions,
        "streak_7" => p.longest_streak,
        "finish_1" | "finish_5" => p.finished,
        "library_25" | "library_100" => p.library,
        "multi_source" => p.sources,
        "hours_10" => p.reading_seconds,
        "formats_3" => p.formats,
        _ => 0,
    }
}

fn earned(seed: &Seed, p: &Progress) -> bool {
    let Some(threshold) = seed.threshold else {
        return false;
    };
    current_for(seed, p) >= threshold
}

pub(super) async fn list_achievements(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            migrate(db)?;
            let p = progress(db, user)?;
            let mut unlocked: BTreeSet<String> = BTreeSet::new();
            {
                let mut stmt = db.prepare(
                    "SELECT achievement_key, awarded_at FROM user_achievements WHERE user_id=?",
                )?;
                for row in stmt.query_map(params![user], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })? {
                    let (key, _) = row?;
                    unlocked.insert(key);
                }
            }
            // Award newly earned keys.
            for seed in SEED {
                if earned(seed, &p) && !unlocked.contains(seed.key) {
                    db.execute(
                        "INSERT OR IGNORE INTO user_achievements(user_id, achievement_key)
                         VALUES (?,?)",
                        params![user, seed.key],
                    )?;
                    unlocked.insert(seed.key.to_string());
                }
            }
            let mut awarded_at: std::collections::BTreeMap<String, String> =
                std::collections::BTreeMap::new();
            {
                let mut stmt = db.prepare(
                    "SELECT achievement_key, awarded_at FROM user_achievements WHERE user_id=?",
                )?;
                for row in stmt.query_map(params![user], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })? {
                    let (k, at) = row?;
                    awarded_at.insert(k, at);
                }
            }

            let mut by_cat: std::collections::BTreeMap<&str, Vec<Value>> =
                std::collections::BTreeMap::new();
            for seed in SEED {
                let is_earned = unlocked.contains(seed.key);
                let item = json!({
                    "key": seed.key,
                    "groupKey": Value::Null,
                    "tier": Value::Null,
                    "category": seed.category,
                    "name": seed.name,
                    "description": seed.description,
                    "iconName": seed.key,
                    "rarity": seed.rarity,
                    "threshold": seed.threshold,
                    "hidden": false,
                    "sortOrder": seed.sort_order,
                    "earned": is_earned,
                    "awardedAt": awarded_at.get(seed.key).cloned(),
                    "context": Value::Null,
                    "currentProgress": current_for(seed, &p),
                });
                by_cat.entry(seed.category).or_default().push(item);
            }

            let labels = [
                ("reading", "Reading"),
                ("library", "Library"),
                ("exploration", "Exploration"),
                ("dedication", "Dedication"),
                ("devices", "Devices"),
            ];
            let mut categories = Vec::new();
            let mut total_earned = 0_i64;
            let total_available = SEED.len() as i64;
            for (key, label) in labels {
                let achievements = by_cat.remove(key).unwrap_or_default();
                let earned_count = achievements
                    .iter()
                    .filter(|a| a["earned"].as_bool() == Some(true))
                    .count() as i64;
                total_earned += earned_count;
                categories.push(json!({
                    "key": key,
                    "label": label,
                    "earnedCount": earned_count,
                    "totalCount": achievements.len(),
                    "achievements": achievements,
                }));
            }

            Ok(Json(json!({
                "categories": categories,
                "totalEarned": total_earned,
                "totalAvailable": total_available,
            })))
        })
        .await
}
