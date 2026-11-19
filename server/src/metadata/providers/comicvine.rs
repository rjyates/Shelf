//! Adapted from Book Orbit (`bookorbit/bookorbit`, AGPL-3.0):
//! https://github.com/bookorbit/bookorbit
//! See `/NOTICE` for full attribution.
//!
//! ComicVine API search (BO `comicvine.provider` / `comicvine.client` / `comicvine.matching`).

use super::{trim_opt, urlencoding_lite, Candidate, SearchQuery};
use crate::{ApiError, ApiResult};
use axum::http::StatusCode;
use regex::Regex;
use serde_json::Value;

const BASE: &str = "https://comicvine.gamespot.com/api";
const UA: &str = "Shelf/0.1 (Book Library; +https://github.com/rjyates/Shelf)";
const VOLUME_FIELDS: &str =
    "id,name,publisher,start_year,count_of_issues,description,deck,image,site_detail_url";
const ISSUE_FIELDS: &str =
    "id,name,issue_number,cover_date,description,deck,image,volume,site_detail_url,person_credits";

pub fn search(q: &SearchQuery, api_key: Option<&str>) -> ApiResult<Vec<Candidate>> {
    let Some(key) = api_key.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(vec![]);
    };
    let title = q
        .title
        .clone()
        .or_else(|| q.query.clone())
        .unwrap_or_default();
    if title.trim().is_empty() && q.series_name.as_deref().unwrap_or("").is_empty() {
        return Ok(vec![]);
    }

    // BO resolveComicVineIssueQuery — "Series #12" or series_name + series_index.
    if let Some(parsed) = resolve_issue_query(q) {
        let structured = structured_search(&parsed.0, &parsed.1, parsed.2, key, q.limit.min(10))?;
        if !structured.is_empty() {
            return Ok(structured);
        }
    }

    general_search(title.trim(), key, q.limit.min(10))
}

fn resolve_issue_query(q: &SearchQuery) -> Option<(String, String, Option<i64>)> {
    let re = Regex::new(r"^(.*?)\s*#(\d[\d.]*)(.*)$").unwrap();
    if let Some(title) = q.title.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(c) = re.captures(title) {
            let series = c.get(1)?.as_str().trim();
            let num = normalize_issue(c.get(2)?.as_str());
            if !series.is_empty() {
                let (base, year) = split_run_year(series);
                return Some((base, num, year));
            }
        }
    }
    let series = q
        .series_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let num = q
        .series_index
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(normalize_issue)?;
    let (base, year) = split_run_year(series);
    Some((base, num, year))
}

fn normalize_issue(s: &str) -> String {
    let t = s.trim().trim_start_matches('0');
    if t.is_empty() {
        "0".into()
    } else {
        t.to_string()
    }
}

fn split_run_year(series: &str) -> (String, Option<i64>) {
    let re = Regex::new(r"^(.*?)\s*\((\d{4})\)\s*$").unwrap();
    if let Some(c) = re.captures(series.trim()) {
        return (
            c.get(1).unwrap().as_str().trim().to_string(),
            c.get(2).unwrap().as_str().parse().ok(),
        );
    }
    (series.trim().to_string(), None)
}

fn structured_search(
    series_name: &str,
    issue_number: &str,
    start_year: Option<i64>,
    api_key: &str,
    limit: usize,
) -> ApiResult<Vec<Candidate>> {
    // BO: search volumes by name, then issues in volume by issue_number.
    let url = format!(
        "{BASE}/volumes/?api_key={}&format=json&filter=name:{}&field_list={}&limit=20",
        urlencoding_lite(api_key),
        urlencoding_lite(series_name),
        urlencoding_lite(VOLUME_FIELDS),
    );
    let body = match get_json(&url) {
        Ok(v) => v,
        Err(_) => return Ok(vec![]),
    };
    if body.get("status_code").and_then(|v| v.as_i64()) != Some(1) {
        return Ok(vec![]);
    }
    let mut volumes = body
        .get("results")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    // Prefer matching start_year when present (BO ranking).
    if let Some(year) = start_year {
        volumes.sort_by_key(|v| {
            let y = v.get("start_year").and_then(|x| {
                x.as_i64()
                    .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
            });
            if y == Some(year) {
                0
            } else {
                1
            }
        });
    }
    let mut out = Vec::new();
    for vol in volumes.into_iter().take(5) {
        let Some(vid) = vol.get("id").and_then(|v| v.as_i64()) else {
            continue;
        };
        let issue_url = format!(
            "{BASE}/issues/?api_key={}&format=json&filter=volume:{},issue_number:{}&field_list={}&limit=5",
            urlencoding_lite(api_key),
            vid,
            urlencoding_lite(issue_number),
            urlencoding_lite(ISSUE_FIELDS),
        );
        let ibody = match get_json(&issue_url) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if ibody.get("status_code").and_then(|v| v.as_i64()) != Some(1) {
            continue;
        }
        let issues = ibody
            .get("results")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for issue in issues {
            if let Some(mut c) = map_issue(&issue) {
                if c.publisher.is_none() {
                    c.publisher = vol
                        .pointer("/publisher/name")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                }
                out.push(c);
                if out.len() >= limit {
                    return Ok(out);
                }
            }
        }
    }
    Ok(out)
}

fn general_search(title: &str, api_key: &str, limit: usize) -> ApiResult<Vec<Candidate>> {
    let url = format!(
        "{BASE}/search/?api_key={}&format=json&query={}&resources=issue&field_list={}&limit={limit}",
        urlencoding_lite(api_key),
        urlencoding_lite(title),
        urlencoding_lite(ISSUE_FIELDS),
    );
    let body = match get_json(&url) {
        Ok(v) => v,
        Err(_) => return Ok(vec![]),
    };
    if body.get("status_code").and_then(|v| v.as_i64()) != Some(1) {
        return Ok(vec![]);
    }
    let results = body
        .get("results")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(results
        .into_iter()
        .filter_map(|issue| map_issue(&issue))
        .take(limit)
        .collect())
}

fn get_json(url: &str) -> ApiResult<Value> {
    let client = super::http_client(UA)?;
    let resp = client
        .get(url)
        .send()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "ComicVine request failed."))?;
    if !resp.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "ComicVine returned an error.",
        ));
    }
    resp.json()
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "ComicVine returned invalid JSON."))
}

fn map_issue(issue: &Value) -> Option<Candidate> {
    let id = issue.get("id")?.as_i64()?.to_string();
    let volume_name = issue
        .pointer("/volume/name")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let issue_number = issue.get("issue_number").and_then(|v| {
        v.as_str()
            .map(str::to_string)
            .or_else(|| v.as_i64().map(|n| n.to_string()))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    });
    let name = trim_opt(issue.get("name").and_then(|v| v.as_str()));
    let title = match (&volume_name, &issue_number, &name) {
        (Some(v), Some(n), Some(nm)) => format!("{v} #{n}: {nm}"),
        (Some(v), Some(n), None) => format!("{v} #{n}"),
        (Some(v), None, Some(nm)) => format!("{v}: {nm}"),
        (None, _, Some(nm)) => nm.clone(),
        (Some(v), None, None) => v.clone(),
        _ => return None,
    };
    let writers = issue
        .get("person_credits")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|c| {
                    c.get("role")
                        .and_then(|r| r.as_str())
                        .map(|r| {
                            r.to_ascii_lowercase()
                                .split(',')
                                .any(|p| p.trim() == "writer")
                        })
                        .unwrap_or(false)
                })
                .filter_map(|c| c.get("name").and_then(|n| n.as_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let published = trim_opt(issue.get("cover_date").and_then(|v| v.as_str()));
    Some(Candidate {
        provider: "comicvine".into(),
        external_id: id,
        title: Some(title),
        subtitle: None,
        description: trim_opt(
            issue
                .get("description")
                .or_else(|| issue.get("deck"))
                .and_then(|v| v.as_str()),
        )
        .map(|s| super::strip_html(&s)),
        authors: writers,
        narrators: vec![],
        genres: vec![],
        tags: vec![],
        series_name: volume_name,
        series_index: issue_number,
        publisher: None,
        published_date: published.clone(),
        published_year: published.as_deref().and_then(|s| s.get(0..4)?.parse().ok()),
        language: None,
        page_count: None,
        isbn10: None,
        isbn13: None,
        cover_url: issue
            .pointer("/image/original_url")
            .or_else(|| issue.pointer("/image/super_url"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        audio_cover_url: None,
        source_url: trim_opt(issue.get("site_detail_url").and_then(|v| v.as_str())),
        community_rating: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::providers::SearchQuery;

    #[test]
    fn parses_issue_hash_title() {
        let q = SearchQuery {
            title: Some("Batman (2016) #50".into()),
            author: None,
            isbn: None,
            query: None,
            series_name: None,
            series_index: None,
            is_audiobook: false,
            config_json: None,
            limit: 5,
        };
        let (series, num, year) = resolve_issue_query(&q).unwrap();
        assert_eq!(series, "Batman");
        assert_eq!(num, "50");
        assert_eq!(year, Some(2016));
    }
}
