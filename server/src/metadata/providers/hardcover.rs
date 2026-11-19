//! Adapted from Book Orbit (`bookorbit/bookorbit`, AGPL-3.0):
//! https://github.com/bookorbit/bookorbit
//! See `/NOTICE` for full attribution.

//! Hardcover GraphQL search (BO `hardcover.provider` / `hardcover.client` / `hardcover.mapper`).

use super::{http_post_json, trim_opt, Candidate, SearchQuery};
use crate::ApiResult;
use serde_json::{json, Value};

const ENDPOINT: &str = "https://api.hardcover.app/v1/graphql";

const SEARCH_BOOKS_QUERY: &str = r#"
  query BookSearch($q: String!, $limit: Int!) {
    search(query: $q, query_type: "Book", per_page: $limit, page: 1) {
      results
    }
  }
"#;

const SEARCH_BY_ISBN_QUERY: &str = r#"
  query BookSearchByIsbn($isbn: String!) {
    books(where: { editions: { _or: [{ isbn_13: { _eq: $isbn } }, { isbn_10: { _eq: $isbn } }] } }) {
      id
      slug
      title
      subtitle
      description
      pages
      release_date
      release_year
      rating
      ratings_count
      image { url }
      cached_contributors
      featured_book_series { position series { name } }
      editions(where: { _or: [{ isbn_13: { _eq: $isbn } }, { isbn_10: { _eq: $isbn } }] }) {
        isbn_10
        isbn_13
        pages
        release_date
        release_year
        reading_format_id
        audio_seconds
        image { url }
        publisher { name }
      }
    }
  }
"#;

pub fn search(q: &SearchQuery, api_key: Option<&str>) -> ApiResult<Vec<Candidate>> {
    let Some(key) = api_key.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(vec![]);
    };

    if let Some(isbn) = q.isbn.as_deref().filter(|s| !s.is_empty()) {
        let body = match http_post_json(
            ENDPOINT,
            &json!({"query": SEARCH_BY_ISBN_QUERY, "variables": {"isbn": isbn}}),
            Some(key),
        ) {
            Ok(v) => v,
            Err(_) => return Ok(vec![]),
        };
        let books = body
            .pointer("/data/books")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out = Vec::new();
        for book in books {
            if let Some(c) = map_book_json(&book, q.is_audiobook) {
                out.push(c);
            }
        }
        if !out.is_empty() {
            return Ok(out);
        }
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
    let body = match http_post_json(
        ENDPOINT,
        &json!({
            "query": SEARCH_BOOKS_QUERY,
            "variables": {"q": query.trim(), "limit": q.limit.min(20) as i64}
        }),
        Some(key),
    ) {
        Ok(v) => v,
        Err(_) => return Ok(vec![]),
    };
    // BO: data.search.results.hits[].document
    let hits = body
        .pointer("/data/search/results/hits")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(hits
        .into_iter()
        .filter_map(|h| h.get("document").cloned())
        .filter_map(|doc| map_search_document(&doc))
        .take(q.limit.min(20))
        .collect())
}

fn map_search_document(doc: &Value) -> Option<Candidate> {
    let slug = doc.get("slug")?.as_str()?.to_string();
    let title = trim_opt(doc.get("title").and_then(|v| v.as_str()))?;
    let authors = doc
        .get("author_names")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .or_else(|| {
            doc.get("contributions")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| {
                            c.get("author")
                                .and_then(|a| a.get("name"))
                                .and_then(|n| n.as_str())
                                .map(str::to_string)
                                .or_else(|| {
                                    c.get("name").and_then(|n| n.as_str()).map(str::to_string)
                                })
                        })
                        .collect()
                })
        })
        .unwrap_or_default();
    let isbns = doc
        .get("isbns")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let isbn13 = isbns
        .iter()
        .find_map(|i| i.as_str().filter(|s| s.len() == 13))
        .map(str::to_string);
    let isbn10 = isbns
        .iter()
        .find_map(|i| i.as_str().filter(|s| s.len() == 10))
        .map(str::to_string);
    Some(Candidate {
        provider: "hardcover".into(),
        external_id: slug.clone(),
        title: Some(title),
        subtitle: trim_opt(doc.get("subtitle").and_then(|v| v.as_str())),
        description: trim_opt(doc.get("description").and_then(|v| v.as_str())),
        authors,
        narrators: vec![],
        genres: doc
            .get("genres")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        tags: vec![],
        series_name: doc
            .pointer("/featured_series/series/name")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        series_index: doc.pointer("/featured_series/position").and_then(|v| {
            v.as_f64()
                .map(|n| n.to_string())
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        }),
        publisher: None,
        published_date: trim_opt(doc.get("release_date").and_then(|v| v.as_str())),
        published_year: doc.get("release_year").and_then(|v| v.as_i64()),
        language: None,
        page_count: doc.get("pages").and_then(|v| v.as_i64()),
        isbn10,
        isbn13,
        cover_url: doc
            .pointer("/image/url")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        audio_cover_url: None,
        source_url: Some(format!("https://hardcover.app/books/{slug}")),
        community_rating: doc.get("rating").and_then(|v| v.as_f64()),
    })
}

fn edition_is_audio(edition: &Value) -> bool {
    // BO AUDIOBOOK_READING_FORMAT_ID = 2
    edition.get("reading_format_id").and_then(|v| v.as_i64()) == Some(2)
        || edition
            .get("audio_seconds")
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
            > 0
}

fn edition_rank(edition: &Value, want_audio: bool) -> i32 {
    // BO: other medium first (+2), missing pages (+1). Lower sorts first.
    let other = edition_is_audio(edition) != want_audio;
    (if other { 2 } else { 0 })
        + (if edition.get("pages").and_then(|v| v.as_i64()).is_none() {
            1
        } else {
            0
        })
}

fn map_book_json(book: &Value, want_audio: bool) -> Option<Candidate> {
    let slug = book.get("slug")?.as_str()?.to_string();
    let title = trim_opt(book.get("title").and_then(|v| v.as_str()))?;
    let edition = book
        .get("editions")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.iter().min_by_key(|e| edition_rank(e, want_audio)));
    let authors = book
        .get("cached_contributors")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    let role = c
                        .get("contribution")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Author");
                    if !role.eq_ignore_ascii_case("Author")
                        && !role.to_ascii_lowercase().contains("author")
                    {
                        return None;
                    }
                    c.get("author")
                        .and_then(|a| a.get("name"))
                        .and_then(|n| n.as_str())
                        .or_else(|| c.get("name").and_then(|n| n.as_str()))
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Candidate {
        provider: "hardcover".into(),
        external_id: slug.clone(),
        title: Some(title),
        subtitle: trim_opt(book.get("subtitle").and_then(|v| v.as_str())),
        description: trim_opt(book.get("description").and_then(|v| v.as_str())),
        authors,
        narrators: vec![],
        genres: vec![],
        tags: vec![],
        series_name: book
            .pointer("/featured_book_series/series/name")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        series_index: book
            .pointer("/featured_book_series/position")
            .and_then(|v| {
                v.as_f64()
                    .map(|n| n.to_string())
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            }),
        publisher: edition
            .and_then(|e| e.pointer("/publisher/name"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        published_date: trim_opt(
            edition
                .and_then(|e| e.get("release_date"))
                .or_else(|| book.get("release_date"))
                .and_then(|v| v.as_str()),
        ),
        published_year: edition
            .and_then(|e| e.get("release_year"))
            .or_else(|| book.get("release_year"))
            .and_then(|v| v.as_i64()),
        language: None,
        page_count: edition
            .and_then(|e| e.get("pages"))
            .or_else(|| book.get("pages"))
            .and_then(|v| v.as_i64()),
        isbn10: edition
            .and_then(|e| e.get("isbn_10"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        isbn13: edition
            .and_then(|e| e.get("isbn_13"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        cover_url: {
            let url = edition
                .and_then(|e| e.pointer("/image/url"))
                .or_else(|| book.pointer("/image/url"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            if want_audio {
                None
            } else {
                url
            }
        },
        audio_cover_url: {
            let url = edition
                .and_then(|e| e.pointer("/image/url"))
                .or_else(|| book.pointer("/image/url"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            if want_audio {
                url
            } else {
                None
            }
        },
        source_url: Some(format!("https://hardcover.app/books/{slug}")),
        community_rating: book.get("rating").and_then(|v| v.as_f64()),
    })
}
