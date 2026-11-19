//! Field rules, score, apply, and entity dual-write.

use super::providers::{download_cover_bytes, Candidate};
use super::*;
use serde_json::{json, Value};
use std::collections::HashSet;

#[allow(dead_code)]
pub const STRATEGIES: &[&str] = &["fill_missing", "overwrite_if_provided", "always_overwrite"];

pub fn default_field_rules() -> Value {
    let fields = [
        "title",
        "subtitle",
        "description",
        "authors",
        "narrators",
        "genres",
        "tags",
        "series_name",
        "series_index",
        "publisher",
        "published_date",
        "published_year",
        "language",
        "page_count",
        "isbn10",
        "isbn13",
        "cover",
        "audio_cover",
    ];
    let mut rules = serde_json::Map::new();
    for f in fields {
        let providers = match f {
            "cover" => json!(["itunes", "google", "openLibrary", "goodreads", "amazon"]),
            "audio_cover" => json!(["audnexus", "itunes", "audible", "librofm"]),
            "narrators" => json!(["audnexus", "audible"]),
            "series_name" | "series_index" => {
                json!([
                    "amazon",
                    "hardcover",
                    "goodreads",
                    "itunes",
                    "openLibrary",
                    "google",
                    "audnexus"
                ])
            }
            _ => json!([
                "openLibrary",
                "google",
                "itunes",
                "audnexus",
                "hardcover",
                "goodreads",
                "amazon"
            ]),
        };
        let strategy = if f == "title" || f == "authors" {
            "fill_missing"
        } else {
            "overwrite_if_provided"
        };
        rules.insert(
            f.to_string(),
            json!({"enabled": true, "providers": providers, "strategy": strategy}),
        );
    }
    Value::Object(rules)
}

pub fn default_score_weights() -> Value {
    json!({
        "title": 10, "authors": 10, "cover": 10, "description": 8, "isbn13": 7,
        "genres": 6, "publisher": 3, "published_year": 3, "language": 2, "page_count": 2,
        "series_name": 0, "subtitle": 0, "series_index": 0
    })
}

pub fn default_fetch_config() -> Value {
    json!({
        "enabled": true,
        "trigger_on_import": true,
        "conditions": {
            "never_fetched": true,
            "low_score_below": 60,
            "missing_fields": ["description", "cover"]
        }
    })
}

pub fn ensure_settings(db: &Connection, user: i64) -> ApiResult<()> {
    db.execute(
        "INSERT OR IGNORE INTO metadata_settings(user_id, field_rules, score_weights, fetch_config)
         VALUES (?,?,?,?)",
        params![
            user,
            default_field_rules().to_string(),
            default_score_weights().to_string(),
            default_fetch_config().to_string()
        ],
    )?;
    Ok(())
}

pub fn ensure_builtin_providers(db: &Connection, user: i64) -> ApiResult<()> {
    for (i, (key, name, working)) in providers::builtin_defs().into_iter().enumerate() {
        let enabled = if working { 1 } else { 0 };
        db.execute(
            "INSERT OR IGNORE INTO metadata_providers(user_id, key, name, enabled, kind, display_order)
             VALUES (?,?,?,?, 'builtin', ?)",
            params![user, key, name, enabled, i as i64],
        )?;
    }
    Ok(())
}

pub fn ensure_book_metadata_row(db: &Connection, book_id: i64) -> ApiResult<()> {
    db.execute(
        "INSERT OR IGNORE INTO book_metadata(book_id) VALUES (?)",
        params![book_id],
    )?;
    Ok(())
}

fn parse_locks(raw: &str) -> HashSet<String> {
    serde_json::from_str::<Vec<String>>(raw)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

pub fn locked_fields(db: &Connection, book_id: i64) -> ApiResult<HashSet<String>> {
    ensure_book_metadata_row(db, book_id)?;
    let raw: String = db.query_row(
        "SELECT locked_fields FROM book_metadata WHERE book_id=?",
        params![book_id],
        |r| r.get(0),
    )?;
    Ok(parse_locks(&raw))
}

fn normalize_name(name: &str) -> String {
    name.trim().to_lowercase()
}

pub fn upsert_author(db: &Connection, user: i64, name: &str) -> ApiResult<i64> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Author name required."));
    }
    db.execute(
        "INSERT INTO authors(user_id, name, sort_name) VALUES (?,?,?)
         ON CONFLICT(user_id, name) DO NOTHING",
        params![user, name, name],
    )?;
    let id: i64 = db.query_row(
        "SELECT id FROM authors WHERE user_id=? AND name=?",
        params![user, name],
        |r| r.get(0),
    )?;
    Ok(id)
}

pub fn set_book_authors(
    db: &Connection,
    user: i64,
    book_id: i64,
    names: &[String],
) -> ApiResult<()> {
    db.execute("DELETE FROM book_authors WHERE book_id=?", params![book_id])?;
    for (i, name) in names.iter().enumerate() {
        let id = upsert_author(db, user, name)?;
        db.execute(
            "INSERT OR IGNORE INTO book_authors(book_id, author_id, display_order) VALUES (?,?,?)",
            params![book_id, id, i as i64],
        )?;
    }
    let primary = names
        .first()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("Unknown");
    db.execute(
        "UPDATE books SET author=? WHERE id=? AND user_id=?",
        params![primary, book_id, user],
    )?;
    Ok(())
}

pub fn upsert_series(db: &Connection, user: i64, name: &str) -> ApiResult<i64> {
    let name = name.trim();
    let norm = normalize_name(name);
    db.execute(
        "INSERT INTO series(user_id, name, normalized_name) VALUES (?,?,?)
         ON CONFLICT(user_id, normalized_name) DO NOTHING",
        params![user, name, norm],
    )?;
    let id: i64 = db.query_row(
        "SELECT id FROM series WHERE user_id=? AND normalized_name=?",
        params![user, norm],
        |r| r.get(0),
    )?;
    Ok(id)
}

pub fn set_book_series(
    db: &Connection,
    user: i64,
    book_id: i64,
    series_name: Option<&str>,
    series_index: Option<&str>,
) -> ApiResult<()> {
    db.execute(
        "DELETE FROM book_series_memberships WHERE book_id=?",
        params![book_id],
    )?;
    if let Some(name) = series_name.map(str::trim).filter(|s| !s.is_empty()) {
        let sid = upsert_series(db, user, name)?;
        db.execute(
            "INSERT INTO book_series_memberships(book_id, series_id, series_index, display_order) VALUES (?,?,?,0)",
            params![book_id, sid, series_index],
        )?;
        let idx_f: Option<f64> = series_index.and_then(|s| s.parse().ok());
        db.execute(
            "UPDATE books SET series=?, series_index=? WHERE id=? AND user_id=?",
            params![name, idx_f, book_id, user],
        )?;
    } else {
        db.execute(
            "UPDATE books SET series=NULL, series_index=NULL WHERE id=? AND user_id=?",
            params![book_id, user],
        )?;
    }
    Ok(())
}

fn upsert_named(db: &Connection, user: i64, table: &str, name: &str) -> ApiResult<i64> {
    let name = name.trim();
    let sql_ins = format!(
        "INSERT INTO {table}(user_id, name) VALUES (?,?) ON CONFLICT(user_id, name) DO NOTHING"
    );
    db.execute(&sql_ins, params![user, name])?;
    let sql_sel = format!("SELECT id FROM {table} WHERE user_id=? AND name=?");
    Ok(db.query_row(&sql_sel, params![user, name], |r| r.get(0))?)
}

fn set_book_named_join(
    db: &Connection,
    user: i64,
    book_id: i64,
    entity_table: &str,
    join_table: &str,
    fk: &str,
    names: &[String],
) -> ApiResult<()> {
    db.execute(
        &format!("DELETE FROM {join_table} WHERE book_id=?"),
        params![book_id],
    )?;
    for name in names {
        if name.trim().is_empty() {
            continue;
        }
        let id = upsert_named(db, user, entity_table, name)?;
        db.execute(
            &format!("INSERT OR IGNORE INTO {join_table}(book_id, {fk}) VALUES (?,?)"),
            params![book_id, id],
        )?;
    }
    Ok(())
}

/// Turn provider genre noise into shelf labels.
///
/// Retail aisles like iTunes "Sci-Fi & Fantasy" are not a genre. Google/BISAC
/// paths ("Fiction / Fantasy / Epic") keep the specific leaf. Open Library
/// subject dumps are filtered elsewhere before they reach here.
pub fn normalize_genres(names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for raw in names {
        for piece in expand_genre_label(raw) {
            let key = normalize_name(&piece);
            if key.is_empty() || !seen.insert(key) {
                continue;
            }
            out.push(piece);
            if out.len() >= 6 {
                return out;
            }
        }
    }
    out
}

fn expand_genre_label(raw: &str) -> Vec<String> {
    let t = raw.trim();
    if t.is_empty() {
        return Vec::new();
    }
    let lower = t.to_lowercase();
    // Apple / store aisle buckets — not usable as a single shelf genre.
    if matches!(
        lower.as_str(),
        "sci-fi & fantasy"
            | "science fiction & fantasy"
            | "literature & fiction"
            | "comics & graphic novels"
            | "mysteries & thrillers"
            | "books"
            | "kindle books"
            | "audible books & originals"
    ) {
        return Vec::new();
    }
    // Google / BISAC-style path: prefer a real genre segment over Fiction/General.
    if t.contains(" / ") {
        let parts: Vec<&str> = t
            .split(" / ")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        let has_fantasy = parts.iter().any(|p| p.eq_ignore_ascii_case("fantasy"));
        for part in parts.iter().rev() {
            let leaf_l = part.to_lowercase();
            if leaf_l == "fiction" || leaf_l == "general" {
                continue;
            }
            if leaf_l == "epic" && has_fantasy {
                return vec!["Epic Fantasy".into()];
            }
            if leaf_l == "fantasy" {
                return vec!["Fantasy".into()];
            }
            return vec![title_case_genre(part)];
        }
        return Vec::new();
    }
    // "Fantasy fiction" / "English Fantasy fiction" → Fantasy
    for marker in [
        "fantasy",
        "science fiction",
        "sci-fi",
        "mystery",
        "thriller",
        "horror",
        "romance",
        "historical fiction",
        "young adult",
        "adventure",
        "crime",
        "western",
        "dystopian",
        "steampunk",
        "urban fantasy",
        "epic fantasy",
        "literary fiction",
    ] {
        if lower == marker
            || lower.starts_with(&format!("{marker} "))
            || lower.ends_with(&format!(" {marker}"))
            || lower.contains(&format!(" {marker} "))
        {
            return vec![title_case_genre(marker)];
        }
    }
    // Drop OL setting/character noise that slipped through.
    if looks_like_subject_noise(&lower) {
        return Vec::new();
    }
    vec![t.to_string()]
}

fn title_case_genre(s: &str) -> String {
    let s = s.trim();
    if s.eq_ignore_ascii_case("sci-fi") {
        return "Sci-Fi".into();
    }
    if s.eq_ignore_ascii_case("young adult") {
        return "Young Adult".into();
    }
    s.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn looks_like_subject_noise(lower: &str) -> bool {
    lower.contains("(fictitious character)")
        || lower.contains("imaginary place")
        || lower.contains("translations")
        || lower.contains("history and criticism")
        || lower.starts_with("open library")
        || matches!(
            lower,
            "elves"
                | "dwarves"
                | "evil"
                | "fear"
                | "hope"
                | "fiction"
                | "english fiction"
                | "english literature"
                | "fairy tales"
                | "adventure stories"
                | "adventure fiction"
        )
}

pub fn set_book_genres(
    db: &Connection,
    user: i64,
    book_id: i64,
    names: &[String],
) -> ApiResult<()> {
    let cleaned = normalize_genres(names);
    set_book_named_join(
        db,
        user,
        book_id,
        "genres",
        "book_genres",
        "genre_id",
        &cleaned,
    )
}
pub fn set_book_tags(db: &Connection, user: i64, book_id: i64, names: &[String]) -> ApiResult<()> {
    set_book_named_join(db, user, book_id, "tags", "book_tags", "tag_id", names)
}
pub fn set_book_narrators(
    db: &Connection,
    user: i64,
    book_id: i64,
    names: &[String],
) -> ApiResult<()> {
    set_book_named_join(
        db,
        user,
        book_id,
        "narrators",
        "book_narrators",
        "narrator_id",
        names,
    )
}

fn current_str(db: &Connection, book_id: i64, col: &str) -> ApiResult<Option<String>> {
    ensure_book_metadata_row(db, book_id)?;
    let sql = format!("SELECT {col} FROM book_metadata WHERE book_id=?");
    Ok(db.query_row(&sql, params![book_id], |r| r.get(0))?)
}

fn book_title_author(db: &Connection, book_id: i64) -> ApiResult<(String, String)> {
    Ok(db.query_row(
        "SELECT title, author FROM books WHERE id=?",
        params![book_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

fn has_active_cover(db: &Connection, book_id: i64, medium: &str) -> ApiResult<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM book_covers WHERE book_id=? AND medium=? AND active=1)",
        params![book_id, medium],
        |r| r.get(0),
    )?)
}

fn field_empty(db: &Connection, book_id: i64, field: &str) -> ApiResult<bool> {
    Ok(match field {
        "title" | "authors" => {
            let (t, a) = book_title_author(db, book_id)?;
            if field == "title" {
                t.trim().is_empty()
            } else {
                a.trim().is_empty() || a == "Unknown"
            }
        }
        "cover" => !has_active_cover(db, book_id, "ebook")?,
        "audio_cover" => !has_active_cover(db, book_id, "audio")?,
        "series_name" => {
            let s: Option<String> = db.query_row(
                "SELECT series FROM books WHERE id=?",
                params![book_id],
                |r| r.get(0),
            )?;
            s.as_deref().map(str::trim).unwrap_or("").is_empty()
        }
        "series_index" => {
            let s: Option<f64> = db.query_row(
                "SELECT series_index FROM books WHERE id=?",
                params![book_id],
                |r| r.get(0),
            )?;
            s.is_none()
        }
        "genres" => {
            let n: i64 = db.query_row(
                "SELECT COUNT(*) FROM book_genres WHERE book_id=?",
                params![book_id],
                |r| r.get(0),
            )?;
            n == 0
        }
        "tags" => {
            let n: i64 = db.query_row(
                "SELECT COUNT(*) FROM book_tags WHERE book_id=?",
                params![book_id],
                |r| r.get(0),
            )?;
            n == 0
        }
        "narrators" => {
            let n: i64 = db.query_row(
                "SELECT COUNT(*) FROM book_narrators WHERE book_id=?",
                params![book_id],
                |r| r.get(0),
            )?;
            n == 0
        }
        "published_year" | "page_count" => {
            ensure_book_metadata_row(db, book_id)?;
            let n: Option<i64> = db.query_row(
                &format!("SELECT {field} FROM book_metadata WHERE book_id=?"),
                params![book_id],
                |r| r.get(0),
            )?;
            n.is_none() || n == Some(0)
        }
        other => current_str(
            db,
            book_id,
            match other {
                "subtitle" => "subtitle",
                "description" => "description",
                "publisher" => "publisher",
                "published_date" => "published_date",
                "language" => "language",
                "isbn10" => "isbn10",
                "isbn13" => "isbn13",
                _ => return Ok(true),
            },
        )?
        .map(|s| s.trim().is_empty())
        .unwrap_or(true),
    })
}

fn should_write(strategy: &str, empty: bool, provided: bool) -> bool {
    if !provided {
        return false;
    }
    match strategy {
        "fill_missing" => empty,
        "always_overwrite" => true,
        _ => true, // overwrite_if_provided
    }
}

fn candidate_field(c: &Candidate, field: &str) -> Option<Value> {
    match field {
        "title" => c.title.as_ref().map(|s| json!(s)),
        "subtitle" => c.subtitle.as_ref().map(|s| json!(s)),
        "description" => c.description.as_ref().map(|s| json!(s)),
        "authors" => (!c.authors.is_empty()).then(|| json!(c.authors)),
        "narrators" => (!c.narrators.is_empty()).then(|| json!(c.narrators)),
        "genres" => (!c.genres.is_empty()).then(|| json!(c.genres)),
        "tags" => (!c.tags.is_empty()).then(|| json!(c.tags)),
        "series_name" => c.series_name.as_ref().map(|s| json!(s)),
        "series_index" => c.series_index.as_ref().map(|s| json!(s)),
        "publisher" => c.publisher.as_ref().map(|s| json!(s)),
        "published_date" => c.published_date.as_ref().map(|s| json!(s)),
        "published_year" => c.published_year.map(|n| json!(n)),
        "language" => c.language.as_ref().map(|s| json!(s)),
        "page_count" => c.page_count.map(|n| json!(n)),
        "isbn10" => c.isbn10.as_ref().map(|s| json!(s)),
        "isbn13" => c.isbn13.as_ref().map(|s| json!(s)),
        "cover" => c.cover_url.as_ref().map(|s| json!(s)),
        "audio_cover" => c.audio_cover_url.as_ref().map(|s| json!(s)),
        _ => None,
    }
}

pub fn recompute_score(db: &Connection, user: i64, book_id: i64) -> ApiResult<i64> {
    ensure_settings(db, user)?;
    let weights_raw: String = db.query_row(
        "SELECT score_weights FROM metadata_settings WHERE user_id=?",
        params![user],
        |r| r.get(0),
    )?;
    let weights: Value =
        serde_json::from_str(&weights_raw).unwrap_or_else(|_| default_score_weights());
    let mut score = 0i64;
    let mut max = 0i64;
    if let Some(obj) = weights.as_object() {
        for (field, w) in obj {
            let w = w.as_i64().unwrap_or(0);
            if w <= 0 {
                continue;
            }
            max += w;
            if !field_empty(db, book_id, field)? {
                score += w;
            }
        }
    }
    let pct = if max == 0 { 0 } else { (score * 100) / max };
    ensure_book_metadata_row(db, book_id)?;
    db.execute(
        "UPDATE book_metadata SET metadata_score=?, updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE book_id=?",
        params![pct, book_id],
    )?;
    Ok(pct)
}

fn set_provider_id(
    db: &Connection,
    book_id: i64,
    provider: &str,
    external_id: &str,
    source_url: Option<&str>,
) -> ApiResult<()> {
    db.execute(
        "INSERT INTO book_provider_ids(book_id, provider, external_id, source_url) VALUES (?,?,?,?)
         ON CONFLICT(book_id, provider) DO UPDATE SET external_id=excluded.external_id, source_url=excluded.source_url",
        params![book_id, provider, external_id, source_url],
    )?;
    let col = match provider {
        "openLibrary" => Some("open_library_id"),
        "google" => Some("google_books_id"),
        "goodreads" => Some("goodreads_id"),
        "amazon" => Some("amazon_id"),
        "hardcover" => Some("hardcover_id"),
        "itunes" => Some("itunes_id"),
        "audible" => Some("audible_id"),
        "audnexus" => Some("audnexus_id"),
        "comicvine" => Some("comicvine_id"),
        _ => None,
    };
    if let Some(col) = col {
        db.execute(
            &format!("UPDATE book_metadata SET {col}=? WHERE book_id=?"),
            params![external_id, book_id],
        )?;
    }
    Ok(())
}

pub fn store_cover_from_url(
    db: &Connection,
    book_id: i64,
    medium: &str,
    url: &str,
    provider: Option<&str>,
    make_active: bool,
) -> ApiResult<i64> {
    let (bytes, mime) = download_cover_bytes(url)?;
    if make_active {
        db.execute(
            "UPDATE book_covers SET active=0 WHERE book_id=? AND medium=?",
            params![book_id, medium],
        )?;
    }
    db.execute(
        "INSERT INTO book_covers(book_id, medium, source, origin, provider, mime, bytes, active)
         VALUES (?,?, 'provider', 'provider', ?, ?, ?, ?)",
        params![
            book_id,
            medium,
            provider,
            mime,
            bytes,
            if make_active { 1 } else { 0 }
        ],
    )?;
    let id = db.last_insert_rowid();
    if make_active && medium == "ebook" {
        // Keep legacy epub_files.cover in sync for existing cover route.
        db.execute(
            "UPDATE epub_files SET cover=?, cover_mime=? WHERE book_id=?",
            params![
                db.query_row(
                    "SELECT bytes FROM book_covers WHERE id=?",
                    params![id],
                    |r| r.get::<_, Vec<u8>>(0)
                )?,
                db.query_row(
                    "SELECT mime FROM book_covers WHERE id=?",
                    params![id],
                    |r| r.get::<_, Option<String>>(0)
                )?,
                book_id
            ],
        )?;
        db.execute(
            "UPDATE book_metadata SET cover_source='custom' WHERE book_id=?",
            params![book_id],
        )?;
    }
    Ok(id)
}

fn apply_field_value(
    db: &Connection,
    user: i64,
    book_id: i64,
    field: &str,
    value: &Value,
    provider: &str,
) -> ApiResult<()> {
    match field {
        "title" => {
            if let Some(t) = value.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                db.execute(
                    "UPDATE books SET title=? WHERE id=? AND user_id=?",
                    params![t, book_id, user],
                )?;
            }
        }
        "authors" => {
            let names: Vec<String> = value
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if !names.is_empty() {
                set_book_authors(db, user, book_id, &names)?;
            }
        }
        "narrators" => {
            let names: Vec<String> = value
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            set_book_narrators(db, user, book_id, &names)?;
        }
        "genres" => {
            let names: Vec<String> = value
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            set_book_genres(db, user, book_id, &names)?;
        }
        "tags" => {
            let names: Vec<String> = value
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            set_book_tags(db, user, book_id, &names)?;
        }
        "series_name" => {
            let name = value.as_str();
            let idx: Option<String> = db
                .query_row(
                    "SELECT series_index FROM books WHERE id=?",
                    params![book_id],
                    |r| {
                        let v: Option<f64> = r.get(0)?;
                        Ok(v.map(|n| n.to_string()))
                    },
                )
                .optional()?
                .flatten();
            set_book_series(db, user, book_id, name, idx.as_deref())?;
        }
        "series_index" => {
            let idx = value
                .as_str()
                .map(str::to_string)
                .or_else(|| value.as_f64().map(|n| n.to_string()));
            let series: Option<String> = db.query_row(
                "SELECT series FROM books WHERE id=?",
                params![book_id],
                |r| r.get(0),
            )?;
            set_book_series(db, user, book_id, series.as_deref(), idx.as_deref())?;
        }
        "subtitle" | "description" | "publisher" | "published_date" | "language" | "isbn10"
        | "isbn13" => {
            ensure_book_metadata_row(db, book_id)?;
            if let Some(s) = value.as_str() {
                db.execute(
                    &format!("UPDATE book_metadata SET {field}=? WHERE book_id=?"),
                    params![s, book_id],
                )?;
            }
        }
        "published_year" | "page_count" => {
            ensure_book_metadata_row(db, book_id)?;
            if let Some(n) = value.as_i64() {
                db.execute(
                    &format!("UPDATE book_metadata SET {field}=? WHERE book_id=?"),
                    params![n, book_id],
                )?;
            }
        }
        "cover" => {
            if let Some(url) = value.as_str() {
                let _ = store_cover_from_url(db, book_id, "ebook", url, Some(provider), true)?;
            }
        }
        "audio_cover" => {
            if let Some(url) = value.as_str() {
                let _ = store_cover_from_url(db, book_id, "audio", url, Some(provider), true)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Apply selected fields from a candidate (manual compare/apply).
pub fn apply_selected_fields(
    db: &Connection,
    user: i64,
    book_id: i64,
    candidate: &Candidate,
    fields: &[String],
    respect_locks: bool,
) -> ApiResult<Value> {
    ensure_book_metadata_row(db, book_id)?;
    let locks = if respect_locks {
        locked_fields(db, book_id)?
    } else {
        HashSet::new()
    };
    let mut applied = Vec::new();
    for field in fields {
        if locks.contains(field) {
            continue;
        }
        if let Some(val) = candidate_field(candidate, field) {
            apply_field_value(db, user, book_id, field, &val, &candidate.provider)?;
            applied.push(field.clone());
        }
    }
    set_provider_id(
        db,
        book_id,
        &candidate.provider,
        &candidate.external_id,
        candidate.source_url.as_deref(),
    )?;
    db.execute(
        "UPDATE book_metadata SET last_fetch_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE book_id=?",
        params![book_id],
    )?;
    let score = recompute_score(db, user, book_id)?;
    Ok(json!({"applied": applied, "metadata_score": score}))
}

/// Autofill via field rules using an ordered list of candidates (best-first per provider already).
///
/// `preserve_existing` mirrors BO import behavior: never overwrite fields that already have values
/// (forces `fill_missing` for every rule), so embedded EPUB metadata survives the first fetch.
pub fn autofill_from_candidates(
    db: &Connection,
    user: i64,
    book_id: i64,
    by_provider: &[(String, Candidate)],
    preserve_existing: bool,
) -> ApiResult<Value> {
    ensure_settings(db, user)?;
    ensure_book_metadata_row(db, book_id)?;
    let rules_raw: String = db.query_row(
        "SELECT field_rules FROM metadata_settings WHERE user_id=?",
        params![user],
        |r| r.get(0),
    )?;
    let rules: Value = serde_json::from_str(&rules_raw).unwrap_or_else(|_| default_field_rules());
    let locks = locked_fields(db, book_id)?;
    let mut applied = Vec::new();
    let Some(rule_obj) = rules.as_object() else {
        return Ok(json!({"applied": applied}));
    };
    for (field, rule) in rule_obj {
        if locks.contains(field) {
            continue;
        }
        if rule.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
            continue;
        }
        let strategy = if preserve_existing {
            "fill_missing"
        } else {
            rule.get("strategy")
                .and_then(|v| v.as_str())
                .unwrap_or("overwrite_if_provided")
        };
        let providers = rule
            .get("providers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let empty = field_empty(db, book_id, field)?;
        for p in providers {
            let Some(pkey) = p.as_str() else {
                continue;
            };
            let Some((_, cand)) = by_provider.iter().find(|(k, _)| k == pkey) else {
                continue;
            };
            let Some(val) = candidate_field(cand, field) else {
                continue;
            };
            if should_write(strategy, empty, true) {
                apply_field_value(db, user, book_id, field, &val, pkey)?;
                applied.push(json!({"field": field, "provider": pkey}));
                break;
            }
        }
    }
    // BO persists every provider id the run resolved, not only the first candidate.
    for (_, cand) in by_provider {
        if cand.external_id.trim().is_empty() {
            continue;
        }
        set_provider_id(
            db,
            book_id,
            &cand.provider,
            &cand.external_id,
            cand.source_url.as_deref(),
        )?;
    }
    db.execute(
        "UPDATE book_metadata SET last_fetch_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE book_id=?",
        params![book_id],
    )?;
    let score = recompute_score(db, user, book_id)?;
    Ok(json!({"applied": applied, "metadata_score": score, "preserve_existing": preserve_existing}))
}

pub fn backfill_entities(db: &Connection) -> ApiResult<()> {
    // Authors from books.author
    let rows: Vec<(i64, i64, String)> = {
        let mut stmt = db.prepare("SELECT id, user_id, author FROM books")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (book_id, user, author) in rows {
        ensure_book_metadata_row(db, book_id)?;
        let n: i64 = db.query_row(
            "SELECT COUNT(*) FROM book_authors WHERE book_id=?",
            params![book_id],
            |r| r.get(0),
        )?;
        if n == 0 && !author.trim().is_empty() {
            set_book_authors(db, user, book_id, &[author])?;
        }
        let series: Option<String> = db.query_row(
            "SELECT series FROM books WHERE id=?",
            params![book_id],
            |r| r.get(0),
        )?;
        let idx: Option<f64> = db.query_row(
            "SELECT series_index FROM books WHERE id=?",
            params![book_id],
            |r| r.get(0),
        )?;
        let m: i64 = db.query_row(
            "SELECT COUNT(*) FROM book_series_memberships WHERE book_id=?",
            params![book_id],
            |r| r.get(0),
        )?;
        if m == 0 {
            if let Some(s) = series {
                let idx_s = idx.map(|n| n.to_string());
                set_book_series(db, user, book_id, Some(&s), idx_s.as_deref())?;
            }
        }
        // Migrate embedded cover once.
        let cover_n: i64 = db.query_row(
            "SELECT COUNT(*) FROM book_covers WHERE book_id=?",
            params![book_id],
            |r| r.get(0),
        )?;
        if cover_n == 0 {
            let cover: Option<(Vec<u8>, Option<String>)> = db.query_row(
                "SELECT cover, cover_mime FROM epub_files WHERE book_id=? AND cover IS NOT NULL",
                params![book_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ).optional()?;
            if let Some((bytes, mime)) = cover {
                db.execute(
                    "INSERT INTO book_covers(book_id, medium, source, origin, mime, bytes, active)
                     VALUES (?, 'ebook', 'extracted', 'embedded', ?, ?, 1)",
                    params![book_id, mime.unwrap_or_else(|| "image/jpeg".into()), bytes],
                )?;
                db.execute(
                    "UPDATE book_metadata SET cover_source='extracted' WHERE book_id=?",
                    params![book_id],
                )?;
            }
        }
        let _ = recompute_score(db, user, book_id);
    }
    Ok(())
}

pub fn metadata_json_for_book(db: &Connection, book_id: i64) -> ApiResult<Value> {
    ensure_book_metadata_row(db, book_id)?;
    let row = db.query_row(
        "SELECT subtitle, description, isbn10, isbn13, publisher, published_date, published_year,
                language, page_count, rating, metadata_score, locked_fields, last_fetch_at,
                google_books_id, goodreads_id, amazon_id, hardcover_id, open_library_id,
                itunes_id, audible_id, audnexus_id, comicvine_id, cover_source
         FROM book_metadata WHERE book_id=?",
        params![book_id],
        |r| Ok(json!({
            "subtitle": r.get::<_, Option<String>>(0)?,
            "description": r.get::<_, Option<String>>(1)?,
            "isbn10": r.get::<_, Option<String>>(2)?,
            "isbn13": r.get::<_, Option<String>>(3)?,
            "publisher": r.get::<_, Option<String>>(4)?,
            "published_date": r.get::<_, Option<String>>(5)?,
            "published_year": r.get::<_, Option<i64>>(6)?,
            "language": r.get::<_, Option<String>>(7)?,
            "page_count": r.get::<_, Option<i64>>(8)?,
            "rating": r.get::<_, Option<i64>>(9)?,
            "metadata_score": r.get::<_, i64>(10)?,
            "locked_fields": serde_json::from_str::<Value>(&r.get::<_, String>(11)?).unwrap_or(json!([])),
            "last_fetch_at": r.get::<_, Option<String>>(12)?,
            "provider_ids": {
                "google": r.get::<_, Option<String>>(13)?,
                "goodreads": r.get::<_, Option<String>>(14)?,
                "amazon": r.get::<_, Option<String>>(15)?,
                "hardcover": r.get::<_, Option<String>>(16)?,
                "openLibrary": r.get::<_, Option<String>>(17)?,
                "itunes": r.get::<_, Option<String>>(18)?,
                "audible": r.get::<_, Option<String>>(19)?,
                "audnexus": r.get::<_, Option<String>>(20)?,
                "comicvine": r.get::<_, Option<String>>(21)?,
            },
            "cover_source": r.get::<_, Option<String>>(22)?,
        })),
    )?;
    let authors: Vec<String> = {
        let mut stmt = db.prepare(
            "SELECT a.name FROM book_authors ba JOIN authors a ON a.id=ba.author_id
             WHERE ba.book_id=? ORDER BY ba.display_order, a.name",
        )?;
        let rows = stmt
            .query_map(params![book_id], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let genres: Vec<String> = {
        let mut stmt = db.prepare(
            "SELECT g.name FROM book_genres bg JOIN genres g ON g.id=bg.genre_id WHERE bg.book_id=? ORDER BY g.name")?;
        let rows = stmt
            .query_map(params![book_id], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let tags: Vec<String> = {
        let mut stmt = db.prepare(
            "SELECT t.name FROM book_tags bt JOIN tags t ON t.id=bt.tag_id WHERE bt.book_id=? ORDER BY t.name")?;
        let rows = stmt
            .query_map(params![book_id], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let narrators: Vec<String> = {
        let mut stmt = db.prepare(
            "SELECT n.name FROM book_narrators bn JOIN narrators n ON n.id=bn.narrator_id WHERE bn.book_id=? ORDER BY n.name")?;
        let rows = stmt
            .query_map(params![book_id], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let covers: Vec<Value> = {
        let mut stmt = db.prepare(
            "SELECT id, medium, source, origin, provider, mime, active, width, height FROM book_covers WHERE book_id=? ORDER BY id")?;
        let rows = stmt
            .query_map(params![book_id], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "medium": r.get::<_, String>(1)?,
                    "source": r.get::<_, String>(2)?,
                    "origin": r.get::<_, String>(3)?,
                    "provider": r.get::<_, Option<String>>(4)?,
                    "mime": r.get::<_, Option<String>>(5)?,
                    "active": r.get::<_, i64>(6)? != 0,
                    "width": r.get::<_, Option<i64>>(7)?,
                    "height": r.get::<_, Option<i64>>(8)?,
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let (title, author, series_name, series_index): (
        String,
        Option<String>,
        Option<String>,
        Option<f64>,
    ) = db.query_row(
        "SELECT title, author, series, series_index FROM books WHERE id=?",
        params![book_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let mut out = row;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("title".into(), json!(title));
        obj.insert("author".into(), json!(author));
        obj.insert("series_name".into(), json!(series_name));
        obj.insert("series_index".into(), json!(series_index));
        obj.insert("authors".into(), json!(authors));
        obj.insert("genres".into(), json!(genres));
        obj.insert("tags".into(), json!(tags));
        obj.insert("narrators".into(), json!(narrators));
        obj.insert("covers".into(), json!(covers));
    }
    Ok(out)
}

#[cfg(test)]
mod genre_normalize_tests {
    use super::normalize_genres;

    #[test]
    fn normalize_genres_strips_itunes_aisle() {
        assert!(normalize_genres(&["Sci-Fi & Fantasy".into()]).is_empty());
    }

    #[test]
    fn normalize_genres_prefers_google_leaf() {
        assert_eq!(
            normalize_genres(&["Fiction / Fantasy / Epic".into()]),
            vec!["Epic Fantasy".to_string()]
        );
        assert_eq!(
            normalize_genres(&["Fiction / Fantasy".into()]),
            vec!["Fantasy".to_string()]
        );
    }

    #[test]
    fn normalize_genres_maps_fantasy_fiction() {
        assert_eq!(
            normalize_genres(&["Fantasy fiction".into(), "Elves".into()]),
            vec!["Fantasy".to_string()]
        );
    }
}
