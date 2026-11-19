//! Adapted from Book Orbit (`bookorbit/bookorbit`, AGPL-3.0):
//! https://github.com/bookorbit/bookorbit
//! See `/NOTICE` for full attribution.

//! Libro.fm public explore API (BO `librofm.provider` / `librofm.mapper`).

use super::{http_client, strip_html, trim_opt, urlencoding_lite, Candidate, SearchQuery};
use crate::{ApiError, ApiResult};
use axum::http::StatusCode;
use serde_json::Value;

const API_BASE: &str = "https://libro.fm/api/v12/explore";

fn libro_get(url: &str) -> ApiResult<Value> {
    // BO REQUEST_HEADERS — mobile app UA.
    let client = http_client("okhttp/4.12.0")?;
    let resp = client
        .get(url)
        .header("Accept", "application/json")
        .header("x-librofm-appver", "7.37.4")
        .send()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "Libro.fm request failed."))?;
    if !resp.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "Libro.fm returned an error.",
        ));
    }
    resp.json()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "Libro.fm returned invalid JSON."))
}

pub fn search(q: &SearchQuery) -> ApiResult<Vec<Candidate>> {
    // Direct ISBN / id lookup
    if let Some(id) = q.isbn.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(fetch_by_isbn(id).into_iter().collect());
    }
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
    let search_url = format!(
        "{API_BASE}/search?page=1&q={}&searchby=all",
        urlencoding_lite(query.trim())
    );
    let body = match libro_get(&search_url) {
        Ok(v) => v,
        Err(_) => return Ok(vec![]),
    };
    let isbns: Vec<String> = body
        .pointer("/audiobook_collection/audiobooks")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|b| {
                    b.get("isbn").and_then(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .or_else(|| v.as_i64().map(|n| n.to_string()))
                    })
                })
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for isbn in isbns {
        if !seen.insert(isbn.clone()) {
            continue;
        }
        if let Some(c) = fetch_by_isbn(&isbn) {
            out.push(c);
        }
        if out.len() >= q.limit.min(8) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    Ok(out)
}

fn fetch_by_isbn(isbn: &str) -> Option<Candidate> {
    let url = format!(
        "{API_BASE}/audiobook_details/{}",
        urlencoding_lite(isbn.trim())
    );
    let body = libro_get(&url).ok()?;
    let book = body.pointer("/data/audiobook")?;
    map_audiobook(book)
}

fn map_audiobook(book: &Value) -> Option<Candidate> {
    let provider_id = book
        .get("isbn")
        .and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        })?
        .trim()
        .to_string();
    if provider_id.is_empty() {
        return None;
    }
    let title = trim_opt(book.get("title").and_then(|v| v.as_str()))?;
    let cover = book.get("cover_url").and_then(|v| v.as_str()).map(|u| {
        if u.starts_with("//") {
            format!("https:{u}")
        } else if u.starts_with('/') {
            format!("https://libro.fm{u}")
        } else {
            u.to_string()
        }
    });
    let published = trim_opt(book.get("publication_date").and_then(|v| v.as_str()));
    Some(Candidate {
        provider: "librofm".into(),
        external_id: provider_id.clone(),
        title: Some(title),
        subtitle: trim_opt(book.get("subtitle").and_then(|v| v.as_str())),
        description: trim_opt(book.get("description").and_then(|v| v.as_str()))
            .map(|s| strip_html(&s)),
        authors: book
            .get("authors")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        narrators: book
            .pointer("/audiobook_info/narrators")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        genres: book
            .get("genres")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|g| g.get("name").and_then(|n| n.as_str()).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        tags: vec![],
        series_name: trim_opt(book.get("series").and_then(|v| v.as_str())),
        series_index: book.get("series_num").and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.as_f64().map(|n| n.to_string()))
        }),
        publisher: trim_opt(book.get("publisher").and_then(|v| v.as_str())),
        published_date: published.clone(),
        published_year: published.as_deref().and_then(|s| s.get(0..4)?.parse().ok()),
        language: trim_opt(
            book.pointer("/audiobook_info/audio_language_display")
                .or_else(|| book.pointer("/audiobook_info/audio_language"))
                .and_then(|v| v.as_str()),
        ),
        page_count: None,
        isbn10: None,
        isbn13: if provider_id.len() == 13 {
            Some(provider_id.clone())
        } else {
            None
        },
        cover_url: None,
        audio_cover_url: cover,
        source_url: Some(format!("https://libro.fm/audiobooks/{provider_id}")),
        community_rating: None,
    })
}
