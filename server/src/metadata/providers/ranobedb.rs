//! Adapted from Book Orbit (`bookorbit/bookorbit`, AGPL-3.0):
//! https://github.com/bookorbit/bookorbit
//! See `/NOTICE` for full attribution.

//! RanobeDB public API (BO `ranobedb.provider` / `ranobedb.client` / `ranobedb.mapper`).

use super::{http_client, trim_opt, urlencoding_lite, Candidate, SearchQuery};
use crate::{ApiError, ApiResult};
use axum::http::StatusCode;
use serde_json::Value;

const BASE: &str = "https://ranobedb.org/api/v0";
const IMAGE_BASE: &str = "https://images.ranobedb.org";
const UA: &str = "Shelf/0.1 (+https://github.com/rjyates/Shelf)";

fn get_json(path: &str) -> ApiResult<Value> {
    let client = http_client(UA)?;
    let resp = client
        .get(format!("{BASE}{path}"))
        .send()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "RanobeDB request failed."))?;
    if !resp.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "RanobeDB returned an error.",
        ));
    }
    resp.json()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "RanobeDB returned invalid JSON."))
}

pub fn search(q: &SearchQuery) -> ApiResult<Vec<Candidate>> {
    let query = {
        let mut parts = Vec::new();
        if let Some(t) = q.title.as_deref().filter(|s| !s.is_empty()) {
            parts.push(t.to_string());
        }
        if let Some(a) = q.author.as_deref().filter(|s| !s.is_empty()) {
            parts.push(a.to_string());
        }
        if parts.is_empty() {
            if let Some(raw) = q.query.as_deref().filter(|s| !s.is_empty()) {
                parts.push(raw.to_string());
            }
        }
        parts.join(" ")
    };
    if query.trim().is_empty() {
        return Ok(vec![]);
    }
    let body = match get_json(&format!(
        "/books?q={}&limit=5",
        urlencoding_lite(query.trim())
    )) {
        Ok(v) => v,
        Err(_) => return Ok(vec![]),
    };
    let ids: Vec<i64> = body
        .get("books")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|b| b.get("id").and_then(|v| v.as_i64()))
                .collect()
        })
        .unwrap_or_default();
    let mut out = Vec::new();
    for id in ids {
        if let Ok(detail) = get_json(&format!("/book/{id}")) {
            if let Some(book) = detail.get("book") {
                if let Some(c) = map_book(book) {
                    out.push(c);
                }
            }
        }
        if out.len() >= q.limit.min(5) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    Ok(out)
}

fn map_book(book: &Value) -> Option<Candidate> {
    let id = book.get("id")?.as_i64()?;
    let titles = book
        .get("titles")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let title = titles
        .iter()
        .find(|t| {
            t.get("lang").and_then(|v| v.as_str()) == Some("en")
                && t.get("official").and_then(|v| v.as_bool()) == Some(true)
        })
        .and_then(|t| t.get("title").and_then(|v| v.as_str()))
        .or_else(|| book.get("romaji").and_then(|v| v.as_str()))
        .or_else(|| book.get("title").and_then(|v| v.as_str()))?
        .to_string();
    let subtitle = book
        .get("title_orig")
        .and_then(|v| v.as_str())
        .filter(|s| *s != title)
        .map(str::to_string);
    let mut authors = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(editions) = book.get("editions").and_then(|v| v.as_array()) {
        for ed in editions {
            if let Some(staff) = ed.get("staff").and_then(|v| v.as_array()) {
                for s in staff {
                    if s.get("role_type").and_then(|v| v.as_str()) != Some("author") {
                        continue;
                    }
                    let sid = s.get("staff_id").and_then(|v| v.as_i64()).unwrap_or(0);
                    if !seen.insert(sid) {
                        continue;
                    }
                    if let Some(name) = s
                        .get("romaji")
                        .and_then(|v| v.as_str())
                        .or_else(|| s.get("name").and_then(|v| v.as_str()))
                    {
                        authors.push(name.to_string());
                    }
                }
            }
        }
    }
    let releases = book
        .get("releases")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let english = releases
        .iter()
        .filter(|r| {
            r.get("lang").and_then(|v| v.as_str()) == Some("en")
                && r.get("format").and_then(|v| v.as_str()) != Some("audio")
        })
        .collect::<Vec<_>>();
    let selected = english
        .iter()
        .find(|r| r.get("format").and_then(|v| v.as_str()) == Some("print"))
        .or_else(|| english.first())
        .copied();
    let cover = book
        .pointer("/image/filename")
        .and_then(|v| v.as_str())
        .map(|f| format!("{IMAGE_BASE}/{f}"));
    Some(Candidate {
        provider: "ranobedb".into(),
        external_id: id.to_string(),
        title: Some(title),
        subtitle,
        description: trim_opt(
            book.get("description")
                .or_else(|| book.get("description_ja"))
                .and_then(|v| v.as_str()),
        ),
        authors,
        narrators: vec![],
        genres: book
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter(|t| t.get("type").and_then(|v| v.as_str()) == Some("genre"))
                    .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        tags: vec![],
        series_name: book
            .pointer("/series/title")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        series_index: None, // BO resolves from series.books position; skip without full graph
        publisher: book
            .get("publishers")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|p| p.get("name").and_then(|n| n.as_str()))
            .map(str::to_string),
        published_date: None,
        published_year: selected
            .and_then(|r| r.get("release_date").and_then(|v| v.as_i64()))
            .or_else(|| book.get("c_release_date").and_then(|v| v.as_i64()))
            .and_then(|n| {
                // compact YYYYMMDD
                let s = n.to_string();
                if s.len() >= 4 {
                    s[..4].parse().ok()
                } else {
                    None
                }
            }),
        language: if english.is_empty() {
            trim_opt(
                book.get("olang")
                    .or_else(|| book.get("lang"))
                    .and_then(|v| v.as_str()),
            )
        } else {
            Some("en".into())
        },
        page_count: selected.and_then(|r| r.get("pages").and_then(|v| v.as_i64())),
        isbn10: None,
        isbn13: selected
            .and_then(|r| r.get("isbn13").and_then(|v| v.as_str()))
            .map(str::to_string),
        cover_url: cover,
        audio_cover_url: None,
        source_url: Some(format!("https://ranobedb.org/book/{id}")),
        community_rating: book.pointer("/rating/average").and_then(|v| v.as_f64()),
    })
}
