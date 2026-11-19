//! Adapted from Book Orbit (`bookorbit/bookorbit`, AGPL-3.0):
//! https://github.com/bookorbit/bookorbit
//! See `/NOTICE` for full attribution.

//! Amazon product search + `/dp/` page scrape (BO `amazon.provider` / `amazon.scraper`).

use super::{trim_opt, urlencoding_lite, Candidate, SearchQuery, SCRAPER_UA};
use crate::{ApiError, ApiResult};
use axum::http::StatusCode;
use regex::Regex;

fn memo(pat: &str) -> Regex {
    Regex::new(pat).unwrap()
}

fn amazon_cfg(q: &SearchQuery) -> (String, String) {
    // BO amazon domain + cookie from provider config_json.
    let mut domain = "amazon.com".to_string();
    let mut cookie = String::new();
    if let Some(raw) = q.config_json.as_deref() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(d) = v
                .get("domain")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                domain = d
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .trim_start_matches("www.")
                    .to_string();
            }
            if let Some(c) = v
                .get("cookie")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                cookie = c.to_string();
            }
        }
    }
    (domain, cookie)
}

fn amazon_origin(domain: &str) -> String {
    format!("https://www.{domain}")
}

fn amazon_get(url: &str, origin: &str, cookie: &str) -> ApiResult<String> {
    let client = super::http_client(SCRAPER_UA)?;
    let mut req = client
        .get(url)
        .header(
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
        )
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Upgrade-Insecure-Requests", "1")
        .header("Referer", format!("{origin}/"));
    if !cookie.is_empty() {
        req = req.header("Cookie", cookie);
    }
    let resp = req
        .send()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "Amazon request failed."))?;
    if !resp.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "Amazon returned an error.",
        ));
    }
    let html = resp
        .text()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "Amazon returned invalid body."))?;
    if is_bot_challenge(&html) {
        return Err(ApiError(StatusCode::BAD_GATEWAY, "Amazon bot challenge."));
    }
    Ok(html)
}

fn is_bot_challenge(html: &str) -> bool {
    // BO amazon-http.utils isAmazonBotChallenge
    [
        "validateCaptcha",
        "not a robot",
        "triggerInterstitialChallenge",
        "bm-verify",
        "/_sec/verify?provider=interstitial",
    ]
    .iter()
    .any(|p| html.to_ascii_lowercase().contains(&p.to_ascii_lowercase()) || html.contains(p))
}

fn extract_asins(html: &str, limit: usize) -> Vec<String> {
    // BO extractAsins — ASIN links from search results; skip set-like titles when nearby.
    let skip = memo(
        r"(?i)box\s*set|collection\s*set|books\s*set|omnibus|summary\s*&\s*study|streamer|display\s*kit|bookstore\s*kit|shelf\s*kit",
    );
    let asin_re = memo(r#"/dp/([A-Z0-9]{10})"#);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for cap in asin_re.captures_iter(html) {
        let asin = cap.get(1).unwrap().as_str().to_ascii_uppercase();
        if !seen.insert(asin.clone()) {
            continue;
        }
        // Rough sponsored/set filter using a window around the match.
        let start = cap.get(0).unwrap().start().saturating_sub(400);
        let end = (cap.get(0).unwrap().end() + 200).min(html.len());
        let window = &html[start..end];
        if window.to_ascii_lowercase().contains("sponsored") || skip.is_match(window) {
            continue;
        }
        out.push(asin);
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn parse_book_page(html: &str, asin: &str) -> Option<Candidate> {
    // BO parseBookPage field extractors, regex-ported (no cheerio).
    let title_raw = memo(r#"(?is)id="(?:productTitle|ebooksProductTitle)"[^>]*>(.*?)</"#)
        .captures(html)
        .and_then(|c| cleaned_text(c.get(1)?.as_str()))?;
    let (title, subtitle) = match title_raw.split_once(':') {
        Some((t, s)) if !t.trim().is_empty() && !s.trim().is_empty() => {
            (t.trim().to_string(), Some(s.trim().to_string()))
        }
        _ => (title_raw, None),
    };
    let authors = memo(r#"(?is)id="bylineInfo[^"]*"[\s\S]{0,4000}?class="[^"]*author[^"]*"[\s\S]{0,400}?<a[^>]*>(.*?)</a>"#)
        .captures_iter(html)
        .filter_map(|c| cleaned_text(c.get(1)?.as_str()))
        .collect::<Vec<_>>();
    let description = memo(r#"(?is)id="bookDescription_feature_div"[\s\S]{0,8000}?class="[^"]*a-expander-content[^"]*"[^>]*>(.*?)</div>"#)
        .captures(html)
        .and_then(|c| cleaned_text(c.get(1)?.as_str()))
        .or_else(|| {
            memo(r#"(?is)id="bookDescription_feature_div"[\s\S]{0,8000}?<noscript>(.*?)</noscript>"#)
                .captures(html)
                .and_then(|c| cleaned_text(c.get(1)?.as_str()))
        });
    let isbn13 = extract_isbn(html, "isbn13", 13);
    let isbn10 = extract_isbn(html, "isbn10", 10);
    let publisher = attr_value(html, "publisher")
        .map(|s| memo(r"\s*\([^)]*\)\s*$").replace(&s, "").trim().to_string());
    let published_date = attr_value(html, "publication_date");
    let published_year = published_date.as_deref().and_then(|s| {
        memo(r"(\d{4})")
            .captures(s)
            .and_then(|c| c.get(1)?.as_str().parse().ok())
    });
    let language = attr_value(html, "language");
    let page_count = attr_value(html, "pages")
        .or_else(|| detail_bullet(html, r"(?i)print\s*length|paperback|hardcover"))
        .and_then(|s| {
            memo(r"(\d+)")
                .captures(&s)
                .and_then(|c| c.get(1)?.as_str().parse().ok())
        });
    let series_name = memo(r#"(?is)id="rpi-attribute-book_details-series"[\s\S]{0,800}?rpi-attribute-value[\s\S]{0,400}?<a[^>]*>\s*<span[^>]*>(.*?)</span>"#)
        .captures(html)
        .and_then(|c| cleaned_text(c.get(1)?.as_str()));
    let series_index = memo(
        r#"(?is)id="rpi-attribute-book_details-series"[\s\S]{0,600}?Book\s+(\d+(?:\.\d+)?)\s+of"#,
    )
    .captures(html)
    .and_then(|c| c.get(1).map(|m| m.as_str().to_string()));
    let cover_url = extract_cover(html);
    let rating = memo(r"(?i)([0-9]+(?:[.,][0-9]+)?)\s*out of\s*5")
        .captures(html)
        .and_then(|c| c.get(1)?.as_str().replace(',', ".").parse().ok());

    Some(Candidate {
        provider: "amazon".into(),
        external_id: asin.to_string(),
        title: Some(title),
        subtitle,
        description,
        authors,
        narrators: vec![],
        genres: vec![],
        tags: vec![],
        series_name,
        series_index,
        publisher,
        published_date,
        published_year,
        language,
        page_count,
        isbn10,
        isbn13,
        cover_url,
        audio_cover_url: None,
        source_url: Some(format!("https://www.amazon.com/dp/{asin}")),
        community_rating: rating,
    })
}

fn extract_isbn(html: &str, field: &str, len: usize) -> Option<String> {
    let pat = format!(
        r#"(?is)id="rpi-attribute-book_details-{field}"[\s\S]{{0,400}}?rpi-attribute-value[\s\S]{{0,200}}?<span[^>]*>(.*?)</span>"#
    );
    if let Some(c) = Regex::new(&pat).ok()?.captures(html) {
        let digits: String = strip_tags(c.get(1)?.as_str())
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect();
        if digits.len() == len {
            return Some(digits);
        }
    }
    let label = if field == "isbn13" {
        r"ISBN-?13"
    } else {
        r"ISBN-?10"
    };
    detail_bullet(html, label)
        .map(|s| {
            s.chars()
                .filter(|ch| ch.is_ascii_alphanumeric())
                .collect::<String>()
        })
        .filter(|s| s.len() == len)
}

fn attr_value(html: &str, field: &str) -> Option<String> {
    let pat = format!(
        r#"(?is)id="rpi-attribute-book_details-{field}"[\s\S]{{0,400}}?rpi-attribute-value[\s\S]{{0,200}}?<span[^>]*>(.*?)</span>"#
    );
    Regex::new(&pat)
        .ok()?
        .captures(html)
        .and_then(|c| cleaned_text(c.get(1)?.as_str()))
}

fn detail_bullet(html: &str, label: &str) -> Option<String> {
    let pat = format!(
        r#"(?is)<span[^>]*class="[^"]*a-text-bold[^"]*"[^>]*>\s*(?:{label})\s*:?\s*</span>\s*<span[^>]*>(.*?)</span>"#
    );
    Regex::new(&pat)
        .ok()?
        .captures(html)
        .and_then(|c| cleaned_text(c.get(1)?.as_str()))
}

fn extract_cover(html: &str) -> Option<String> {
    // data-a-dynamic-image JSON map — pick largest entry (BO extractCover).
    let raw = memo(r#"data-a-dynamic-image="([^"]+)""#)
        .captures(html)
        .and_then(|c| c.get(1).map(|m| html_unescape(m.as_str())))?;
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&raw).ok()?;
    let mut best: Option<(String, i64)> = None;
    for (url, size) in map {
        let area = size
            .as_array()
            .and_then(|a| Some(a.first()?.as_i64()? * a.get(1)?.as_i64()?))
            .unwrap_or(0);
        if best.as_ref().map(|(_, a)| area > *a).unwrap_or(true) {
            best = Some((url, area));
        }
    }
    best.map(|(u, _)| u).or_else(|| {
        memo(r#"(?is)id="(?:landingImage|imgBlkFront)"[^>]*src="([^"]+)""#)
            .captures(html)
            .and_then(|c| trim_opt(Some(c.get(1)?.as_str())))
    })
}

fn html_unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
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
    memo(r"<[^>]+>")
        .replace_all(&html_unescape(s), " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn search(q: &SearchQuery) -> ApiResult<Vec<Candidate>> {
    let (domain, cookie) = amazon_cfg(q);
    let origin = amazon_origin(&domain);

    let query = if let Some(isbn) = q.isbn.as_deref().filter(|s| !s.is_empty()) {
        isbn.to_string()
    } else {
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
    // BO: mobile search `/gp/aw/s?k=...&i=stripbooks`
    let search_url = format!(
        "{origin}/gp/aw/s?k={}&i=stripbooks",
        urlencoding_lite(query.trim())
    );
    let html = match amazon_get(&search_url, &origin, &cookie) {
        Ok(h) => h,
        Err(_) => return Ok(vec![]),
    };
    let limit = q.limit.min(5);
    let asins = extract_asins(&html, limit);
    let mut out = Vec::new();
    for asin in asins {
        let page_url = format!("{origin}/dp/{asin}");
        if let Ok(page) = amazon_get(&page_url, &origin, &cookie) {
            if let Some(c) = parse_book_page(&page, &asin) {
                out.push(c);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::extract_asins;

    #[test]
    fn extract_asins_from_result_links() {
        let pad = " ".repeat(500);
        let html = format!(
            r#"<div data-component-type="s-search-result"><a href="/dp/B08G9PRS1K">Kindle</a></div>{pad}<div>sponsored <a href="/dp/B00SPONSORED">x</a></div>"#
        );
        let asins = extract_asins(&html, 5);
        assert_eq!(asins, vec!["B08G9PRS1K".to_string()]);
    }
}
