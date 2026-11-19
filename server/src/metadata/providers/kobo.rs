//! Adapted from Book Orbit (`bookorbit/bookorbit`, AGPL-3.0):
//! https://github.com/bookorbit/bookorbit
//! See `/NOTICE` for full attribution.

//! Kobo storefront scrape (BO `kobo.provider` / `kobo.scraper`).
//! shortcut: plain HTTP (Cloudflare may block); soft-fails on challenge pages.

use super::{http_get_text, trim_opt, urlencoding_lite, Candidate, SearchQuery};
use crate::ApiResult;
use regex::Regex;

const KOBO_BASE: &str = "https://www.kobo.com";

pub fn search(q: &SearchQuery) -> ApiResult<Vec<Candidate>> {
    let (country, language) = kobo_region(q);
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
    // BO buildKoboSearchUrl
    let url = format!(
        "{KOBO_BASE}/{country}/{language}/search?query={}&fcmedia=Book&pageNumber=1&fclanguages={language}",
        urlencoding_lite(query.trim())
    );
    let html = match http_get_text(&url, Some(KOBO_BASE)) {
        Ok(h) => h,
        Err(_) => return Ok(vec![]),
    };
    if is_challenge(&html) {
        return Ok(vec![]);
    }
    let links = extract_search_results(&html, q.limit.min(5));
    let mut out = Vec::new();
    for (provider_id, book_url) in links {
        let page = match http_get_text(&book_url, Some(KOBO_BASE)) {
            Ok(h) => h,
            Err(_) => continue,
        };
        if is_challenge(&page) {
            continue;
        }
        if let Some(c) = parse_book_page(&page, &provider_id, &book_url) {
            out.push(c);
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    Ok(out)
}

fn kobo_region(q: &SearchQuery) -> (String, String) {
    let mut country = "us".to_string();
    let mut language = "en".to_string();
    if let Some(raw) = q.config_json.as_deref() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(c) = v
                .get("country")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                country = c.to_ascii_lowercase();
            }
            if let Some(l) = v
                .get("language")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                language = l.to_ascii_lowercase();
            }
        }
    }
    (country, language)
}

fn is_challenge(html: &str) -> bool {
    // BO isKoboChallengePage — coarse signals
    let lower = html.to_ascii_lowercase();
    lower.contains("cf-browser-verification")
        || lower.contains("cdn-cgi/challenge")
        || lower.contains("attention required")
        || (lower.contains("cloudflare") && lower.contains("challenge"))
}

fn extract_search_results(html: &str, limit: usize) -> Vec<(String, String)> {
    // BO extractKoboSearchResults — /ebook/ links with provider id
    let re = Regex::new(r#"href="([^"]*?/ebook/[^"]+)""#).unwrap();
    let id_re = Regex::new(
        r"(?i)(?:ebook/[^/]+/)?([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})",
    )
    .unwrap();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for cap in re.captures_iter(html) {
        let href = cap.get(1).unwrap().as_str();
        let Some(id_cap) = id_re.captures(href) else {
            continue;
        };
        let id = id_cap.get(1).unwrap().as_str().to_ascii_lowercase();
        if !seen.insert(id.clone()) {
            continue;
        }
        let url = if href.starts_with("http") {
            href.to_string()
        } else {
            format!("{KOBO_BASE}{href}")
        };
        out.push((id, url));
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn parse_book_page(html: &str, provider_id: &str, source_url: &str) -> Option<Candidate> {
    let title = meta(html, "og:title")
        .or_else(|| tag_text(html, r#"<h1[^>]*class="[^"]*title[^"]*"[^>]*>(.*?)</h1>"#))
        .or_else(|| tag_text(html, r#"<h1[^>]*data-testid="title"[^>]*>(.*?)</h1>"#))?;
    let authors = {
        let re = Regex::new(r#"(?is)<a[^>]*href="[^"]*/author/[^"]*"[^>]*>(.*?)</a>"#).unwrap();
        let mut seen = std::collections::HashSet::new();
        re.captures_iter(html)
            .filter_map(|c| cleaned_text(c.get(1)?.as_str()))
            .filter(|n| seen.insert(n.clone()))
            .take(8)
            .collect::<Vec<_>>()
    };
    let description = attr(html, "data-full-synopsis")
        .or_else(|| meta(html, "og:description"))
        .or_else(|| meta(html, "description"));
    let cover = {
        let raw = Regex::new(r#"<(?:img)[^>]*class="[^"]*cover-image[^"]*"[^>]*src="([^"]+)""#)
            .unwrap()
            .captures(html)
            .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
            .or_else(|| meta(html, "og:image"));
        raw.map(|u| {
            if u.starts_with("http") {
                u
            } else {
                format!("{KOBO_BASE}{u}")
            }
        })
    };
    let isbn13 = capture_label(html, r"ISBN:\s*</[^>]+>\s*([^<\s]+)")
        .or_else(|| capture_label(html, r"ISBN</span>\s*<span[^>]*>([^<]+)"));
    let publisher = capture_label(html, r"Publisher:\s*</[^>]+>\s*([^<]+)");
    let language = capture_label(html, r"Language:\s*</[^>]+>\s*([^<]+)");
    let published = capture_label(html, r"Release Date:\s*</[^>]+>\s*([^<]+)");

    Some(Candidate {
        provider: "kobo".into(),
        external_id: provider_id.to_string(),
        title: Some(title),
        subtitle: meta(html, "book:subtitle"),
        description,
        authors,
        narrators: vec![],
        genres: vec![],
        tags: vec![],
        series_name: None,
        series_index: None,
        publisher,
        published_date: published.clone(),
        published_year: published.as_deref().and_then(|s| {
            Regex::new(r"(\d{4})")
                .unwrap()
                .captures(s)
                .and_then(|c| c.get(1)?.as_str().parse().ok())
        }),
        language,
        page_count: None,
        isbn10: None,
        isbn13: isbn13.map(|s| s.chars().filter(|c| c.is_ascii_digit()).collect()),
        cover_url: cover,
        audio_cover_url: None,
        source_url: Some(source_url.to_string()),
        community_rating: None,
    })
}

fn meta(html: &str, prop: &str) -> Option<String> {
    let pat = format!(r#"(?is)<meta[^>]*(?:property|name)="{prop}"[^>]*content="([^"]+)""#);
    let pat2 = format!(r#"(?is)<meta[^>]*content="([^"]+)"[^>]*(?:property|name)="{prop}""#);
    Regex::new(&pat)
        .ok()?
        .captures(html)
        .and_then(|c| trim_opt(Some(c.get(1)?.as_str())))
        .or_else(|| {
            Regex::new(&pat2)
                .ok()?
                .captures(html)
                .and_then(|c| trim_opt(Some(c.get(1)?.as_str())))
        })
}

fn attr(html: &str, name: &str) -> Option<String> {
    let pat = format!(r#"{name}="([^"]+)""#);
    Regex::new(&pat)
        .ok()?
        .captures(html)
        .and_then(|c| trim_opt(Some(c.get(1)?.as_str())))
}

fn tag_text(html: &str, pat: &str) -> Option<String> {
    Regex::new(pat)
        .ok()?
        .captures(html)
        .and_then(|c| cleaned_text(c.get(1)?.as_str()))
}

fn capture_label(html: &str, pat: &str) -> Option<String> {
    Regex::new(pat)
        .ok()?
        .captures(html)
        .and_then(|c| cleaned_text(c.get(1)?.as_str()))
}

fn cleaned_text(s: &str) -> Option<String> {
    let cleaned = strip_tags(s);
    let t = cleaned.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn strip_tags(s: &str) -> String {
    Regex::new(r"<[^>]+>")
        .unwrap()
        .replace_all(s, " ")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
