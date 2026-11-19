//! Durable metadata auto-fetch queue (Book Orbit–shaped, in-process poller).

use super::pipeline::{autofill_from_candidates, ensure_settings, metadata_json_for_book};
use super::providers::{search_builtin, search_external, SearchQuery};
use super::*;
use serde_json::{json, Value};

pub(crate) fn status(db: &Connection, user: i64) -> ApiResult<Value> {
    ensure_settings(db, user)?;
    let paused: i64 = db.query_row(
        "SELECT paused FROM metadata_settings WHERE user_id=?",
        params![user],
        |r| r.get(0),
    )?;
    let queued: i64 = db.query_row(
        "SELECT COUNT(*) FROM metadata_fetch_jobs WHERE user_id=? AND status='queued'",
        params![user],
        |r| r.get(0),
    )?;
    let processing: i64 = db.query_row(
        "SELECT COUNT(*) FROM metadata_fetch_jobs WHERE user_id=? AND status='processing'",
        params![user],
        |r| r.get(0),
    )?;
    let failed: i64 = db.query_row(
        "SELECT COUNT(*) FROM metadata_fetch_jobs WHERE user_id=? AND status='failed'",
        params![user],
        |r| r.get(0),
    )?;
    let current: Option<Value> = db
        .query_row(
            "SELECT j.id, j.book_id, b.title FROM metadata_fetch_jobs j
             JOIN books b ON b.id=j.book_id
             WHERE j.user_id=? AND j.status='processing' ORDER BY j.id LIMIT 1",
            params![user],
            |r| {
                Ok(json!({
                    "job_id": r.get::<_, i64>(0)?,
                    "book_id": r.get::<_, i64>(1)?,
                    "title": r.get::<_, String>(2)?,
                }))
            },
        )
        .optional()?;
    Ok(json!({
        "paused": paused != 0,
        "queued": queued,
        "processing": processing,
        "failed": failed,
        "current": current,
    }))
}

fn eligible(db: &Connection, user: i64, book_id: i64, ignore_cache: bool) -> ApiResult<bool> {
    if ignore_cache {
        return Ok(true);
    }
    ensure_settings(db, user)?;
    let cfg_raw: String = db.query_row(
        "SELECT fetch_config FROM metadata_settings WHERE user_id=?",
        params![user],
        |r| r.get(0),
    )?;
    let cfg: Value =
        serde_json::from_str(&cfg_raw).unwrap_or_else(|_| pipeline::default_fetch_config());
    if cfg.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
        return Ok(false);
    }
    let conditions = cfg.get("conditions").cloned().unwrap_or(json!({}));
    let meta = metadata_json_for_book(db, book_id)?;
    let score = meta
        .get("metadata_score")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let last = meta.get("last_fetch_at").and_then(|v| v.as_str());
    let never = last.is_none();
    if conditions
        .get("never_fetched")
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
        && never
    {
        return Ok(true);
    }
    let low = conditions
        .get("low_score_below")
        .and_then(|v| v.as_i64())
        .unwrap_or(60);
    if score < low {
        return Ok(true);
    }
    let missing = conditions
        .get("missing_fields")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for f in missing {
        let Some(name) = f.as_str() else { continue };
        let empty = match name {
            "description" => meta
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty(),
            "cover" => meta
                .get("covers")
                .and_then(|v| v.as_array())
                .map(|a| {
                    !a.iter()
                        .any(|c| c.get("active").and_then(|x| x.as_bool()) == Some(true))
                })
                .unwrap_or(true),
            _ => false,
        };
        if empty {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn enqueue_book(
    db: &Connection,
    user: i64,
    book_id: i64,
    reason: &str,
    ignore_cache: bool,
) -> ApiResult<()> {
    // Skip duplicate queued/processing for same book.
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM metadata_fetch_jobs WHERE user_id=? AND book_id=? AND status IN ('queued','processing'))",
        params![user, book_id],
        |r| r.get(0),
    )?;
    if exists {
        return Ok(());
    }
    db.execute(
        "INSERT INTO metadata_fetch_jobs(user_id, book_id, reason, status, ignore_cache)
         VALUES (?,?,?,'queued',?)",
        params![user, book_id, reason, if ignore_cache { 1 } else { 0 }],
    )?;
    Ok(())
}

pub(crate) fn try_enqueue(db: &Connection, user: i64, book_id: i64, reason: &str) -> ApiResult<()> {
    ensure_settings(db, user)?;
    // Import path never hits metadata HTTP handlers; seed builtins here like BO's registry.
    pipeline::ensure_builtin_providers(db, user)?;
    let cfg_raw: String = db.query_row(
        "SELECT fetch_config FROM metadata_settings WHERE user_id=?",
        params![user],
        |r| r.get(0),
    )?;
    let cfg: Value =
        serde_json::from_str(&cfg_raw).unwrap_or_else(|_| pipeline::default_fetch_config());
    if cfg.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
        return Ok(());
    }
    if reason == "import" && cfg.get("trigger_on_import").and_then(|v| v.as_bool()) == Some(false) {
        return Ok(());
    }
    if eligible(db, user, book_id, false)? {
        enqueue_book(db, user, book_id, reason, false)?;
    }
    Ok(())
}

pub(crate) fn enqueue_if_eligible(db: &Connection, user: i64, book_id: i64) -> ApiResult<()> {
    try_enqueue(db, user, book_id, "import")
}

pub(crate) fn enqueue_run(db: &Connection, user: i64, ignore_cache: bool) -> ApiResult<i64> {
    let ids: Vec<i64> = {
        let mut stmt = db.prepare("SELECT id FROM books WHERE user_id=?")?;
        let rows = stmt
            .query_map(params![user], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut n = 0i64;
    for id in ids {
        if eligible(db, user, id, ignore_cache)? {
            enqueue_book(db, user, id, "run", ignore_cache)?;
            n += 1;
        }
    }
    Ok(n)
}

fn has_text(v: Option<&str>) -> bool {
    v.map(str::trim).filter(|s| !s.is_empty()).is_some()
}

/// BO `searchAndRankProvider`: primary → drop ISBN → title-only, each pass filterAndRank.
pub(super) fn search_and_rank_provider_list(
    search: impl Fn(&SearchQuery) -> Vec<super::providers::Candidate>,
    base: &SearchQuery,
    title: &str,
    author: &str,
    isbn: Option<&str>,
) -> Vec<super::providers::Candidate> {
    let limit = base.limit.max(1);
    let primary = search(base);
    let ranked = super::rank::filter_and_rank(primary, title, author, isbn, limit);
    if !ranked.is_empty() {
        return ranked;
    }

    let has_isbn = has_text(isbn);
    let has_fallback_terms = has_text(Some(title)) || has_text(Some(author));
    if has_isbn && has_fallback_terms {
        let mut fallback = base.clone();
        fallback.isbn = None;
        let found = search(&fallback);
        let ranked = super::rank::filter_and_rank(found, title, author, None, limit);
        if !ranked.is_empty() {
            return ranked;
        }
    }

    if has_text(Some(title)) && has_text(Some(author)) {
        let mut title_only = base.clone();
        title_only.author = None;
        title_only.isbn = None;
        let found = search(&title_only);
        return super::rank::filter_and_rank(found, title, author, None, limit);
    }
    Vec::new()
}

fn search_and_rank_provider(
    search: impl Fn(&SearchQuery) -> Vec<super::providers::Candidate>,
    base: &SearchQuery,
    title: &str,
    author: &str,
    isbn: Option<&str>,
) -> Option<super::providers::Candidate> {
    search_and_rank_provider_list(search, base, title, author, isbn)
        .into_iter()
        .next()
}

pub(super) fn collect_provider_candidates(
    db: &Connection,
    user: i64,
    title: &str,
    author: &str,
    isbn: Option<&str>,
) -> ApiResult<Vec<(String, super::providers::Candidate)>> {
    pipeline::ensure_builtin_providers(db, user)?;
    let search_title = {
        let core = super::rank::title_core(title);
        if core.is_empty() {
            title.to_string()
        } else {
            title.split(':').next().unwrap_or(title).trim().to_string()
        }
    };
    let base_q = SearchQuery {
        title: Some(search_title.clone()),
        author: Some(author.to_string()),
        isbn: isbn.map(str::to_string),
        query: None,
        series_name: None,
        series_index: None,
        is_audiobook: false,
        config_json: None,
        limit: 5,
    };
    let mut stmt = db.prepare(
        "SELECT key, enabled, kind, base_url, api_key, config_json FROM metadata_providers WHERE user_id=?",
    )?;
    let rows: Vec<(
        String,
        bool,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = stmt
        .query_map(params![user], |r| {
            Ok((
                r.get(0)?,
                r.get::<_, i64>(1)? != 0,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut by_provider = Vec::new();
    for (key, enabled, kind, base_url, api_key, config_json) in rows {
        if !enabled {
            continue;
        }
        let q = super::with_provider_ctx(base_q.clone(), config_json);
        let key_for_search = key.clone();
        let kind_owned = kind.clone();
        let base_url_owned = base_url.clone();
        let api_key_owned = api_key.clone();
        let search = move |query: &SearchQuery| {
            if kind_owned == "external" {
                let Some(url) = base_url_owned
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                else {
                    return Vec::new();
                };
                search_external(url, &key_for_search, query).unwrap_or_default()
            } else {
                search_builtin(&key_for_search, query, api_key_owned.as_deref()).unwrap_or_default()
            }
        };
        if let Some(best) = search_and_rank_provider(search, &q, &search_title, author, isbn) {
            by_provider.push((key, best));
        }
    }
    Ok(by_provider)
}

fn process_one(db: &Connection, user: i64, book_id: i64, reason: &str) -> ApiResult<()> {
    let (title, author): (String, String) = db.query_row(
        "SELECT title, author FROM books WHERE id=?",
        params![book_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    pipeline::ensure_book_metadata_row(db, book_id)?;
    let (isbn13, isbn10): (Option<String>, Option<String>) = db.query_row(
        "SELECT isbn13, isbn10 FROM book_metadata WHERE book_id=?",
        params![book_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let isbn = isbn13
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            isbn10
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        });
    let by_provider = collect_provider_candidates(db, user, &title, &author, isbn.as_deref())?;
    // BO: import runs preserveExisting so embedded file metadata is not overwritten.
    let preserve_existing = reason == "import";
    let _ = autofill_from_candidates(db, user, book_id, &by_provider, preserve_existing)?;
    Ok(())
}

fn poll_once(database: &PathBuf) -> ApiResult<()> {
    let db = Connection::open(database)?;
    configure_connection(&db)?;
    // Reset stuck processing from prior boot.
    // Claim one job where user is not paused.
    let job: Option<(i64, i64, i64, String)> = db
        .query_row(
            "SELECT j.id, j.user_id, j.book_id, j.reason FROM metadata_fetch_jobs j
             JOIN metadata_settings s ON s.user_id=j.user_id
             WHERE j.status='queued' AND s.paused=0
             ORDER BY j.id LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((job_id, user, book_id, reason)) = job else {
        return Ok(());
    };
    db.execute(
        "UPDATE metadata_fetch_jobs SET status='processing', attempts=attempts+1,
         updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=?",
        params![job_id],
    )?;
    drop(db);
    let db = Connection::open(database)?;
    configure_connection(&db)?;
    match process_one(&db, user, book_id, &reason) {
        Ok(()) => {
            db.execute(
                "UPDATE metadata_fetch_jobs SET status='done', error=NULL,
                 updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=?",
                params![job_id],
            )?;
        }
        Err(e) => {
            let msg = e.1;
            db.execute(
                "UPDATE metadata_fetch_jobs SET status='failed', error=?,
                 updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=?",
                params![msg, job_id],
            )?;
        }
    }
    Ok(())
}

pub(crate) fn kick(state: AppState) {
    tokio::spawn(async move {
        let path = state.database.clone();
        let _ = tokio::task::spawn_blocking(move || poll_once(&path)).await;
    });
}

pub(crate) fn resume_pending(state: &AppState) {
    let database = state.database.clone();
    // Reset processing → queued on boot, then poll loop.
    if let Ok(db) = Connection::open(&database) {
        let _ = configure_connection(&db);
        let _ = db.execute_batch(
            "UPDATE metadata_fetch_jobs SET status='queued'
             WHERE status='processing'",
        );
    }
    let state = state.clone();
    tokio::spawn(async move {
        loop {
            let path = state.database.clone();
            let _ = tokio::task::spawn_blocking(move || poll_once(&path)).await;
            tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        }
    });
}

#[cfg(test)]
mod fallback_tests {
    use super::*;
    use crate::metadata::providers::{Candidate, SearchQuery};

    fn cand(title: &str, authors: &[&str], isbn: Option<&str>) -> Candidate {
        Candidate {
            provider: "test".into(),
            external_id: title.into(),
            title: Some(title.into()),
            authors: authors.iter().map(|s| (*s).to_string()).collect(),
            isbn13: isbn.map(str::to_string),
            ..Candidate::default()
        }
    }

    #[test]
    fn isbn_miss_falls_back_to_title_author() {
        let base = SearchQuery {
            title: Some("The Return of the King".into()),
            author: Some("Tolkien".into()),
            isbn: Some("0000000000000".into()),
            query: None,
            series_name: None,
            series_index: None,
            is_audiobook: false,
            config_json: None,
            limit: 5,
        };
        let search = |q: &SearchQuery| {
            if q.isbn.is_some() {
                // ISBN-only miss
                vec![]
            } else if q.author.is_some() {
                vec![cand("The Return of the King", &["J.R.R. Tolkien"], None)]
            } else {
                vec![cand("Wrong Book", &["Other"], None)]
            }
        };
        let best = search_and_rank_provider(
            search,
            &base,
            "The Return of the King",
            "Tolkien",
            Some("0000000000000"),
        );
        assert_eq!(
            best.unwrap().title.as_deref(),
            Some("The Return of the King")
        );
    }

    #[test]
    fn title_author_miss_falls_back_to_title_only() {
        let base = SearchQuery {
            title: Some("Ubik".into()),
            author: Some("Nobody Match".into()),
            isbn: None,
            query: None,
            series_name: None,
            series_index: None,
            is_audiobook: false,
            config_json: None,
            limit: 5,
        };
        let search = |q: &SearchQuery| {
            if q.author.is_some() {
                vec![cand("Completely Unrelated", &["Someone"], None)]
            } else {
                vec![cand("Ubik", &["Philip K. Dick"], None)]
            }
        };
        let best = search_and_rank_provider(search, &base, "Ubik", "Nobody Match", None);
        assert_eq!(best.unwrap().title.as_deref(), Some("Ubik"));
    }
}
