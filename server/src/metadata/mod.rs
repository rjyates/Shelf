//! Book Orbit–shaped metadata: schema helpers, providers, field-rules apply, auto-fetch queue.

mod fetch;
mod pipeline;
mod providers;
mod rank;

use self::pipeline::{
    backfill_entities, ensure_book_metadata_row, ensure_builtin_providers, ensure_settings,
    metadata_json_for_book, set_book_authors, set_book_series,
};
use self::providers::{
    cover_hit_json, download_cover_bytes, search_builtin, search_cover_hits, search_external,
    Candidate, SearchQuery,
};
use super::*;
use serde_json::{json, Value};

pub(crate) use fetch::{enqueue_if_eligible, kick, resume_pending};

fn with_provider_ctx(mut q: SearchQuery, config_json: Option<String>) -> SearchQuery {
    q.config_json = config_json;
    q
}

pub(crate) fn migrate(db: &Connection) {
    let _ = backfill_entities(db);
    // One-shot: Goodreads autocomplete ships default-on (BO). Existing seeds were off.
    let _ = db
        .execute_batch("CREATE TABLE IF NOT EXISTS schema_flags(flag TEXT PRIMARY KEY NOT NULL);");
    for (flag, key) in [
        ("goodreads_default_on_v1", "goodreads"),
        ("amazon_default_on_v1", "amazon"),
    ] {
        let inserted = db
            .execute(
                "INSERT OR IGNORE INTO schema_flags(flag) VALUES (?1)",
                params![flag],
            )
            .unwrap_or(0);
        if inserted > 0 {
            let _ = db.execute(
                "UPDATE metadata_providers SET enabled = 1 WHERE key = ?1 AND kind = 'builtin'",
                params![key],
            );
        }
    }
    let series_flag = "series_providers_amazon_v1";
    if db
        .execute(
            "INSERT OR IGNORE INTO schema_flags(flag) VALUES (?1)",
            params![series_flag],
        )
        .unwrap_or(0)
        > 0
    {
        let _ = db.execute_batch(
            r#"
            UPDATE metadata_settings
            SET field_rules = json_set(
              json_set(
                field_rules,
                '$.series_name.providers',
                json('["amazon","hardcover","goodreads","itunes","openLibrary","google","audnexus"]')
              ),
              '$.series_index.providers',
              json('["amazon","hardcover","goodreads","itunes","openLibrary","google","audnexus"]')
            );
            "#,
        );
    }
}

pub(crate) fn on_user_ready(db: &Connection, user: i64) -> ApiResult<()> {
    ensure_settings(db, user)?;
    ensure_builtin_providers(db, user)?;
    Ok(())
}

fn owned_book(db: &Connection, user: i64, id: i64) -> ApiResult<()> {
    let ok: bool = db
        .query_row(
            "SELECT 1 FROM books WHERE id=? AND user_id=?",
            params![id, user],
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

#[derive(Deserialize)]
pub(super) struct SearchBody {
    title: Option<String>,
    author: Option<String>,
    isbn: Option<String>,
    query: Option<String>,
    series_name: Option<String>,
    series_index: Option<String>,
    /// When set, match tiers use the book's stored ISBNs / provider ids (BO MatchSubject).
    book_id: Option<i64>,
    #[serde(default)]
    is_audiobook: bool,
    providers: Option<Vec<String>>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    10
}

pub(super) async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SearchBody>,
) -> ApiResult<Json<Value>> {
    let want = body.providers;
    let book_id = body.book_id;
    let limit = body.limit.clamp(1, 20);
    let title = body.title.clone();
    let author = body.author.clone();
    let isbn = body.isbn.clone();
    let base_q = SearchQuery {
        title: body.title,
        author: body.author,
        isbn: body.isbn,
        query: body.query,
        series_name: body.series_name.clone(),
        series_index: body.series_index.clone(),
        is_audiobook: body.is_audiobook,
        config_json: None,
        limit,
    };
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            on_user_ready(db, user)?;

            let mut subject = rank::MatchSubject {
                title: title.clone(),
                authors: author
                    .as_deref()
                    .map(|a| vec![a.to_string()])
                    .unwrap_or_default(),
                isbn13: isbn.clone(),
                isbn10: None,
                series_name: body.series_name.clone(),
                series_index: body.series_index.clone(),
                provider_ids: vec![],
            };
            if let Some(id) = book_id {
                owned_book(db, user, id)?;
                ensure_book_metadata_row(db, id)?;
                let (isbn13, isbn10, series, series_idx, author_col, title_col): (
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<f64>,
                    String,
                    String,
                ) = db.query_row(
                    "SELECT m.isbn13, m.isbn10, b.series, b.series_index, b.author, b.title
                     FROM books b JOIN book_metadata m ON m.book_id=b.id WHERE b.id=?",
                    params![id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
                )?;
                if subject.title.is_none() {
                    subject.title = Some(title_col);
                }
                if subject.authors.is_empty() && !author_col.trim().is_empty() {
                    subject.authors = vec![author_col];
                }
                if subject.isbn13.as_deref().unwrap_or("").is_empty() {
                    subject.isbn13 = isbn13;
                }
                if subject.isbn10.as_deref().unwrap_or("").is_empty() {
                    subject.isbn10 = isbn10;
                }
                if subject.series_name.is_none() {
                    subject.series_name = series;
                }
                if subject.series_index.is_none() {
                    subject.series_index = series_idx.map(|n| n.to_string());
                }
                let mut id_stmt = db.prepare(
                    "SELECT provider, external_id FROM book_provider_ids WHERE book_id=?",
                )?;
                subject.provider_ids = id_stmt
                    .query_map(params![id], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
            }

            let search_title = subject
                .title
                .as_deref()
                .map(|t| {
                    let core = rank::title_core(t);
                    if core.is_empty() {
                        t.to_string()
                    } else {
                        t.split(':').next().unwrap_or(t).trim().to_string()
                    }
                })
                .unwrap_or_default();
            let search_author = subject.authors.first().cloned().unwrap_or_default();
            let search_isbn = subject
                .isbn13
                .as_deref()
                .or(subject.isbn10.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty());

            let mut stmt = db.prepare(
                "SELECT key, name, enabled, kind, base_url, api_key, config_json FROM metadata_providers
                 WHERE user_id=? ORDER BY display_order, id",
            )?;
            type ProviderListRow = (
                String,
                String,
                bool,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
            );
            let rows: Vec<ProviderListRow> = stmt
                .query_map(params![user], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get::<_, i64>(2)? != 0,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut provider_order = Vec::new();
            let mut candidates = Vec::new();
            for (key, _name, enabled, kind, base_url, api_key, config_json) in rows {
                if !enabled {
                    continue;
                }
                if let Some(ref filter) = want {
                    if !filter.iter().any(|p| p == &key) {
                        continue;
                    }
                }
                provider_order.push(key.clone());
                let q = with_provider_ctx(base_q.clone(), config_json);
                let key_for_search = key.clone();
                let kind_owned = kind.clone();
                let base_url_owned = base_url.clone();
                let api_key_owned = api_key.clone();
                let search_fn = move |query: &SearchQuery| {
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
                        search_builtin(&key_for_search, query, api_key_owned.as_deref())
                            .unwrap_or_default()
                    }
                };
                let found = fetch::search_and_rank_provider_list(
                    search_fn,
                    &q,
                    &search_title,
                    &search_author,
                    search_isbn,
                );
                candidates.extend(found);
            }
            Ok(Json(rank::rank_and_group(candidates, &subject, &provider_order)))
        })
        .await
}

#[derive(Deserialize)]
pub(super) struct ApplyBody {
    candidate: Value,
    fields: Vec<String>,
    #[serde(default = "default_true")]
    respect_locks: bool,
}

fn default_true() -> bool {
    true
}

fn candidate_from_json(v: &Value) -> ApiResult<Candidate> {
    Ok(Candidate {
        provider: v
            .get("provider")
            .and_then(|x| x.as_str())
            .ok_or(ApiError(
                StatusCode::BAD_REQUEST,
                "candidate.provider required.",
            ))?
            .to_string(),
        external_id: v
            .get("external_id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        title: v.get("title").and_then(|x| x.as_str()).map(str::to_string),
        subtitle: v
            .get("subtitle")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        description: v
            .get("description")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        authors: v
            .get("authors")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|i| i.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        narrators: v
            .get("narrators")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|i| i.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        genres: v
            .get("genres")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|i| i.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        tags: v
            .get("tags")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|i| i.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        series_name: v
            .get("series_name")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        series_index: v.get("series_index").and_then(|x| {
            x.as_str()
                .map(str::to_string)
                .or_else(|| x.as_f64().map(|n| n.to_string()))
        }),
        publisher: v
            .get("publisher")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        published_date: v
            .get("published_date")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        published_year: v.get("published_year").and_then(|x| x.as_i64()),
        language: v
            .get("language")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        page_count: v.get("page_count").and_then(|x| x.as_i64()),
        isbn10: v.get("isbn10").and_then(|x| x.as_str()).map(str::to_string),
        isbn13: v.get("isbn13").and_then(|x| x.as_str()).map(str::to_string),
        cover_url: v
            .get("cover_url")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        audio_cover_url: v
            .get("audio_cover_url")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        source_url: v
            .get("source_url")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        community_rating: v.get("community_rating").and_then(|x| x.as_f64()),
    })
}

pub(super) async fn apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<ApplyBody>,
) -> ApiResult<Json<Value>> {
    let cand = candidate_from_json(&body.candidate)?;
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, id)?;
            on_user_ready(db, user)?;
            let result = pipeline::apply_selected_fields(
                db,
                user,
                id,
                &cand,
                &body.fields,
                body.respect_locks,
            )?;
            Ok(Json(result))
        })
        .await
}

pub(super) async fn autofill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    // Search with book title/author, then apply field rules.
    let (title, author, user_for_spawn) = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                owned_book(db, user, id)?;
                on_user_ready(db, user)?;
                let (title, author): (String, String) = db.query_row(
                    "SELECT title, author FROM books WHERE id=?",
                    params![id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                Ok((title, author, user))
            }
        })
        .await?;
    let by_provider = state
        .db(move |db| {
            // Manual autofill uses the same BO fallbacks as the queue worker.
            fetch::collect_provider_candidates(db, user_for_spawn, &title, &author, None)
        })
        .await?;
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let result = pipeline::autofill_from_candidates(db, user, id, &by_provider, false)?;
            Ok(Json(result))
        })
        .await
}

pub(super) async fn get_book_metadata(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, id)?;
            Ok(Json(metadata_json_for_book(db, id)?))
        })
        .await
}

#[derive(Deserialize)]
pub(super) struct PatchMeta {
    subtitle: Option<Option<String>>,
    description: Option<Option<String>>,
    isbn10: Option<Option<String>>,
    isbn13: Option<Option<String>>,
    publisher: Option<Option<String>>,
    published_date: Option<Option<String>>,
    published_year: Option<Option<i64>>,
    language: Option<Option<String>>,
    page_count: Option<Option<i64>>,
    rating: Option<Option<i64>>,
    authors: Option<Vec<String>>,
    genres: Option<Vec<String>>,
    tags: Option<Vec<String>>,
    narrators: Option<Vec<String>>,
    series_name: Option<Option<String>>,
    series_index: Option<Option<String>>,
    locked_fields: Option<Vec<String>>,
}

pub(super) async fn patch_book_metadata(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<PatchMeta>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, id)?;
            ensure_book_metadata_row(db, id)?;
            let mut sets = Vec::new();
            let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            macro_rules! opt_str {
                ($field:ident, $col:expr) => {
                    if let Some(v) = &body.$field {
                        sets.push(format!("{}=?", $col));
                        vals.push(Box::new(v.clone()));
                    }
                };
            }
            opt_str!(subtitle, "subtitle");
            opt_str!(description, "description");
            opt_str!(isbn10, "isbn10");
            opt_str!(isbn13, "isbn13");
            opt_str!(publisher, "publisher");
            opt_str!(published_date, "published_date");
            opt_str!(language, "language");
            if let Some(v) = &body.published_year {
                sets.push("published_year=?".into());
                vals.push(Box::new(*v));
            }
            if let Some(v) = &body.page_count {
                sets.push("page_count=?".into());
                vals.push(Box::new(*v));
            }
            if let Some(v) = &body.rating {
                sets.push("rating=?".into());
                vals.push(Box::new(*v));
            }
            if let Some(locks) = &body.locked_fields {
                sets.push("locked_fields=?".into());
                vals.push(Box::new(
                    serde_json::to_string(locks).unwrap_or_else(|_| "[]".into()),
                ));
            }
            if !sets.is_empty() {
                sets.push("updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')".into());
                let sql = format!(
                    "UPDATE book_metadata SET {} WHERE book_id=?",
                    sets.join(", ")
                );
                vals.push(Box::new(id));
                let refs: Vec<&dyn rusqlite::ToSql> = vals.iter().map(|v| v.as_ref()).collect();
                db.execute(&sql, refs.as_slice())?;
            }
            if let Some(authors) = &body.authors {
                set_book_authors(db, user, id, authors)?;
            }
            if let Some(g) = &body.genres {
                pipeline::set_book_genres(db, user, id, g)?;
            }
            if let Some(t) = &body.tags {
                pipeline::set_book_tags(db, user, id, t)?;
            }
            if let Some(n) = &body.narrators {
                pipeline::set_book_narrators(db, user, id, n)?;
            }
            if body.series_name.is_some() || body.series_index.is_some() {
                let cur_series: Option<String> =
                    db.query_row("SELECT series FROM books WHERE id=?", params![id], |r| {
                        r.get(0)
                    })?;
                let series_name = body.series_name.clone().unwrap_or(cur_series);
                let idx = body.series_index.clone().flatten();
                set_book_series(db, user, id, series_name.as_deref(), idx.as_deref())?;
            }
            let _ = pipeline::recompute_score(db, user, id)?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn list_providers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            on_user_ready(db, user)?;
            let mut stmt = db.prepare(
                "SELECT id, key, name, enabled, kind, base_url, api_key IS NOT NULL, display_order
                 FROM metadata_providers WHERE user_id=? ORDER BY display_order, id",
            )?;
            let rows = stmt
                .query_map(params![user], |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "key": r.get::<_, String>(1)?,
                        "name": r.get::<_, String>(2)?,
                        "enabled": r.get::<_, i64>(3)? != 0,
                        "kind": r.get::<_, String>(4)?,
                        "base_url": r.get::<_, Option<String>>(5)?,
                        "has_api_key": r.get::<_, bool>(6)?,
                        "display_order": r.get::<_, i64>(7)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Json(json!(rows)))
        })
        .await
}

#[derive(Deserialize)]
pub(super) struct PatchProvider {
    enabled: Option<bool>,
    api_key: Option<Option<String>>,
    base_url: Option<Option<String>>,
    name: Option<String>,
    config_json: Option<String>,
}

pub(super) async fn patch_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<PatchProvider>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let exists: bool = db
                .query_row(
                    "SELECT 1 FROM metadata_providers WHERE id=? AND user_id=?",
                    params![id, user],
                    |_| Ok(true),
                )
                .optional()?
                .unwrap_or(false);
            if !exists {
                return Err(ApiError(StatusCode::NOT_FOUND, "Provider not found."));
            }
            if let Some(en) = body.enabled {
                db.execute(
                    "UPDATE metadata_providers SET enabled=? WHERE id=?",
                    params![if en { 1 } else { 0 }, id],
                )?;
            }
            if let Some(key) = body.api_key {
                db.execute(
                    "UPDATE metadata_providers SET api_key=? WHERE id=?",
                    params![key, id],
                )?;
            }
            if let Some(url) = body.base_url {
                db.execute(
                    "UPDATE metadata_providers SET base_url=? WHERE id=?",
                    params![url, id],
                )?;
            }
            if let Some(name) = body.name {
                db.execute(
                    "UPDATE metadata_providers SET name=? WHERE id=?",
                    params![name.trim(), id],
                )?;
            }
            if let Some(cfg) = body.config_json {
                let raw = cfg.trim();
                let parsed: Value = if raw.is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(raw).map_err(|_| {
                        ApiError(StatusCode::BAD_REQUEST, "config_json must be valid JSON.")
                    })?
                };
                db.execute(
                    "UPDATE metadata_providers SET config_json=? WHERE id=?",
                    params![parsed.to_string(), id],
                )?;
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

#[derive(Deserialize)]
pub(super) struct NewExternalProvider {
    key: String,
    name: String,
    base_url: String,
    api_key: Option<String>,
}

pub(super) async fn add_external_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<NewExternalProvider>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let key = body.key.trim().to_string();
    if key.len() < 2
        || key.len() > 32
        || !key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Provider key must be 2–32 chars: a-z, 0-9, hyphen.",
        ));
    }
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            on_user_ready(db, user)?;
            db.execute(
                "INSERT INTO metadata_providers(user_id, key, name, enabled, kind, base_url, api_key, display_order)
                 VALUES (?,?,?,1,'external',?,?,100)",
                params![user, key, body.name.trim(), body.base_url.trim(), body.api_key],
            )?;
            Ok((
                StatusCode::CREATED,
                Json(json!({"id": db.last_insert_rowid()})),
            ))
        })
        .await
}

pub(super) async fn get_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            on_user_ready(db, user)?;
            let (rules, weights, fetch, paused): (String, String, String, i64) = db.query_row(
                "SELECT field_rules, score_weights, fetch_config, paused FROM metadata_settings WHERE user_id=?",
                params![user],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
            Ok(Json(json!({
                "field_rules": serde_json::from_str::<Value>(&rules).unwrap_or(json!({})),
                "score_weights": serde_json::from_str::<Value>(&weights).unwrap_or(json!({})),
                "fetch_config": serde_json::from_str::<Value>(&fetch).unwrap_or(json!({})),
                "paused": paused != 0,
            })))
        })
        .await
}

#[derive(Deserialize)]
pub(super) struct PutSettings {
    field_rules: Option<Value>,
    score_weights: Option<Value>,
    fetch_config: Option<Value>,
    paused: Option<bool>,
}

pub(super) async fn put_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PutSettings>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            on_user_ready(db, user)?;
            if let Some(v) = body.field_rules {
                db.execute(
                    "UPDATE metadata_settings SET field_rules=?, updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE user_id=?",
                    params![v.to_string(), user],
                )?;
            }
            if let Some(v) = body.score_weights {
                db.execute(
                    "UPDATE metadata_settings SET score_weights=?, updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE user_id=?",
                    params![v.to_string(), user],
                )?;
            }
            if let Some(v) = body.fetch_config {
                db.execute(
                    "UPDATE metadata_settings SET fetch_config=?, updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE user_id=?",
                    params![v.to_string(), user],
                )?;
            }
            if let Some(p) = body.paused {
                db.execute(
                    "UPDATE metadata_settings SET paused=? WHERE user_id=?",
                    params![if p { 1 } else { 0 }, user],
                )?;
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn list_covers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, id)?;
            let mut stmt = db.prepare(
                "SELECT id, medium, source, origin, provider, active FROM book_covers WHERE book_id=? ORDER BY id",
            )?;
            let rows = stmt
                .query_map(params![id], |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "medium": r.get::<_, String>(1)?,
                        "source": r.get::<_, String>(2)?,
                        "origin": r.get::<_, String>(3)?,
                        "provider": r.get::<_, Option<String>>(4)?,
                        "active": r.get::<_, i64>(5)? != 0,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Json(json!(rows)))
        })
        .await
}

pub(super) async fn activate_cover(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((book_id, cover_id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, book_id)?;
            let medium: String = db
                .query_row(
                    "SELECT medium FROM book_covers WHERE id=? AND book_id=?",
                    params![cover_id, book_id],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Cover not found."))?;
            db.execute(
                "UPDATE book_covers SET active=0 WHERE book_id=? AND medium=?",
                params![book_id, medium],
            )?;
            db.execute(
                "UPDATE book_covers SET active=1 WHERE id=?",
                params![cover_id],
            )?;
            if medium == "ebook" {
                let (bytes, mime): (Vec<u8>, Option<String>) = db.query_row(
                    "SELECT bytes, mime FROM book_covers WHERE id=?",
                    params![cover_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                db.execute(
                    "UPDATE epub_files SET cover=?, cover_mime=? WHERE book_id=?",
                    params![bytes, mime, book_id],
                )?;
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn cover_bytes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((book_id, cover_id)): Path<(i64, i64)>,
) -> ApiResult<Response> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, book_id)?;
            let (bytes, mime): (Vec<u8>, Option<String>) = db
                .query_row(
                    "SELECT bytes, mime FROM book_covers WHERE id=? AND book_id=?",
                    params![cover_id, book_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Cover not found."))?;
            let mime = mime.unwrap_or_else(|| "image/jpeg".into());
            Ok(([(header::CONTENT_TYPE, mime)], bytes).into_response())
        })
        .await
}

#[derive(Deserialize)]
pub(super) struct CoverFromUrl {
    url: String,
    #[serde(default = "ebook_medium")]
    medium: String,
}

fn ebook_medium() -> String {
    "ebook".into()
}

#[derive(Deserialize)]
pub(super) struct CoverSearchBody {
    title: Option<String>,
    author: Option<String>,
    #[serde(default)]
    audiobook: bool,
}

pub(super) async fn search_covers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<CoverSearchBody>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, id)?;
            let (book_title, book_author): (String, Option<String>) = db.query_row(
                "SELECT title, author FROM books WHERE id=?",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let title = body
                .title
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(book_title.as_str());
            let author = body
                .author
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .or(book_author
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty()));
            let title = title.to_string();
            let author = author.map(str::to_string);
            let audiobook = body.audiobook;
            let hits = search_cover_hits(&title, author.as_deref(), audiobook).unwrap_or_default();
            Ok(Json(json!({
                "results": hits.iter().map(cover_hit_json).collect::<Vec<_>>(),
            })))
        })
        .await
}

pub(super) async fn add_cover_url(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<CoverFromUrl>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let medium = if body.medium == "audio" {
        "audio"
    } else {
        "ebook"
    };
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, id)?;
            ensure_book_metadata_row(db, id)?;
            let cid = pipeline::store_cover_from_url(db, id, medium, &body.url, None, true)?;
            Ok((StatusCode::CREATED, Json(json!({"id": cid}))))
        })
        .await
}

// Fetch queue HTTP — thin wrappers over fetch.rs
pub(super) async fn fetch_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            on_user_ready(db, user)?;
            Ok(Json(fetch::status(db, user)?))
        })
        .await
}

#[derive(Deserialize)]
pub(super) struct RunBody {
    #[serde(default)]
    ignore_cache: bool,
}

pub(super) async fn fetch_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RunBody>,
) -> ApiResult<Json<Value>> {
    let spawn = state.clone();
    let queued = state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            on_user_ready(db, user)?;
            fetch::enqueue_run(db, user, body.ignore_cache)
        })
        .await?;
    fetch::kick(spawn);
    Ok(Json(json!({"queued": queued})))
}

pub(super) async fn refresh_book(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let spawn = state.clone();
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, id)?;
            on_user_ready(db, user)?;
            fetch::enqueue_book(db, user, id, "refresh", true)?;
            Ok(Json(json!({"queued": true})))
        })
        .await
        .inspect(|_| {
            fetch::kick(spawn);
        })
}

pub(super) async fn fetch_pause(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            on_user_ready(db, user)?;
            db.execute(
                "UPDATE metadata_settings SET paused=1 WHERE user_id=?",
                params![user],
            )?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn fetch_resume(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    let spawn = state.clone();
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            on_user_ready(db, user)?;
            db.execute(
                "UPDATE metadata_settings SET paused=0 WHERE user_id=?",
                params![user],
            )?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
        .inspect(|_| {
            fetch::kick(spawn);
        })
}

pub(super) async fn fetch_cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let n = db.execute(
                "UPDATE metadata_fetch_jobs SET status='cancelled', updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE user_id=? AND status IN ('queued','processing')",
                params![user],
            )?;
            Ok(Json(json!({"cancelled": n})))
        })
        .await
}

#[derive(Deserialize)]
pub(super) struct ProxyImageQuery {
    url: String,
}

/// Same-origin cover preview so CSP `img-src 'self'` can show provider art.
pub(super) async fn proxy_image(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProxyImageQuery>,
) -> ApiResult<Response> {
    let url = query.url.trim().to_string();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Cover URL must be http(s).",
        ));
    }
    if url.len() > 2048 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Cover URL is too long."));
    }
    // Auth only — fetch runs outside the DB lock.
    state
        .db({
            let headers = headers.clone();
            move |db| {
                let _user = authenticate(db, &headers)?;
                Ok(())
            }
        })
        .await?;
    let (bytes, mime) = tokio::task::spawn_blocking(move || download_cover_bytes(&url))
        .await
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "Cover proxy failed."))??;
    Ok(([(header::CONTENT_TYPE, mime)], bytes).into_response())
}
