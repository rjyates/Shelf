//! Metadata providers (Book Orbit–shaped).
//!
//! Portions adapted from [Book Orbit](https://github.com/bookorbit/bookorbit) (AGPL-3.0).
//! See `/NOTICE` for attribution.

mod amazon;
mod comicvine;
mod hardcover;
mod kobo;
mod librofm;
mod ranobedb;

use super::*;

use serde_json::{json, Value};

#[derive(Clone, Debug, Default)]
pub struct Candidate {
    pub provider: String,
    pub external_id: String,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub narrators: Vec<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub series_name: Option<String>,
    pub series_index: Option<String>,
    pub publisher: Option<String>,
    pub published_date: Option<String>,
    pub published_year: Option<i64>,
    pub language: Option<String>,
    pub page_count: Option<i64>,
    pub isbn10: Option<String>,
    pub isbn13: Option<String>,
    pub cover_url: Option<String>,
    pub audio_cover_url: Option<String>,
    pub source_url: Option<String>,
    pub community_rating: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct SearchQuery {
    pub title: Option<String>,
    pub author: Option<String>,
    pub isbn: Option<String>,
    pub query: Option<String>,
    pub series_name: Option<String>,
    pub series_index: Option<String>,
    pub is_audiobook: bool,
    pub limit: usize,
    /// Provider `config_json` (Amazon domain/cookie, Kobo region, …).
    pub config_json: Option<String>,
}

pub(super) fn http_get_json(url: &str) -> ApiResult<Value> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("Shelf/0.1 (metadata; +https://github.com/rjyates/Shelf)")
        .build()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "Could not build HTTP client."))?;
    let resp = client
        .get(url)
        .send()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "Metadata provider request failed."))?;
    if !resp.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "Metadata provider returned an error.",
        ));
    }
    resp.json().map_err(|_| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            "Metadata provider returned invalid JSON.",
        )
    })
}

fn open_library_genres(doc: &serde_json::Value) -> Vec<String> {
    let Some(arr) = doc.get("subject").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut genres = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for sub in arr.iter().filter_map(|a| a.as_str()) {
        let lower = sub.to_lowercase();
        let picked = if lower.contains("fantasy") {
            Some("Fantasy")
        } else if lower.contains("science fiction") || lower == "sci-fi" || lower.contains("sci-fi")
        {
            Some("Science Fiction")
        } else if lower.contains("mystery") {
            Some("Mystery")
        } else if lower.contains("thriller") {
            Some("Thriller")
        } else if lower.contains("horror") {
            Some("Horror")
        } else if lower.contains("romance") {
            Some("Romance")
        } else if lower.contains("historical fiction") {
            Some("Historical Fiction")
        } else if lower.contains("young adult") {
            Some("Young Adult")
        } else if lower.contains("dystopi") {
            Some("Dystopian")
        } else if lower.contains("adventure") && lower.contains("fiction") {
            Some("Adventure")
        } else {
            None
        };
        let Some(label) = picked else {
            continue;
        };
        if seen.insert(label) {
            genres.push(label.to_string());
        }
        if genres.len() >= 4 {
            break;
        }
    }
    genres
}

pub(super) fn trim_opt(s: Option<&str>) -> Option<String> {
    s.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub(super) fn urlencoding_lite(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub(super) fn http_client(ua: &str) -> ApiResult<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(ua)
        .build()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "Could not build HTTP client."))
}

/// Browser-ish UA for HTML/autocomplete scrapers (Goodreads WAF, DDG vqd).
pub(super) const SCRAPER_UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub(super) fn http_get_text(url: &str, referer: Option<&str>) -> ApiResult<String> {
    let client = http_client(SCRAPER_UA)?;
    let mut req = client.get(url).header("Accept-Language", "en-US,en;q=0.9");
    if let Some(r) = referer {
        req = req.header("Referer", r);
    }
    let resp = req
        .send()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "Metadata provider request failed."))?;
    if !resp.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "Metadata provider returned an error.",
        ));
    }
    resp.text().map_err(|_| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            "Metadata provider returned invalid body.",
        )
    })
}

pub(super) fn http_get_json_scraper(
    url: &str,
    referer: Option<&str>,
    extra: &[(&str, &str)],
) -> ApiResult<Value> {
    let client = http_client(SCRAPER_UA)?;
    let mut req = client
        .get(url)
        .header("Accept", "application/json, text/javascript, */*; q=0.01")
        .header("Accept-Language", "en-US,en;q=0.9");
    if let Some(r) = referer {
        req = req.header("Referer", r);
    }
    for (k, v) in extra {
        req = req.header(*k, *v);
    }
    let resp = req
        .send()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "Metadata provider request failed."))?;
    if !resp.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "Metadata provider returned an error.",
        ));
    }
    resp.json().map_err(|_| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            "Metadata provider returned invalid JSON.",
        )
    })
}

pub(super) fn strip_html(s: &str) -> String {
    let re = regex::Regex::new(r"<[^>]+>").unwrap();
    let plain = re.replace_all(s, " ");
    plain
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn http_post_json(url: &str, body: &Value, bearer: Option<&str>) -> ApiResult<Value> {
    let client = http_client("Shelf/0.1 (metadata; +https://github.com/rjyates/Shelf)")?;
    let mut req = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json");
    if let Some(token) = bearer.map(str::trim).filter(|s| !s.is_empty()) {
        let auth = if token.to_ascii_lowercase().starts_with("bearer ") {
            token.to_string()
        } else {
            format!("Bearer {token}")
        };
        req = req.header("Authorization", auth);
    }
    let resp = req
        .json(body)
        .send()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "Metadata provider request failed."))?;
    if !resp.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "Metadata provider returned an error.",
        ));
    }
    resp.json().map_err(|_| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            "Metadata provider returned invalid JSON.",
        )
    })
}

pub(super) fn query_terms(q: &SearchQuery) -> String {
    let mut parts = Vec::new();
    if let Some(t) = q.title.as_deref().filter(|s| !s.is_empty()) {
        parts.push(t.to_string());
    }
    if let Some(a) = q.author.as_deref().filter(|s| !s.is_empty()) {
        parts.push(a.to_string());
    }
    if let Some(raw) = q.query.as_deref().filter(|s| !s.is_empty()) {
        parts.push(raw.to_string());
    }
    if let Some(isbn) = q.isbn.as_deref().filter(|s| !s.is_empty()) {
        parts.push(isbn.to_string());
    }
    parts.join(" ")
}

fn parse_series_suffix(title: &str) -> (Option<String>, Option<String>) {
    // "Book Title (Series Name, #3)" — Goodreads autocomplete shape (BO).
    let re = regex::Regex::new(r"^(?P<title>.+?)\s*\((?P<series>[^,]+),\s*#(?P<idx>[\d.]+)\)\s*$")
        .unwrap();
    if let Some(c) = re.captures(title) {
        return (
            c.name("series").map(|m| m.as_str().trim().to_string()),
            c.name("idx").map(|m| m.as_str().to_string()),
        );
    }
    // "The Return of the King (Lord of the Rings)" — iTunes / storefront shape.
    let paren = regex::Regex::new(r"^(?P<title>.+?)\s*\((?P<series>[^)]+)\)\s*$").unwrap();
    if let Some(c) = paren.captures(title) {
        let series = c.name("series").map(|m| m.as_str().trim()).unwrap_or("");
        if looks_like_series_name(series) {
            return (Some(series.to_string()), None);
        }
    }
    (None, None)
}

fn looks_like_series_name(series: &str) -> bool {
    let s = series.trim();
    if s.len() < 3 || s.len() > 80 {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    const NOISE: &[&str] = &[
        "paperback",
        "hardcover",
        "kindle",
        "ebook",
        "audiobook",
        "unabridged",
        "abridged",
        "edition",
        "illustrated",
        "anniversary",
        "movie",
        "film",
        "tie-in",
        "omnibus",
        "box set",
        "boxed set",
        "mass market",
        "library binding",
    ];
    if NOISE.iter().any(|n| {
        lower == *n || lower.starts_with(&format!("{n} ")) || lower.ends_with(&format!(" {n}"))
    }) {
        return false;
    }
    if s.chars()
        .all(|c| c.is_ascii_digit() || c == '-' || c == ' ')
    {
        return false;
    }
    true
}

/// Fill series from the title when a provider left series fields empty.
fn enrich_series_from_title(mut c: Candidate) -> Candidate {
    if c.series_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some()
    {
        return c;
    }
    let Some(title) = c.title.as_deref() else {
        return c;
    };
    let (series, idx) = parse_series_suffix(title);
    if let Some(series) = series {
        c.series_name = Some(series);
        if c.series_index.is_none() {
            c.series_index = idx;
        }
    }
    c
}

fn upgrade_goodreads_cover(url: &str) -> String {
    // Autocomplete thumbs carry `._SY75_`; strip for full-res (BO).
    regex::Regex::new(r"\._S[XY]\d+_\.")
        .unwrap()
        .replace(url, ".")
        .into_owned()
}

pub fn candidate_json(c: &Candidate) -> Value {
    json!({
        "provider": c.provider,
        "external_id": c.external_id,
        "title": c.title,
        "subtitle": c.subtitle,
        "description": c.description,
        "authors": c.authors,
        "narrators": c.narrators,
        "genres": c.genres,
        "tags": c.tags,
        "series_name": c.series_name,
        "series_index": c.series_index,
        "publisher": c.publisher,
        "published_date": c.published_date,
        "published_year": c.published_year,
        "language": c.language,
        "page_count": c.page_count,
        "isbn10": c.isbn10,
        "isbn13": c.isbn13,
        "cover_url": c.cover_url,
        "audio_cover_url": c.audio_cover_url,
        "source_url": c.source_url,
        "community_rating": c.community_rating,
    })
}

pub fn builtin_defs() -> Vec<(&'static str, &'static str, bool)> {
    vec![
        ("openLibrary", "Open Library", true),
        ("google", "Google Books", true),
        ("itunes", "iTunes", true),
        ("audnexus", "AudNexus", true),
        ("goodreads", "Goodreads", true), // BO default; autocomplete JSON (detail HTML is WAF-fragile)
        ("amazon", "Amazon", true),       // BO default; HTML scrape (fragile/WAF)
        ("hardcover", "Hardcover", false),
        ("audible", "Audible", false), // BO default off; public catalog API
        ("comicvine", "ComicVine", false),
        ("kobo", "Kobo", false),
        ("librofm", "Libro.fm", false),
        ("ranobedb", "RanobeDB", false),
    ]
}

fn search_open_library(q: &SearchQuery) -> ApiResult<Vec<Candidate>> {
    let mut params = vec![("limit", q.limit.min(20).to_string())];
    if let Some(isbn) = q.isbn.as_deref().filter(|s| !s.is_empty()) {
        params.push(("isbn", isbn.to_string()));
    } else {
        let mut parts = Vec::new();
        if let Some(t) = q.title.as_deref().filter(|s| !s.is_empty()) {
            parts.push(t.to_string());
        }
        if let Some(a) = q.author.as_deref().filter(|s| !s.is_empty()) {
            parts.push(a.to_string());
        }
        if let Some(raw) = q.query.as_deref().filter(|s| !s.is_empty()) {
            parts.push(raw.to_string());
        }
        let joined = parts.join(" ");
        if joined.is_empty() {
            return Ok(vec![]);
        }
        params.push(("q", joined));
    }
    let qs: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding_lite(v)))
        .collect::<Vec<_>>()
        .join("&");
    let body = http_get_json(&format!("https://openlibrary.org/search.json?{qs}"))?;
    let docs = body
        .get("docs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(docs
        .into_iter()
        .take(q.limit.min(20))
        .filter_map(|doc| {
            let key = doc.get("key")?.as_str()?.to_string();
            let authors = doc
                .get("author_name")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|a| a.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let cover_id = doc.get("cover_i").and_then(|v| v.as_i64());
            Some(Candidate {
                provider: "openLibrary".into(),
                external_id: key.clone(),
                title: trim_opt(doc.get("title").and_then(|v| v.as_str())),
                subtitle: trim_opt(doc.get("subtitle").and_then(|v| v.as_str())),
                description: None,
                authors,
                narrators: vec![],
                genres: open_library_genres(&doc),
                tags: vec![],
                series_name: None,
                series_index: None,
                publisher: doc
                    .get("publisher")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                published_date: None,
                published_year: doc.get("first_publish_year").and_then(|v| v.as_i64()),
                language: {
                    let codes: Vec<String> = doc
                        .get("language")
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default();
                    super::rank::prefer_language_code(&codes)
                },
                page_count: doc.get("number_of_pages_median").and_then(|v| v.as_i64()),
                isbn10: doc
                    .get("isbn")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.iter().find_map(|i| i.as_str().filter(|s| s.len() == 10)))
                    .map(str::to_string),
                isbn13: doc
                    .get("isbn")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.iter().find_map(|i| i.as_str().filter(|s| s.len() == 13)))
                    .map(str::to_string),
                cover_url: cover_id
                    .map(|id| format!("https://covers.openlibrary.org/b/id/{id}-L.jpg")),
                audio_cover_url: None,
                source_url: Some(format!("https://openlibrary.org{key}")),
                community_rating: None,
            })
        })
        .collect())
}

fn search_google(q: &SearchQuery, api_key: Option<&str>) -> ApiResult<Vec<Candidate>> {
    let mut terms = Vec::new();
    if let Some(isbn) = q.isbn.as_deref().filter(|s| !s.is_empty()) {
        terms.push(format!("isbn:{isbn}"));
    } else {
        if let Some(t) = q.title.as_deref().filter(|s| !s.is_empty()) {
            terms.push(format!("intitle:{t}"));
        }
        if let Some(a) = q.author.as_deref().filter(|s| !s.is_empty()) {
            terms.push(format!("inauthor:{a}"));
        }
        if let Some(raw) = q.query.as_deref().filter(|s| !s.is_empty()) {
            terms.push(raw.to_string());
        }
    }
    if terms.is_empty() {
        return Ok(vec![]);
    }
    let mut url = format!(
        "https://www.googleapis.com/books/v1/volumes?q={}&maxResults={}",
        urlencoding_lite(&terms.join("+")),
        q.limit.min(20)
    );
    if let Some(key) = api_key.filter(|s| !s.is_empty()) {
        url.push_str("&key=");
        url.push_str(&urlencoding_lite(key));
    }
    let body = http_get_json(&url)?;
    let items = body
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(items
        .into_iter()
        .take(q.limit.min(20))
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.to_string();
            let info = item.get("volumeInfo")?;
            let authors = info
                .get("authors")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|a| a.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let isbns = info
                .get("industryIdentifiers")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let isbn10 = isbns.iter().find_map(|i| {
                (i.get("type")?.as_str()? == "ISBN_10")
                    .then(|| i.get("identifier")?.as_str().map(str::to_string))
                    .flatten()
            });
            let isbn13 = isbns.iter().find_map(|i| {
                (i.get("type")?.as_str()? == "ISBN_13")
                    .then(|| i.get("identifier")?.as_str().map(str::to_string))
                    .flatten()
            });
            let cover = info
                .get("imageLinks")
                .and_then(|v| v.get("thumbnail").or_else(|| v.get("smallThumbnail")))
                .and_then(|v| v.as_str())
                .map(|u| u.replace("http://", "https://"));
            Some(Candidate {
                provider: "google".into(),
                external_id: id.clone(),
                title: trim_opt(info.get("title").and_then(|v| v.as_str())),
                subtitle: trim_opt(info.get("subtitle").and_then(|v| v.as_str())),
                description: trim_opt(info.get("description").and_then(|v| v.as_str())),
                authors,
                narrators: vec![],
                genres: info
                    .get("categories")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|a| a.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                tags: vec![],
                series_name: None,
                series_index: None,
                publisher: trim_opt(info.get("publisher").and_then(|v| v.as_str())),
                published_date: trim_opt(info.get("publishedDate").and_then(|v| v.as_str())),
                published_year: info
                    .get("publishedDate")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.get(0..4)?.parse().ok()),
                language: trim_opt(info.get("language").and_then(|v| v.as_str())),
                page_count: info.get("pageCount").and_then(|v| v.as_i64()),
                isbn10,
                isbn13,
                cover_url: cover,
                audio_cover_url: None,
                source_url: Some(format!("https://books.google.com/books?id={id}")),
                community_rating: info.get("averageRating").and_then(|v| v.as_f64()),
            })
        })
        .collect())
}

#[allow(clippy::unnecessary_map_on_constructor)]
fn search_itunes(q: &SearchQuery) -> ApiResult<Vec<Candidate>> {
    let term = q
        .query
        .clone()
        .or_else(|| {
            let mut parts = Vec::new();
            if let Some(t) = &q.title {
                parts.push(t.clone());
            }
            if let Some(a) = &q.author {
                parts.push(a.clone());
            }
            (!parts.is_empty()).then_some(parts.join(" "))
        })
        .unwrap_or_default();
    if term.trim().is_empty() {
        return Ok(vec![]);
    }
    let url = format!(
        "https://itunes.apple.com/search?term={}&entity=ebook,audiobook&limit={}",
        urlencoding_lite(&term),
        q.limit.min(20)
    );
    let body = http_get_json(&url)?;
    let results = body
        .get("results")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(results
        .into_iter()
        .take(q.limit.min(20))
        .filter_map(|item| {
            let id = item
                .get("trackId")
                .or_else(|| item.get("collectionId"))?
                .as_i64()?;
            let kind = item
                .get("wrapperType")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let cover = item
                .get("artworkUrl100")
                .or_else(|| item.get("artworkUrl60"))
                .and_then(|v| v.as_str())
                .map(|u| {
                    u.replace("100x100bb", "600x600bb")
                        .replace("60x60bb", "600x600bb")
                });
            let is_audio = kind == "audiobook";
            Some(Candidate {
                provider: "itunes".into(),
                external_id: id.to_string(),
                title: trim_opt(
                    item.get("trackName")
                        .or_else(|| item.get("collectionName"))
                        .and_then(|v| v.as_str()),
                ),
                subtitle: None,
                description: trim_opt(item.get("description").and_then(|v| v.as_str())),
                authors: trim_opt(item.get("artistName").and_then(|v| v.as_str()))
                    .into_iter()
                    .collect(),
                narrators: vec![],
                genres: trim_opt(item.get("primaryGenreName").and_then(|v| v.as_str()))
                    .into_iter()
                    .collect(),
                tags: vec![],
                series_name: None,
                series_index: None,
                publisher: None,
                published_date: trim_opt(item.get("releaseDate").and_then(|v| v.as_str())),
                published_year: item
                    .get("releaseDate")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.get(0..4)?.parse().ok()),
                language: None,
                page_count: None,
                isbn10: None,
                isbn13: None,
                cover_url: if is_audio { None } else { cover.clone() },
                audio_cover_url: if is_audio { cover } else { None },
                source_url: trim_opt(item.get("trackViewUrl").and_then(|v| v.as_str())),
                community_rating: None,
            })
            .map(enrich_series_from_title)
        })
        .collect())
}

fn search_audnexus(q: &SearchQuery) -> ApiResult<Vec<Candidate>> {
    let title = q
        .title
        .clone()
        .or_else(|| q.query.clone())
        .unwrap_or_default();
    if title.trim().is_empty() {
        return Ok(vec![]);
    }
    let mut url = format!(
        "https://api.audnex.us/books?title={}",
        urlencoding_lite(title.trim())
    );
    if let Some(a) = q.author.as_deref().filter(|s| !s.is_empty()) {
        url.push_str("&author=");
        url.push_str(&urlencoding_lite(a));
    }
    let body = match http_get_json(&url) {
        Ok(v) => v,
        Err(_) => return Ok(vec![]),
    };
    let items = if let Some(arr) = body.as_array() {
        arr.clone()
    } else if body.is_object() {
        vec![body]
    } else {
        vec![]
    };
    Ok(items
        .into_iter()
        .take(q.limit.min(20))
        .filter_map(|item| {
            let asin = item.get("asin")?.as_str()?.to_string();
            let authors = item
                .get("authors")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|a| {
                            a.get("name")
                                .and_then(|n| n.as_str())
                                .or_else(|| a.as_str())
                                .map(str::to_string)
                        })
                        .collect()
                })
                .unwrap_or_default();
            let narrators = item
                .get("narrators")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|a| {
                            a.get("name")
                                .and_then(|n| n.as_str())
                                .or_else(|| a.as_str())
                                .map(str::to_string)
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(Candidate {
                provider: "audnexus".into(),
                external_id: asin.clone(),
                title: trim_opt(item.get("title").and_then(|v| v.as_str())),
                subtitle: trim_opt(item.get("subtitle").and_then(|v| v.as_str())),
                description: trim_opt(
                    item.get("description")
                        .or_else(|| item.get("summary"))
                        .and_then(|v| v.as_str()),
                ),
                authors,
                narrators,
                genres: item
                    .get("genres")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|g| {
                                g.get("name")
                                    .and_then(|n| n.as_str())
                                    .or_else(|| g.as_str())
                                    .map(str::to_string)
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                tags: vec![],
                series_name: item
                    .get("seriesPrimary")
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                series_index: item
                    .get("seriesPrimary")
                    .and_then(|v| v.get("position"))
                    .and_then(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .or_else(|| v.as_f64().map(|n| n.to_string()))
                    }),
                publisher: trim_opt(item.get("publisherName").and_then(|v| v.as_str())),
                published_date: trim_opt(item.get("releaseDate").and_then(|v| v.as_str())),
                published_year: item
                    .get("releaseDate")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.get(0..4)?.parse().ok()),
                language: trim_opt(item.get("language").and_then(|v| v.as_str())),
                page_count: None,
                isbn10: None,
                isbn13: None,
                cover_url: None,
                audio_cover_url: trim_opt(item.get("image").and_then(|v| v.as_str())),
                source_url: Some(format!("https://www.audible.com/pd/{asin}")),
                community_rating: item.get("rating").and_then(|v| v.as_f64()),
            })
        })
        .collect())
}

/// Goodreads autocomplete JSON — BO's reliable path (detail HTML is often WAF-gated).
/// Ported from bookorbit goodreads.provider / goodreads.mapper (AGPL).
#[allow(clippy::unnecessary_map_on_constructor)]
fn search_goodreads(q: &SearchQuery) -> ApiResult<Vec<Candidate>> {
    let terms = query_terms(q);
    if terms.trim().is_empty() {
        return Ok(vec![]);
    }
    let url = format!(
        "https://www.goodreads.com/book/auto_complete?format=json&q={}",
        urlencoding_lite(terms.trim())
    );
    let body = match http_get_json_scraper(&url, Some("https://www.goodreads.com/"), &[]) {
        Ok(v) => v,
        Err(_) => return Ok(vec![]),
    };
    let items = body.as_array().cloned().unwrap_or_default();
    Ok(items
        .into_iter()
        .take(q.limit.min(20))
        .filter_map(|item| {
            let id = item.get("bookId").and_then(|v| {
                v.as_i64()
                    .map(|n| n.to_string())
                    .or_else(|| v.as_str().map(str::to_string))
            })?;
            let bare = trim_opt(item.get("bookTitleBare").and_then(|v| v.as_str()));
            let full = trim_opt(item.get("title").and_then(|v| v.as_str()));
            let title = bare.clone().or_else(|| {
                full.as_ref().map(|t| {
                    regex::Regex::new(r"\s*\([^,(]+,\s*#[\d.]+\)\s*$")
                        .unwrap()
                        .replace(t, "")
                        .trim()
                        .to_string()
                })
            })?;
            let (series_name, series_index) = full
                .as_deref()
                .map(parse_series_suffix)
                .unwrap_or((None, None));
            let author = match item.get("author") {
                Some(Value::String(s)) => trim_opt(Some(s.as_str())),
                Some(Value::Object(o)) => trim_opt(o.get("name").and_then(|v| v.as_str())),
                _ => None,
            };
            let desc = item
                .get("description")
                .and_then(|d| {
                    d.as_str()
                        .map(str::to_string)
                        .or_else(|| d.get("html").and_then(|h| h.as_str()).map(strip_html))
                })
                .filter(|s| !s.is_empty());
            let cover = trim_opt(item.get("imageUrl").and_then(|v| v.as_str()))
                .map(|u| upgrade_goodreads_cover(&u));
            let rating = item.get("avgRating").and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            });
            Some(Candidate {
                provider: "goodreads".into(),
                external_id: id.clone(),
                title: Some(title),
                subtitle: None,
                description: desc,
                authors: author.into_iter().collect(),
                narrators: vec![],
                genres: vec![],
                tags: vec![],
                series_name,
                series_index,
                publisher: None,
                published_date: None,
                published_year: None,
                language: None,
                page_count: item.get("numPages").and_then(|v| {
                    v.as_i64()
                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                }),
                isbn10: None,
                isbn13: None,
                cover_url: cover,
                audio_cover_url: None,
                source_url: Some(format!("https://www.goodreads.com/book/show/{id}")),
                community_rating: rating,
            })
            .map(enrich_series_from_title)
        })
        .collect())
}

/// Audible public catalog API — BO audible.provider (AGPL). Default off.
fn search_audible(q: &SearchQuery) -> ApiResult<Vec<Candidate>> {
    let terms = {
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
    if terms.trim().is_empty() {
        return Ok(vec![]);
    }
    const GROUPS: &str =
        "product_desc,product_extended_attrs,media,product_attrs,series,product_plan_details,category_ladders,rating";
    let url = format!(
        "https://api.audible.com/1.0/catalog/products?num_results={}&keywords={}&response_groups={}&image_sizes=500,1024",
        q.limit.min(10),
        urlencoding_lite(terms.trim()),
        urlencoding_lite(GROUPS),
    );
    let body = match http_get_json(&url) {
        Ok(v) => v,
        Err(_) => return Ok(vec![]),
    };
    let products = body
        .get("products")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(products
        .into_iter()
        .take(q.limit.min(10))
        .filter_map(|p| {
            let asin = p.get("asin")?.as_str()?.to_string();
            let authors = p
                .get("authors")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|a| a.get("name").and_then(|n| n.as_str()).map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let narrators = p
                .get("narrators")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|a| a.get("name").and_then(|n| n.as_str()).map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let images = p.get("product_images");
            let cover = images
                .and_then(|v| v.get("1024").or_else(|| v.get("500")))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let raw_desc = p
                .get("publisher_summary")
                .or_else(|| p.get("merchandising_summary"))
                .and_then(|v| v.as_str())
                .map(strip_html)
                .filter(|s| !s.is_empty());
            let series = p
                .get("series")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first());
            let series_name = series
                .and_then(|s| s.get("title"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let series_index = series.and_then(|s| s.get("sequence")).and_then(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.as_f64().map(|n| n.to_string()))
            });
            let genres = p
                .get("category_ladders")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|ladder| {
                            let leaf = ladder.get("ladder")?.as_array()?.last()?;
                            let name = leaf.get("name")?.as_str()?.trim();
                            let lower = name.to_lowercase();
                            if matches!(
                                lower.as_str(),
                                "audible books & originals"
                                    | "books"
                                    | "kindle books"
                                    | "audible originals"
                            ) {
                                return None;
                            }
                            Some(name.to_string())
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let rating = p
                .get("rating")
                .and_then(|r| r.get("overall_distribution"))
                .and_then(|d| d.get("average_rating"))
                .and_then(|v| v.as_f64());
            Some(Candidate {
                provider: "audible".into(),
                external_id: asin.clone(),
                title: trim_opt(p.get("title").and_then(|v| v.as_str())),
                subtitle: trim_opt(p.get("subtitle").and_then(|v| v.as_str())),
                description: raw_desc,
                authors,
                narrators,
                genres,
                tags: vec![],
                series_name,
                series_index,
                publisher: trim_opt(p.get("publisher_name").and_then(|v| v.as_str())),
                published_date: trim_opt(p.get("release_date").and_then(|v| v.as_str())),
                published_year: p
                    .get("release_date")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.get(0..4)?.parse().ok()),
                language: trim_opt(p.get("language").and_then(|v| v.as_str())),
                page_count: None,
                isbn10: None,
                isbn13: None,
                cover_url: None,
                audio_cover_url: cover,
                source_url: Some(format!("https://www.audible.com/pd/{asin}")),
                community_rating: rating,
            })
        })
        .collect())
}

#[derive(Clone, Debug)]
pub struct CoverHit {
    pub url: String,
    pub preview_url: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub source: String,
}

pub fn cover_hit_json(c: &CoverHit) -> Value {
    json!({
        "url": c.url,
        "preview_url": c.preview_url,
        "width": c.width,
        "height": c.height,
        "source": c.source,
    })
}

fn cover_host_label(url: &str) -> String {
    let Ok(u) = reqwest::Url::parse(url) else {
        return "Web".into();
    };
    let host = u.host_str().unwrap_or("").trim_start_matches("www.");
    if host == "amazon.com" || host.ends_with(".amazon.com") {
        return "Amazon".into();
    }
    if host == "goodreads.com" || host.ends_with(".goodreads.com") {
        return "Goodreads".into();
    }
    if host.is_empty() {
        "Web".into()
    } else {
        host.into()
    }
}

/// Cover-only helpers (BO cover providers): iTunes + Open Library + DDG + AudiobookCovers.
pub fn search_cover_hits(
    title: &str,
    author: Option<&str>,
    is_audiobook: bool,
) -> ApiResult<Vec<CoverHit>> {
    let title = title.trim();
    if title.is_empty() {
        return Ok(vec![]);
    }
    let mut combined = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let push = |hit: CoverHit,
                combined: &mut Vec<CoverHit>,
                seen: &mut std::collections::HashSet<String>| {
        if seen.insert(hit.url.clone()) {
            combined.push(hit);
        }
    };
    for hit in search_covers_itunes(title, author, is_audiobook).unwrap_or_default() {
        push(hit, &mut combined, &mut seen);
        if combined.len() >= 25 {
            return Ok(combined);
        }
    }
    if !is_audiobook {
        for hit in search_covers_open_library(title, author).unwrap_or_default() {
            push(hit, &mut combined, &mut seen);
            if combined.len() >= 25 {
                return Ok(combined);
            }
        }
    } else {
        for hit in search_covers_audiobookcovers(title, author).unwrap_or_default() {
            push(hit, &mut combined, &mut seen);
            if combined.len() >= 25 {
                return Ok(combined);
            }
        }
    }
    // DDG is often challenge-gated now; soft-fail keeps the gallery useful.
    for hit in search_covers_duckduckgo(title, author, is_audiobook).unwrap_or_default() {
        push(hit, &mut combined, &mut seen);
        if combined.len() >= 25 {
            break;
        }
    }
    Ok(combined)
}

/// iTunes artwork — BO itunes-cover-provider (AGPL).
fn search_covers_itunes(
    title: &str,
    author: Option<&str>,
    is_audiobook: bool,
) -> ApiResult<Vec<CoverHit>> {
    let query = match author.map(str::trim).filter(|s| !s.is_empty()) {
        Some(a) => format!("{title} {a}"),
        None => title.to_string(),
    };
    let entity = if is_audiobook { "audiobook" } else { "ebook" };
    let url = format!(
        "https://itunes.apple.com/search?term={}&entity={entity}&limit=25",
        urlencoding_lite(&query)
    );
    let body = match http_get_json(&url) {
        Ok(v) => v,
        Err(_) => return Ok(vec![]),
    };
    let results = body
        .get("results")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(results
        .into_iter()
        .filter_map(|r| {
            let art = r.get("artworkUrl100")?.as_str()?;
            let url = art
                .replace("100x100bb", "600x600bb")
                .replace("60x60bb", "600x600bb");
            let preview = art
                .replace("100x100bb", "300x300bb")
                .replace("60x60bb", "300x300bb");
            Some(CoverHit {
                url,
                preview_url: Some(preview),
                width: Some(600),
                height: Some(600),
                source: "iTunes".into(),
            })
        })
        .collect())
}

fn search_covers_open_library(title: &str, author: Option<&str>) -> ApiResult<Vec<CoverHit>> {
    let mut parts = vec![title.to_string()];
    if let Some(a) = author.map(str::trim).filter(|s| !s.is_empty()) {
        parts.push(a.to_string());
    }
    let url = format!(
        "https://openlibrary.org/search.json?q={}&limit=20",
        urlencoding_lite(&parts.join(" "))
    );
    let body = match http_get_json(&url) {
        Ok(v) => v,
        Err(_) => return Ok(vec![]),
    };
    let docs = body
        .get("docs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(docs
        .into_iter()
        .filter_map(|doc| {
            let id = doc.get("cover_i")?.as_i64()?;
            let url = format!("https://covers.openlibrary.org/b/id/{id}-L.jpg");
            Some(CoverHit {
                preview_url: Some(format!("https://covers.openlibrary.org/b/id/{id}-M.jpg")),
                width: None,
                height: None,
                source: "Open Library".into(),
                url,
            })
        })
        .collect())
}

/// AudiobookCovers.com HTML scrape — BO audiobookcovers-cover-provider (AGPL).
fn search_covers_audiobookcovers(title: &str, author: Option<&str>) -> ApiResult<Vec<CoverHit>> {
    let mut url = format!(
        "https://www.audiobookcovers.com/search?title={}",
        urlencoding_lite(title)
    );
    if let Some(a) = author.map(str::trim).filter(|s| !s.is_empty()) {
        url.push_str("&author=");
        url.push_str(&urlencoding_lite(a));
    }
    let html = match http_get_text(&url, None) {
        Ok(h) => h,
        Err(_) => return Ok(vec![]),
    };
    let re = regex::Regex::new(
        r#"id:"([0-9a-fA-F-]{36})".*?jpeg:$R\[\d+\]=\{320:"([^"]+)",640:"([^"]+)",1280:"([^"]+)"\}"#,
    )
    .unwrap();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for cap in re.captures_iter(&html) {
        let id = cap.get(1).unwrap().as_str();
        if !seen.insert(id.to_string()) {
            continue;
        }
        out.push(CoverHit {
            preview_url: Some(cap.get(2).unwrap().as_str().to_string()),
            url: cap.get(4).unwrap().as_str().to_string(),
            width: Some(1280),
            height: Some(1280),
            source: "AudiobookCovers".into(),
        });
        if out.len() >= 25 {
            break;
        }
    }
    Ok(out)
}

/// DuckDuckGo image search — BO duckduckgo-cover-provider (AGPL). Often challenge-gated.
fn search_covers_duckduckgo(
    title: &str,
    author: Option<&str>,
    is_audiobook: bool,
) -> ApiResult<Vec<CoverHit>> {
    let book_type = if is_audiobook { "audiobook" } else { "book" };
    let search_term = match author.map(str::trim).filter(|s| !s.is_empty()) {
        Some(a) => format!("{title} {a} {book_type}"),
        None => format!("{title} {book_type}"),
    };
    let layout = if is_audiobook {
        "&iar=images&iaf=size%3ALarge%2Clayout%3ASquare"
    } else {
        "&iar=images&iaf=size%3ALarge%2Clayout%3ATall"
    };

    let mut combined = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for query in [
        format!("{search_term} (site:amazon.com OR site:goodreads.com)"),
        search_term,
    ] {
        for hit in ddg_image_search(&query, layout).unwrap_or_default() {
            if !cover_aspect_ok(&hit, is_audiobook) {
                continue;
            }
            if seen.insert(hit.url.clone()) {
                combined.push(hit);
                if combined.len() >= 25 {
                    return Ok(combined);
                }
            }
        }
    }
    Ok(combined)
}

fn cover_aspect_ok(hit: &CoverHit, is_audiobook: bool) -> bool {
    let (Some(w), Some(h)) = (hit.width, hit.height) else {
        return true;
    };
    if w < 350 {
        return false;
    }
    if is_audiobook {
        let ratio = w as f64 / h as f64;
        (0.85..=1.15).contains(&ratio)
    } else {
        w < h
    }
}

fn ddg_image_search(query: &str, layout: &str) -> ApiResult<Vec<CoverHit>> {
    let search_url = format!(
        "https://duckduckgo.com/?q={}{layout}",
        urlencoding_lite(query)
    );
    let html = http_get_text(&search_url, None)?;
    let vqd = regex::Regex::new(r#"vqd="(\d+-\d+)""#)
        .unwrap()
        .captures(&html)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        .ok_or(ApiError(
            StatusCode::BAD_GATEWAY,
            "DuckDuckGo cover search token missing.",
        ))?;
    let api_url = format!(
        "https://duckduckgo.com/i.js?o=json&q={}{layout}&vqd={vqd}",
        urlencoding_lite(query)
    );
    let body = http_get_json_scraper(&api_url, Some(&search_url), &[("x-vqd-4", &vqd)])?;
    let results = body
        .get("results")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(results
        .into_iter()
        .filter_map(|r| {
            let url = trim_opt(r.get("image").and_then(|v| v.as_str()))?;
            Some(CoverHit {
                preview_url: trim_opt(r.get("thumbnail").and_then(|v| v.as_str())),
                width: r.get("width").and_then(|v| v.as_i64()),
                height: r.get("height").and_then(|v| v.as_i64()),
                source: cover_host_label(&url),
                url,
            })
        })
        .collect())
}

pub fn search_builtin(
    key: &str,
    q: &SearchQuery,
    api_key: Option<&str>,
) -> ApiResult<Vec<Candidate>> {
    match key {
        "openLibrary" => search_open_library(q),
        "google" => search_google(q, api_key),
        "itunes" => search_itunes(q),
        "audnexus" => search_audnexus(q),
        "goodreads" => search_goodreads(q),
        "audible" => search_audible(q),
        "amazon" => amazon::search(q),
        "hardcover" => hardcover::search(q, api_key),
        "comicvine" => comicvine::search(q, api_key),
        "kobo" => kobo::search(q),
        "librofm" => librofm::search(q),
        "ranobedb" => ranobedb::search(q),
        _ => Ok(vec![]),
    }
}

pub fn search_external(base_url: &str, key: &str, q: &SearchQuery) -> ApiResult<Vec<Candidate>> {
    let base = base_url.trim_end_matches('/');
    let mut url = format!("{base}/search?limit={}", q.limit.min(20));
    if let Some(t) = q.title.as_deref().filter(|s| !s.is_empty()) {
        url.push_str("&title=");
        url.push_str(&urlencoding_lite(t));
    }
    if let Some(a) = q.author.as_deref().filter(|s| !s.is_empty()) {
        url.push_str("&author=");
        url.push_str(&urlencoding_lite(a));
    }
    if let Some(i) = q.isbn.as_deref().filter(|s| !s.is_empty()) {
        url.push_str("&isbn=");
        url.push_str(&urlencoding_lite(i));
    }
    if let Some(raw) = q.query.as_deref().filter(|s| !s.is_empty()) {
        url.push_str("&query=");
        url.push_str(&urlencoding_lite(raw));
    }
    let body = http_get_json(&url)?;
    let matches = body
        .get("matches")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(matches
        .into_iter()
        .take(q.limit.min(20))
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_string();
            Some(Candidate {
                provider: key.to_string(),
                external_id: id,
                title: trim_opt(m.get("title").and_then(|v| v.as_str())),
                subtitle: trim_opt(m.get("subtitle").and_then(|v| v.as_str())),
                description: trim_opt(m.get("description").and_then(|v| v.as_str())),
                authors: m
                    .get("authors")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|a| a.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                narrators: vec![],
                genres: m
                    .get("genres")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|a| a.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                tags: vec![],
                series_name: trim_opt(m.get("seriesName").and_then(|v| v.as_str())),
                series_index: m.get("seriesIndex").and_then(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .or_else(|| v.as_f64().map(|n| n.to_string()))
                }),
                publisher: trim_opt(m.get("publisher").and_then(|v| v.as_str())),
                published_date: trim_opt(m.get("publishedDate").and_then(|v| v.as_str())),
                published_year: m.get("publishedYear").and_then(|v| v.as_i64()),
                language: trim_opt(m.get("language").and_then(|v| v.as_str())),
                page_count: m.get("pageCount").and_then(|v| v.as_i64()),
                isbn10: trim_opt(m.get("isbn10").and_then(|v| v.as_str())),
                isbn13: trim_opt(m.get("isbn13").and_then(|v| v.as_str())),
                cover_url: trim_opt(m.get("coverUrl").and_then(|v| v.as_str())),
                audio_cover_url: trim_opt(m.get("audioCoverUrl").and_then(|v| v.as_str())),
                source_url: trim_opt(m.get("sourceUrl").and_then(|v| v.as_str())),
                community_rating: m.get("rating").and_then(|v| v.as_f64()),
            })
        })
        .collect())
}

pub fn download_cover_bytes(url: &str) -> ApiResult<(Vec<u8>, String)> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Shelf/0.1 (metadata; +https://github.com/rjyates/Shelf)")
        .build()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "Could not build HTTP client."))?;
    let resp = client
        .get(url)
        .send()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "Cover download failed."))?;
    if !resp.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "Cover download returned an error.",
        ));
    }
    let mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .split(';')
        .next()
        .unwrap_or("image/jpeg")
        .to_string();
    let bytes = resp
        .bytes()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "Cover download failed."))?
        .to_vec();
    if bytes.is_empty() || bytes.len() > 15_000_000 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Cover image is empty or too large.",
        ));
    }
    Ok((bytes, mime))
}
