//! OPDS catalog (M12 slice 7): Atom feeds + Basic-auth acquisition for external readers.
//!
//! BO reference @ c77966cc: root nav, catalog/search, authors/series/collections,
//! OpenSearch, acquisition download/cover. Shelf skips surprise/libraries/smart-scopes,
//! cover HMAC tokens, read-aloud variants, and OPDS sort prefs.

use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

const BASE: &str = "/api/v1/opds";
const SEARCH_TEMPLATE: &str = "/api/v1/opds/catalog?q={searchTerms}";
const OPDS_MIME_NAV: &str = "application/atom+xml;profile=opds-catalog;kind=navigation";
const OPDS_MIME_ACQ: &str = "application/atom+xml;profile=opds-catalog;kind=acquisition";
const OPDS_MIME_SEARCH: &str = "application/opensearchdescription+xml";
const OPDS_MIME_ATOM: &str = "application/atom+xml";
const HIDE_WORK_EDITIONS: &str = "b.merged_into IS NULL AND b.id NOT IN (
  SELECT audio_book_id FROM book_works
  UNION
  SELECT readaloud_book_id FROM book_works WHERE readaloud_book_id IS NOT NULL
)";

pub(crate) fn migrate(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS opds_users (
           id INTEGER PRIMARY KEY,
           user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
           username TEXT NOT NULL COLLATE NOCASE,
           password_hash TEXT NOT NULL,
           created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
           UNIQUE(username)
         );
         CREATE INDEX IF NOT EXISTS opds_users_user ON opds_users(user_id)",
    )?;
    Ok(())
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let cp = ch as u32;
        if (cp < 0x20 && ch != '\t' && ch != '\n' && ch != '\r') || cp == 0xFFFE || cp == 0xFFFF {
            continue;
        }
        if (0xD800..=0xDFFF).contains(&cp) {
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

fn pct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn xml_el(tag: &str, text: &str) -> String {
    format!("<{tag}>{}</{tag}>", esc(text))
}

fn xml_link(rel: &str, href: &str, type_: &str, title: Option<&str>) -> String {
    match title {
        Some(t) => format!(
            "<link rel=\"{}\" href=\"{}\" type=\"{}\" title=\"{}\"/>",
            esc(rel),
            esc(href),
            esc(type_),
            esc(t)
        ),
        None => format!(
            "<link rel=\"{}\" href=\"{}\" type=\"{}\"/>",
            esc(rel),
            esc(href),
            esc(type_)
        ),
    }
}

fn file_mime(format: &str) -> &'static str {
    match format.to_ascii_lowercase().as_str() {
        "epub" => "application/epub+zip",
        "pdf" => "application/pdf",
        "djvu" => "image/vnd.djvu",
        "cbz" => "application/vnd.comicbook+zip",
        "cbr" => "application/vnd.comicbook-rar",
        "cb7" => "application/x-cb7",
        "m4b" | "m4a" => "audio/mp4",
        "mp3" => "audio/mpeg",
        "ogg" | "opus" => "audio/ogg",
        "flac" => "audio/flac",
        "audiozip" => "application/zip",
        _ => "application/octet-stream",
    }
}

fn utc_now(db: &Connection) -> String {
    db.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')", [], |r| {
        r.get(0)
    })
    .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

fn search_links() -> Vec<String> {
    vec![
        xml_link(
            "search",
            &format!("{BASE}/search.opds"),
            OPDS_MIME_SEARCH,
            None,
        ),
        xml_link("search", SEARCH_TEMPLATE, OPDS_MIME_ATOM, Some("Search")),
    ]
}

fn nav_entry(id: &str, title: &str, content: &str, href: &str, updated: &str) -> String {
    [
        "<entry>".into(),
        format!("  {}", xml_el("title", title)),
        format!("  {}", xml_el("id", id)),
        format!("  {}", xml_el("updated", updated)),
        format!("  <content type=\"text\">{}</content>", esc(content)),
        format!("  {}", xml_link("subsection", href, OPDS_MIME_NAV, None)),
        "</entry>".into(),
    ]
    .join("\n")
}

fn wrap_feed(
    title: &str,
    id: &str,
    updated: &str,
    links: &[String],
    entries: &[String],
    total: Option<i64>,
) -> String {
    let mut lines = vec![
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".into(),
        "<feed xmlns=\"http://www.w3.org/2005/Atom\"".into(),
        "      xmlns:dc=\"http://purl.org/dc/terms/\"".into(),
        "      xmlns:opds=\"http://opds-spec.org/2010/catalog\"".into(),
        "      xmlns:opensearch=\"http://a9.com/-/spec/opensearch/1.1/\">".into(),
        format!("  {}", xml_el("title", title)),
        format!("  {}", xml_el("id", id)),
        format!("  {}", xml_el("updated", updated)),
    ];
    if let Some(n) = total {
        lines.push(format!(
            "  {}",
            xml_el("opensearch:totalResults", &n.to_string())
        ));
    }
    for link in links {
        lines.push(format!("  {link}"));
    }
    for entry in entries {
        lines.push(entry.clone());
    }
    lines.push("</feed>".into());
    lines.join("\n")
}

fn xml_response(mime: &'static str, body: String) -> Response {
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

fn opds_unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [
            (header::WWW_AUTHENTICATE, "Basic realm=\"Shelf OPDS\""),
            (header::CACHE_CONTROL, "no-store"),
        ],
        Json(json!({"error": "Basic authentication required"})),
    )
        .into_response()
}

fn authenticate_opds(db: &Connection, headers: &HeaderMap) -> ApiResult<i64> {
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError(
            StatusCode::UNAUTHORIZED,
            "Basic authentication required",
        ))?;
    let encoded = auth.strip_prefix("Basic ").ok_or(ApiError(
        StatusCode::UNAUTHORIZED,
        "Basic authentication required",
    ))?;
    let decoded = STANDARD
        .decode(encoded.trim())
        .map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "Invalid credentials"))?;
    let pair = String::from_utf8(decoded)
        .map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "Invalid credentials"))?;
    let (username, password) = pair
        .split_once(':')
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "Invalid credentials"))?;
    if username.is_empty() || username.len() > 100 || password.len() > 1024 {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "Invalid credentials"));
    }
    let record: Option<(i64, String)> = db
        .query_row(
            "SELECT user_id, password_hash FROM opds_users WHERE username=? COLLATE NOCASE",
            [username],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((user_id, hash)) = record else {
        // Same padding verify as login so unknown usernames stay constant-time-ish.
        let dummy_salt = SaltString::encode_b64(b"shelf-opds-padding")
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Authentication failed."))?;
        let dummy = Argon2::default()
            .hash_password(b"not-an-opds-user", &dummy_salt)
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Authentication failed."))?
            .to_string();
        let parsed = PasswordHash::new(&dummy)
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Authentication failed."))?;
        let _ = Argon2::default().verify_password(password.as_bytes(), &parsed);
        return Err(ApiError(StatusCode::UNAUTHORIZED, "Invalid credentials"));
    };
    let parsed = PasswordHash::new(&hash)
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Authentication failed."))?;
    if Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_err()
    {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "Invalid credentials"));
    }
    Ok(user_id)
}

#[allow(clippy::result_large_err)] // WWW-Authenticate 401 must be axum Response, not ApiError
fn require_opds(db: &Connection, headers: &HeaderMap) -> Result<i64, Response> {
    match authenticate_opds(db, headers) {
        Ok(user) => Ok(user),
        Err(e) if e.0 == StatusCode::UNAUTHORIZED => Err(opds_unauthorized()),
        Err(e) => Err(e.into_response()),
    }
}

fn root_feed(updated: &str) -> String {
    wrap_feed(
        "Shelf OPDS Catalog",
        "urn:shelf:opds:root",
        updated,
        &[
            xml_link("self", BASE, OPDS_MIME_NAV, None),
            xml_link("start", BASE, OPDS_MIME_NAV, None),
        ]
        .into_iter()
        .chain(search_links())
        .collect::<Vec<_>>(),
        &[
            nav_entry(
                "urn:shelf:opds:all",
                "All Books",
                "Browse the full catalog",
                &format!("{BASE}/catalog"),
                updated,
            ),
            nav_entry(
                "urn:shelf:opds:recent",
                "Recent Books",
                "Recently added books",
                &format!("{BASE}/recent"),
                updated,
            ),
            nav_entry(
                "urn:shelf:opds:collections",
                "Collections",
                "Browse your collections",
                &format!("{BASE}/collections"),
                updated,
            ),
            nav_entry(
                "urn:shelf:opds:authors",
                "Authors",
                "Browse by author",
                &format!("{BASE}/authors"),
                updated,
            ),
            nav_entry(
                "urn:shelf:opds:series",
                "Series",
                "Browse by series",
                &format!("{BASE}/series"),
                updated,
            ),
        ],
        None,
    )
}

fn open_search_description() -> String {
    [
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
        "<OpenSearchDescription xmlns=\"http://a9.com/-/spec/opensearch/1.1/\">",
        &format!("  {}", xml_el("ShortName", "Shelf OPDS")),
        &format!(
            "  {}",
            xml_el("Description", "Search the Shelf book catalog")
        ),
        &format!(
            "  <Url type=\"{}\" template=\"{}\"/>",
            esc(OPDS_MIME_ACQ),
            esc(SEARCH_TEMPLATE)
        ),
        "  <InputEncoding>UTF-8</InputEncoding>",
        "  <OutputEncoding>UTF-8</OutputEncoding>",
        "</OpenSearchDescription>",
    ]
    .join("\n")
}

struct CatalogQuery {
    q: Option<String>,
    author: Option<String>,
    series: Option<String>,
    collection_id: Option<i64>,
    page: i64,
    size: i64,
    recent_only: bool,
}

#[derive(Deserialize)]
pub(crate) struct CatalogParams {
    q: Option<String>,
    author: Option<String>,
    series: Option<String>,
    #[serde(rename = "collectionId")]
    collection_id: Option<i64>,
    page: Option<i64>,
    size: Option<i64>,
}

struct BookEntry {
    id: i64,
    title: String,
    author: String,
    series: Option<String>,
    series_index: Option<f64>,
    format: Option<String>,
    has_cover: bool,
    updated_at: String,
}

fn fetch_books(
    db: &Connection,
    user: i64,
    query: &CatalogQuery,
) -> ApiResult<(Vec<BookEntry>, i64)> {
    let mut where_sql = format!("WHERE b.user_id=? AND {HIDE_WORK_EDITIONS}");
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(user)];
    if let Some(ref author) = query.author {
        where_sql.push_str(" AND b.author=?");
        params_vec.push(Box::new(author.clone()));
    }
    if let Some(ref series) = query.series {
        where_sql.push_str(" AND b.series=?");
        params_vec.push(Box::new(series.clone()));
    }
    if let Some(collection_id) = query.collection_id {
        let owned: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM collections WHERE id=? AND user_id=?)",
            params![collection_id, user],
            |r| r.get(0),
        )?;
        if !owned {
            return Err(ApiError(StatusCode::NOT_FOUND, "Collection not found."));
        }
        where_sql
            .push_str(" AND b.id IN (SELECT book_id FROM collection_books WHERE collection_id=?)");
        params_vec.push(Box::new(collection_id));
    }
    if let Some(ref q) = query.q {
        let needle = format!("%{q}%");
        where_sql
            .push_str(" AND (b.title LIKE ? COLLATE NOCASE OR b.author LIKE ? COLLATE NOCASE)");
        params_vec.push(Box::new(needle.clone()));
        params_vec.push(Box::new(needle));
    }
    // Only books with a downloadable file belong in acquisition feeds.
    where_sql.push_str(" AND f.book_id IS NOT NULL");

    let count_sql = format!(
        "SELECT count(*) FROM books b LEFT JOIN epub_files f ON f.book_id=b.id {where_sql}"
    );
    let count_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let total: i64 = db.query_row(&count_sql, count_refs.as_slice(), |r| r.get(0))?;

    let order = if query.recent_only {
        "ORDER BY b.id DESC"
    } else {
        "ORDER BY b.title COLLATE NOCASE ASC, b.id ASC"
    };
    let offset = (query.page - 1) * query.size;
    let list_sql = format!(
        "SELECT b.id,b.title,b.author,b.series,b.series_index,f.format,f.cover IS NOT NULL,
                strftime('%Y-%m-%dT%H:%M:%SZ','now')
         FROM books b LEFT JOIN epub_files f ON f.book_id=b.id
         {where_sql} {order} LIMIT ? OFFSET ?"
    );
    params_vec.push(Box::new(query.size));
    params_vec.push(Box::new(offset));
    let list_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = db.prepare(&list_sql)?;
    let books = stmt
        .query_map(list_refs.as_slice(), |r| {
            Ok(BookEntry {
                id: r.get(0)?,
                title: r.get(1)?,
                author: r.get(2)?,
                series: r.get(3)?,
                series_index: r.get(4)?,
                format: r.get(5)?,
                has_cover: r.get(6)?,
                updated_at: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok((books, total))
}

fn book_entry_xml(book: &BookEntry) -> String {
    let mut lines = vec![
        "<entry>".into(),
        format!("  {}", xml_el("title", &book.title)),
        format!("  {}", xml_el("id", &format!("urn:shelf:book:{}", book.id))),
        format!("  {}", xml_el("updated", &book.updated_at)),
        format!("  <author>{}</author>", xml_el("name", &book.author)),
        "  <content type=\"text\"/>".into(),
    ];
    if let Some(ref series) = book.series {
        let href = format!("{BASE}/catalog?series={}", pct(series));
        let title = match book.series_index {
            Some(idx) => format!("{series} #{idx}"),
            None => series.clone(),
        };
        lines.push(format!(
            "  {}",
            xml_link(
                "http://opds-spec.org/sort/series",
                &href,
                OPDS_MIME_ACQ,
                Some(&title),
            )
        ));
    }
    if book.has_cover {
        lines.push(format!(
            "  {}",
            xml_link(
                "http://opds-spec.org/image",
                &format!("{BASE}/{}/cover", book.id),
                "image/jpeg",
                None,
            )
        ));
        lines.push(format!(
            "  {}",
            xml_link(
                "http://opds-spec.org/image/thumbnail",
                &format!("{BASE}/{}/cover", book.id),
                "image/jpeg",
                None,
            )
        ));
    }
    if let Some(ref format) = book.format {
        let label = format.to_ascii_uppercase();
        lines.push(format!(
            "  {}",
            xml_link(
                "http://opds-spec.org/acquisition",
                &format!("{BASE}/{}/file", book.id),
                file_mime(format),
                Some(&label),
            )
        ));
    }
    lines.push("</entry>".into());
    lines.join("\n")
}

#[allow(clippy::too_many_arguments)] // feed metadata + pagination stay positional
fn acquisition_feed(
    title: &str,
    feed_id: &str,
    updated: &str,
    books: &[BookEntry],
    total: i64,
    page: i64,
    size: i64,
    self_path: &str,
) -> String {
    let total_pages = ((total as f64) / (size as f64)).ceil().max(1.0) as i64;
    let mut links = vec![
        xml_link("self", self_path, OPDS_MIME_ACQ, None),
        xml_link("start", BASE, OPDS_MIME_NAV, None),
    ];
    links.extend(search_links());

    let page_url = |p: i64| -> String {
        if self_path.contains('?') {
            // Replace or append page=
            let mut parts: Vec<String> = self_path
                .split('?')
                .nth(1)
                .unwrap_or("")
                .split('&')
                .filter(|s| !s.is_empty() && !s.starts_with("page="))
                .map(str::to_string)
                .collect();
            parts.push(format!("page={p}"));
            format!(
                "{}?{}",
                self_path.split('?').next().unwrap_or(self_path),
                parts.join("&")
            )
        } else {
            format!("{self_path}?page={p}")
        }
    };
    if page > 1 {
        links.push(xml_link("first", &page_url(1), OPDS_MIME_ACQ, None));
        links.push(xml_link(
            "previous",
            &page_url(page - 1),
            OPDS_MIME_ACQ,
            None,
        ));
    }
    if page < total_pages {
        links.push(xml_link("next", &page_url(page + 1), OPDS_MIME_ACQ, None));
        links.push(xml_link(
            "last",
            &page_url(total_pages),
            OPDS_MIME_ACQ,
            None,
        ));
    }

    let entries: Vec<String> = books.iter().map(book_entry_xml).collect();
    wrap_feed(title, feed_id, updated, &links, &entries, Some(total))
}

fn parse_catalog(params: CatalogParams, recent_only: bool) -> CatalogQuery {
    CatalogQuery {
        q: params.q.filter(|s| !s.trim().is_empty()),
        author: params.author.filter(|s| !s.is_empty()),
        series: params.series.filter(|s| !s.is_empty()),
        collection_id: params.collection_id,
        page: params.page.unwrap_or(1).max(1),
        size: params.size.unwrap_or(50).clamp(1, 100),
        recent_only,
    }
}

fn self_path_for(query: &CatalogQuery, base: &str) -> String {
    let mut parts = Vec::new();
    if let Some(ref q) = query.q {
        parts.push(format!("q={}", pct(q)));
    }
    if let Some(ref author) = query.author {
        parts.push(format!("author={}", pct(author)));
    }
    if let Some(ref series) = query.series {
        parts.push(format!("series={}", pct(series)));
    }
    if let Some(id) = query.collection_id {
        parts.push(format!("collectionId={id}"));
    }
    if query.page != 1 {
        parts.push(format!("page={}", query.page));
    }
    if query.size != 50 {
        parts.push(format!("size={}", query.size));
    }
    if parts.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", parts.join("&"))
    }
}

pub(super) async fn root(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match state
        .db(move |db| {
            let _user = match require_opds(db, &headers) {
                Ok(u) => u,
                Err(resp) => return Ok(resp),
            };
            let updated = utc_now(db);
            Ok(xml_response(OPDS_MIME_NAV, root_feed(&updated)))
        })
        .await
    {
        Ok(resp) => resp,
        Err(e) => e.into_response(),
    }
}

pub(super) async fn search_desc(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match state
        .db(move |db| {
            let _user = match require_opds(db, &headers) {
                Ok(u) => u,
                Err(resp) => return Ok(resp),
            };
            Ok(xml_response(OPDS_MIME_SEARCH, open_search_description()))
        })
        .await
    {
        Ok(resp) => resp,
        Err(e) => e.into_response(),
    }
}

async fn catalog_like(
    state: AppState,
    headers: HeaderMap,
    params: CatalogParams,
    recent_only: bool,
    title: &str,
    feed_id: &str,
    path_base: &str,
) -> Response {
    let title = title.to_string();
    let feed_id = feed_id.to_string();
    let path_base = path_base.to_string();
    match state
        .db(move |db| {
            let user = match require_opds(db, &headers) {
                Ok(u) => u,
                Err(resp) => return Ok(resp),
            };
            let query = parse_catalog(params, recent_only);
            if (query.page - 1).saturating_mul(query.size) > 10_000 {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "pagination window is too deep",
                ));
            }
            let (books, total) = fetch_books(db, user, &query)?;
            let self_path = self_path_for(&query, &path_base);
            let updated = utc_now(db);
            let xml = acquisition_feed(
                &title, &feed_id, &updated, &books, total, query.page, query.size, &self_path,
            );
            Ok(xml_response(OPDS_MIME_ACQ, xml))
        })
        .await
    {
        Ok(resp) => resp,
        Err(e) => e.into_response(),
    }
}

pub(super) async fn catalog(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<CatalogParams>,
) -> Response {
    catalog_like(
        state,
        headers,
        params,
        false,
        "All Books",
        "urn:shelf:opds:catalog",
        &format!("{BASE}/catalog"),
    )
    .await
}

pub(super) async fn recent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<CatalogParams>,
) -> Response {
    catalog_like(
        state,
        headers,
        params,
        true,
        "Recent Books",
        "urn:shelf:opds:recent",
        &format!("{BASE}/recent"),
    )
    .await
}

pub(super) async fn authors(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match state
        .db(move |db| {
            let user = match require_opds(db, &headers) {
                Ok(u) => u,
                Err(resp) => return Ok(resp),
            };
            let updated = utc_now(db);
            let sql = format!(
                "SELECT b.author, count(*) FROM books b
                 WHERE b.user_id=? AND {HIDE_WORK_EDITIONS} AND trim(b.author) != ''
                 GROUP BY b.author ORDER BY b.author COLLATE NOCASE"
            );
            let mut stmt = db.prepare(&sql)?;
            let entries: Vec<String> = stmt
                .query_map(params![user], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                })?
                .filter_map(|row| row.ok())
                .map(|(name, count)| {
                    nav_entry(
                        &format!("urn:shelf:opds:author:{}", pct(&name)),
                        &name,
                        &format!("{count} books"),
                        &format!("{BASE}/catalog?author={}", pct(&name)),
                        &updated,
                    )
                })
                .collect();
            let xml = wrap_feed(
                "Authors",
                "urn:shelf:opds:authors",
                &updated,
                &[xml_link(
                    "self",
                    &format!("{BASE}/authors"),
                    OPDS_MIME_NAV,
                    None,
                )]
                .into_iter()
                .chain(search_links())
                .collect::<Vec<_>>(),
                &entries,
                None,
            );
            Ok(xml_response(OPDS_MIME_NAV, xml))
        })
        .await
    {
        Ok(resp) => resp,
        Err(e) => e.into_response(),
    }
}

pub(super) async fn series(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match state
        .db(move |db| {
            let user = match require_opds(db, &headers) {
                Ok(u) => u,
                Err(resp) => return Ok(resp),
            };
            let updated = utc_now(db);
            let sql = format!(
                "SELECT b.series, count(*) FROM books b
                 WHERE b.user_id=? AND {HIDE_WORK_EDITIONS} AND b.series IS NOT NULL AND trim(b.series) != ''
                 GROUP BY b.series ORDER BY b.series COLLATE NOCASE"
            );
            let mut stmt = db.prepare(&sql)?;
            let entries: Vec<String> = stmt
                .query_map(params![user], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                })?
                .filter_map(|row| row.ok())
                .map(|(name, count)| {
                    nav_entry(
                        &format!("urn:shelf:opds:series:{}", pct(&name)),
                        &name,
                        &format!("{count} books"),
                        &format!("{BASE}/catalog?series={}", pct(&name)),
                        &updated,
                    )
                })
                .collect();
            let xml = wrap_feed(
                "Series",
                "urn:shelf:opds:series",
                &updated,
                &[xml_link(
                    "self",
                    &format!("{BASE}/series"),
                    OPDS_MIME_NAV,
                    None,
                )]
                .into_iter()
                .chain(search_links())
                .collect::<Vec<_>>(),
                &entries,
                None,
            );
            Ok(xml_response(OPDS_MIME_NAV, xml))
        })
        .await
    {
        Ok(resp) => resp,
        Err(e) => e.into_response(),
    }
}

pub(super) async fn collections(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match state
        .db(move |db| {
            let user = match require_opds(db, &headers) {
                Ok(u) => u,
                Err(resp) => return Ok(resp),
            };
            let updated = utc_now(db);
            let mut stmt = db.prepare(
                "SELECT c.id, c.name,
                        (SELECT count(*) FROM collection_books cb WHERE cb.collection_id=c.id)
                 FROM collections c WHERE c.user_id=? ORDER BY c.name COLLATE NOCASE",
            )?;
            let entries: Vec<String> = stmt
                .query_map(params![user], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                })?
                .filter_map(|row| row.ok())
                .map(|(id, name, count)| {
                    nav_entry(
                        &format!("urn:shelf:opds:collection:{id}"),
                        &name,
                        &format!("{count} books"),
                        &format!("{BASE}/catalog?collectionId={id}"),
                        &updated,
                    )
                })
                .collect();
            let xml = wrap_feed(
                "Collections",
                "urn:shelf:opds:collections",
                &updated,
                &[xml_link(
                    "self",
                    &format!("{BASE}/collections"),
                    OPDS_MIME_NAV,
                    None,
                )]
                .into_iter()
                .chain(search_links())
                .collect::<Vec<_>>(),
                &entries,
                None,
            );
            Ok(xml_response(OPDS_MIME_NAV, xml))
        })
        .await
    {
        Ok(resp) => resp,
        Err(e) => e.into_response(),
    }
}

pub(super) async fn download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let data_dir = state.data_dir.clone();
    let row = match state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = match require_opds(db, &headers) {
                    Ok(u) => u,
                    Err(resp) => return Ok(Err(resp)),
                };
                match owned_book_file_row(db, user, id) {
                    Ok(row) => Ok(Ok(row)),
                    Err(e) if e.0 == StatusCode::NOT_FOUND => Err(e),
                    Err(e) => Err(e),
                }
            }
        })
        .await
    {
        Ok(Ok(row)) => row,
        Ok(Err(resp)) => return resp,
        Err(e) => return e.into_response(),
    };
    let (inline, storage_path, format) = row;
    if format == "aax" {
        return ApiError(
            StatusCode::CONFLICT,
            "This audiobook is still converting from AAX.",
        )
        .into_response();
    }
    let (mime, disposition) = book_file_mime(&format);
    if let Some(relative) = storage_path.filter(|p| !p.is_empty()) {
        if relative.contains("..") || PathBuf::from(&relative).is_absolute() {
            return ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid on-disk book path.",
            )
            .into_response();
        }
        return match stream_book_file(
            data_dir.join(relative),
            mime,
            disposition,
            headers.get(header::RANGE),
        )
        .await
        {
            Ok(resp) => resp,
            Err(e) => e.into_response(),
        };
    }
    let mut response = (
        [
            (header::CONTENT_TYPE, mime),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        inline,
    )
        .into_response();
    if audiobook::is_audio_format(&format) {
        response.headers_mut().insert(
            header::ACCEPT_RANGES,
            header::HeaderValue::from_static("bytes"),
        );
    }
    response
}

pub(super) async fn cover(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let data_dir = state.data_dir.clone();
    match state
        .db(move |db| {
            let user = match require_opds(db, &headers) {
                Ok(u) => u,
                Err(resp) => return Ok(resp),
            };
            let owned: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM books WHERE id=? AND user_id=?)",
                params![id, user],
                |r| r.get(0),
            )?;
            if !owned {
                return Err(ApiError(StatusCode::NOT_FOUND, "Cover not found."));
            }
            let _ = audio_meta::enrich_audio_book(db, &data_dir, id);
            let (content, mime) = db
                .query_row(
                    "SELECT f.cover,f.cover_mime FROM epub_files f JOIN books b ON b.id=f.book_id
                     WHERE b.id=? AND b.user_id=? AND f.cover IS NOT NULL",
                    params![id, user],
                    |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()?
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Cover not found."))?;
            Ok(([(header::CONTENT_TYPE, mime)], content).into_response())
        })
        .await
    {
        Ok(resp) => resp,
        Err(e) => e.into_response(),
    }
}

// --- Session-auth management of OPDS credentials ---

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateOpdsUser {
    username: String,
    password: String,
}

pub(super) async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let mut stmt = db.prepare(
                "SELECT id, username, created_at FROM opds_users WHERE user_id=? ORDER BY id",
            )?;
            let rows: Vec<Value> = stmt
                .query_map(params![user], |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "username": r.get::<_, String>(1)?,
                        "createdAt": r.get::<_, String>(2)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Json(json!({"users": rows})))
        })
        .await
}

pub(super) async fn create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateOpdsUser>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let username = body.username.trim().to_string();
    if username.is_empty() || username.len() > 100 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Username must be 1–100 characters.",
        ));
    }
    if body.password.len() < 8 || body.password.len() > 1024 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Password must be 8–1024 characters.",
        ));
    }
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let salt = SaltString::generate(&mut OsRng);
            let hash = Argon2::default()
                .hash_password(body.password.as_bytes(), &salt)
                .map_err(|_| {
                    ApiError(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Could not create OPDS user.",
                    )
                })?
                .to_string();
            let result = db.execute(
                "INSERT INTO opds_users(user_id, username, password_hash) VALUES (?,?,?)",
                params![user, username, hash],
            );
            if let Err(rusqlite::Error::SqliteFailure(code, _)) = &result {
                if code.code == rusqlite::ErrorCode::ConstraintViolation {
                    return Err(ApiError(
                        StatusCode::CONFLICT,
                        "That OPDS username is already taken.",
                    ));
                }
            }
            result?;
            let id = db.last_insert_rowid();
            let created_at: String =
                db.query_row("SELECT created_at FROM opds_users WHERE id=?", [id], |r| {
                    r.get(0)
                })?;
            Ok((
                StatusCode::CREATED,
                Json(json!({
                    "id": id,
                    "username": username,
                    "createdAt": created_at,
                })),
            ))
        })
        .await
}

pub(super) async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let deleted = db.execute(
                "DELETE FROM opds_users WHERE id=? AND user_id=?",
                params![id, user],
            )?;
            if deleted == 0 {
                return Err(ApiError(StatusCode::NOT_FOUND, "OPDS user not found."));
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_strips_controls_and_escapes_markup() {
        assert_eq!(esc("a&b<c>\"d\""), "a&amp;b&lt;c&gt;&quot;d&quot;");
        assert_eq!(esc("ok\u{0001}x"), "okx");
    }

    #[test]
    fn root_feed_lists_catalog_sections() {
        let xml = root_feed("2026-01-01T00:00:00Z");
        assert!(xml.contains("All Books"));
        assert!(xml.contains("/api/v1/opds/catalog"));
        assert!(xml.contains("search.opds"));
        assert!(xml.contains("{searchTerms}"));
    }
}
