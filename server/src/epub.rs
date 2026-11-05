use super::*;
use axum::{body::to_bytes, extract::Request};
use rust_embed::RustEmbed;
use std::io::Cursor;

/// Built Book Orbit EPUB reader (AGPL-3.0-only). See web/orbit-reader/vendor/bookorbit/ATTRIBUTION.md.
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../web/orbit-reader/dist"]
struct OrbitReaderAssets;

fn orbit_reader_csp() -> header::HeaderValue {
    // Foliate: blob iframes + Vue inline styles. EmbedPDF Pdfium: wasm + worker.
    "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' blob: 'unsafe-inline'; img-src 'self' blob: data:; font-src 'self' blob: data:; connect-src 'self'; frame-src blob:; worker-src 'self' blob:; frame-ancestors 'self'; base-uri 'none'; form-action 'none'; object-src 'none'"
        .parse()
        .expect("reader CSP")
}

fn orbit_asset_response(path: &str) -> Option<Response> {
    let file = OrbitReaderAssets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut response = (
        [(header::CONTENT_TYPE, mime.essence_str())],
        file.data.into_owned(),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_SECURITY_POLICY, orbit_reader_csp());
    response
        .headers_mut()
        .insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    Some(response)
}

fn reader_shell_response(headers: &HeaderMap) -> ApiResult<Response> {
    let mut response = orbit_asset_response("index.html").ok_or(ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        "The Book Orbit reader build is missing. Run npm run build in web/orbit-reader.",
    ))?;
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "text/html; charset=utf-8".parse().unwrap(),
    );
    if let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        let secure = if std::env::var("SHELF_SECURE_COOKIE").as_deref() == Ok("true") {
            "; Secure"
        } else {
            ""
        };
        let cookie = format!("shelf_session={token}; HttpOnly; SameSite=Strict; Path=/{secure}");
        response
            .headers_mut()
            .insert(header::SET_COOKIE, cookie.parse().map_err(|_| invalid())?);
    }
    Ok(response)
}

static IMPORTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

fn invalid() -> ApiError {
    ApiError(
        StatusCode::BAD_REQUEST,
        "Not a supported book file. Use an unencrypted EPUB, PDF, or CBZ.",
    )
}

fn is_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF")
}

fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06")
}

fn image_mime(path: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

fn is_comic_image_path(path: &str) -> bool {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    if lower.contains("__macosx/") || lower.split('/').any(|p| p.starts_with('.')) {
        return false;
    }
    image_mime(path).is_some()
}

/// Lexicographic page order (zero-padded names sort correctly).
fn comic_page_names(bytes: &[u8]) -> ApiResult<Vec<String>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| invalid())?;
    if archive.len() > 10_000 {
        return Err(invalid());
    }
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|_| invalid())?;
        if file.is_dir() || file.is_symlink() || file.enclosed_name().is_none() {
            continue;
        }
        if file.size()
            > file
                .compressed_size()
                .saturating_mul(200)
                .saturating_add(1024 * 1024)
        {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "This archive contains an unusually compressed resource.",
            ));
        }
        let name = file.name().replace('\\', "/");
        if is_comic_image_path(&name) {
            names.push(name);
        }
    }
    names.sort();
    if names.is_empty() {
        return Err(invalid());
    }
    Ok(names)
}

fn is_epub_zip(bytes: &[u8]) -> bool {
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return false;
    };
    if archive.by_name("META-INF/container.xml").is_ok() {
        return true;
    }
    if let Ok(mut file) = archive.by_name("mimetype") {
        let mut mime = String::new();
        if std::io::Read::read_to_string(&mut file, &mut mime).is_ok()
            && mime.trim() == "application/epub+zip"
        {
            return true;
        }
    }
    false
}

/// Stem of an upload filename (`Report.pdf` → `Report`). Paths and empty stems are ignored.
fn title_from_filename(name: Option<&str>) -> Option<String> {
    let raw = name?.trim();
    if raw.is_empty() {
        return None;
    }
    let base = std::path::Path::new(raw)
        .file_name()
        .and_then(|s| s.to_str())?
        .trim();
    let stem = std::path::Path::new(base)
        .file_stem()
        .and_then(|s| s.to_str())?
        .trim();
    // Filenames often use underscores where a catalogue title wants spaces.
    let title = stem
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if title.is_empty() || title.len() > 500 || title.chars().any(char::is_control) {
        return None;
    }
    Some(title)
}

/// Best-effort `/Title` from the PDF info dict; empty when absent or binary-encoded.
fn pdf_title(bytes: &[u8]) -> Option<String> {
    let haystack = &bytes[..bytes.len().min(512 * 1024)];
    let marker = b"/Title";
    let mut i = 0;
    while i + marker.len() < haystack.len() {
        if &haystack[i..i + marker.len()] != marker {
            i += 1;
            continue;
        }
        let mut j = i + marker.len();
        while j < haystack.len() && haystack[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= haystack.len() || haystack[j] != b'(' {
            i += 1;
            continue;
        }
        j += 1;
        let mut out = String::new();
        while j < haystack.len() {
            match haystack[j] {
                b')' => {
                    let title = out.trim();
                    if !title.is_empty() && title.len() <= 500 {
                        return Some(title.to_owned());
                    }
                    return None;
                }
                b'\\' if j + 1 < haystack.len() => {
                    out.push(haystack[j + 1] as char);
                    j += 2;
                }
                b if b.is_ascii_control() => return None,
                b => {
                    out.push(b as char);
                    j += 1;
                }
            }
            if out.len() > 500 {
                return None;
            }
        }
        return None;
    }
    None
}

fn valid_page_token(position: &str, prefix: &str) -> bool {
    let Some(inner) = position
        .strip_prefix(prefix)
        .and_then(|s| s.strip_suffix(')'))
    else {
        return false;
    };
    !inner.is_empty()
        && inner.bytes().all(|b| b.is_ascii_digit())
        && inner.parse::<u32>().is_ok_and(|n| n >= 1)
}

fn valid_position(position: &str) -> bool {
    if position.len() > 4096 || position.chars().any(char::is_control) {
        return false;
    }
    if position.starts_with("epubcfi(") && position.ends_with(')') {
        return true;
    }
    // pdfpage(N) / cbzpage(N) — 1-based page indexes for EmbedPDF / comic readers.
    valid_page_token(position, "pdfpage(") || valid_page_token(position, "cbzpage(")
}

fn import_epub(
    tx: &rusqlite::Transaction<'_>,
    user: i64,
    bytes: &[u8],
    digest: &str,
) -> ApiResult<i64> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| invalid())?;
    if archive.len() > 4096 {
        return Err(invalid());
    }
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|_| invalid())?;
        if file.size()
            > file
                .compressed_size()
                .saturating_mul(200)
                .saturating_add(1024 * 1024)
        {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "This EPUB contains an unusually compressed resource.",
            ));
        }
        if file.enclosed_name().is_none() || file.is_symlink() {
            return Err(invalid());
        }
    }
    if archive.by_name("META-INF/encryption.xml").is_ok() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Encrypted EPUB resources are not supported yet.",
        ));
    }
    drop(archive);
    let mut doc = ::epub::doc::EpubDoc::from_reader(Cursor::new(bytes)).map_err(|_| invalid())?;
    let title = doc
        .mdata("title")
        .map(|v| v.value.trim().to_owned())
        .filter(|s| !s.is_empty() && s.len() <= 500)
        .ok_or_else(invalid)?;
    let author = doc
        .mdata("creator")
        .map(|v| v.value.trim().to_owned())
        .unwrap_or_default();
    if author.len() > 300 || doc.spine.is_empty() {
        return Err(invalid());
    }
    let cover = doc.get_cover().and_then(|(data, _)| {
        let mime = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if data.starts_with(b"\xff\xd8\xff") {
            "image/jpeg"
        } else {
            return None;
        };
        Some((data, mime))
    });
    tx.execute(
        "INSERT INTO books(user_id,title,author,total_pages) VALUES (?,?,?,1)",
        params![user, title, author],
    )?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO epub_files(book_id,digest,content,cover,cover_mime,format) VALUES (?,?,?,?,?,'epub')",
        params![
            id,
            digest,
            bytes,
            cover.as_ref().map(|v| &v.0),
            cover.as_ref().map(|v| v.1)
        ],
    )?;
    Ok(id)
}

fn import_pdf(
    tx: &rusqlite::Transaction<'_>,
    user: i64,
    bytes: &[u8],
    digest: &str,
    filename: Option<&str>,
) -> ApiResult<i64> {
    if bytes.len() < 5 || !is_pdf(bytes) {
        return Err(invalid());
    }
    // Reject obvious encrypted PDFs (Encrypt dict in the trailer / early catalog).
    let probe = &bytes[..bytes.len().min(1024 * 1024)];
    if probe.windows(8).any(|w| w == b"/Encrypt") {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Encrypted PDFs are not supported yet.",
        ));
    }
    let title = pdf_title(bytes)
        .or_else(|| title_from_filename(filename))
        .unwrap_or_else(|| "Untitled PDF".into());
    tx.execute(
        "INSERT INTO books(user_id,title,author,total_pages) VALUES (?,?,?,1)",
        params![user, title, ""],
    )?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO epub_files(book_id,digest,content,format) VALUES (?,?,?,'pdf')",
        params![id, digest, bytes],
    )?;
    Ok(id)
}

fn import_cbz(
    tx: &rusqlite::Transaction<'_>,
    user: i64,
    bytes: &[u8],
    digest: &str,
    filename: Option<&str>,
) -> ApiResult<i64> {
    if !is_zip(bytes) || is_epub_zip(bytes) {
        return Err(invalid());
    }
    let pages = comic_page_names(bytes)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| invalid())?;
    let cover_name = pages[0].clone();
    let mut cover_file = archive.by_name(&cover_name).map_err(|_| invalid())?;
    let mut cover = Vec::new();
    std::io::Read::read_to_end(&mut cover_file, &mut cover).map_err(|_| invalid())?;
    let cover_mime = image_mime(&cover_name).ok_or_else(invalid)?;
    drop(cover_file);
    let title = title_from_filename(filename).unwrap_or_else(|| "Untitled comic".into());
    tx.execute(
        "INSERT INTO books(user_id,title,author,total_pages) VALUES (?,?,?,1)",
        params![user, title, ""],
    )?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO epub_files(book_id,digest,content,cover,cover_mime,format) VALUES (?,?,?,?,?,'cbz')",
        params![id, digest, bytes, cover, cover_mime],
    )?;
    Ok(id)
}

pub(super) async fn import(State(state): State<AppState>, request: Request) -> ApiResult<Response> {
    let headers = request.headers().clone();
    let filename = headers
        .get("x-shelf-filename")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let user = state.db(move |db| authenticate(db, &headers)).await?;
    let _permit = IMPORTS.try_acquire().map_err(|_| {
        ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "Another import is running. Try again shortly.",
        )
    })?;
    let bytes = to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|_| {
            ApiError(
                StatusCode::BAD_REQUEST,
                "Could not receive the book upload.",
            )
        })?
        .to_vec();
    state
        .db(move |db| {
            let digest = format!("{:x}", Sha256::digest(&bytes));
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            if let Some(id) = tx
                .query_row(
                    "SELECT b.id FROM books b JOIN epub_files f ON f.book_id=b.id WHERE b.user_id=? AND f.digest=?",
                    params![user, digest],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?
            {
                return Ok((StatusCode::OK, Json(serde_json::json!({"id":id,"duplicate":true}))).into_response());
            }
            let id = if is_pdf(&bytes) {
                import_pdf(&tx, user, &bytes, &digest, filename.as_deref())?
            } else if is_zip(&bytes) && !is_epub_zip(&bytes) {
                import_cbz(&tx, user, &bytes, &digest, filename.as_deref())?
            } else {
                import_epub(&tx, user, &bytes, &digest)?
            };
            tx.commit()?;
            Ok((StatusCode::CREATED, Json(serde_json::json!({"id":id,"duplicate":false}))).into_response())
        })
        .await
}

pub(super) async fn delete_book(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let owned: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM books WHERE id=? AND user_id=?)",
                params![id, user],
                |r| r.get(0),
            )?;
            if !owned {
                return Err(ApiError(StatusCode::NOT_FOUND, "Book not found."));
            }
            tx.execute(
                "DELETE FROM bookmarks WHERE book_id=? AND user_id=?",
                params![id, user],
            )?;
            tx.execute(
                "DELETE FROM annotations WHERE book_id=? AND user_id=?",
                params![id, user],
            )?;
            tx.execute(
                "DELETE FROM reader_preferences WHERE book_id=? AND user_id=?",
                params![id, user],
            )?;
            tx.execute("DELETE FROM epub_files WHERE book_id=?", [id])?;
            tx.execute(
                "DELETE FROM books WHERE id=? AND user_id=?",
                params![id, user],
            )?;
            tx.commit()?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Response> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let (content, format): (Vec<u8>, String) = db
                .query_row(
                    "SELECT f.content,f.format FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=? AND b.user_id=?",
                    params![id, user],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Book file not found."))?;
            let (mime, disposition) = match format.as_str() {
                "pdf" => ("application/pdf", "attachment; filename=book.pdf"),
                "cbz" => (
                    "application/vnd.comicbook+zip",
                    "attachment; filename=book.cbz",
                ),
                _ => ("application/epub+zip", "attachment; filename=book.epub"),
            };
            Ok((
                [
                    (header::CONTENT_TYPE, mime),
                    (header::CONTENT_DISPOSITION, disposition),
                ],
                content,
            )
                .into_response())
        })
        .await
}

pub(super) async fn cover(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Response> {
    state.db(move |db| {
        let user = authenticate(db, &headers)?;
        let (content,mime) = db.query_row("SELECT f.cover,f.cover_mime FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=? AND b.user_id=? AND f.cover IS NOT NULL", params![id,user], |r| Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,String>(1)?))).optional()?.ok_or(ApiError(StatusCode::NOT_FOUND,"Cover not found."))?;
        Ok(([(header::CONTENT_TYPE,mime)],content).into_response())
    }).await
}

/// `GET /books/{id}/pages` → `{ pageCount }` for CBZ (Book Orbit comic reader).
pub(super) async fn comic_page_count(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let (content, format): (Vec<u8>, String) = db
                .query_row(
                    "SELECT f.content,f.format FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=? AND b.user_id=?",
                    params![id, user],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Book file not found."))?;
            if format != "cbz" {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Page images are only available for CBZ comics.",
                ));
            }
            let page_count = comic_page_names(&content)?.len();
            Ok(Json(serde_json::json!({ "pageCount": page_count })))
        })
        .await
}

/// `GET /books/{id}/pages/{page}` — 0-based page image from a CBZ.
pub(super) async fn comic_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, page)): Path<(i64, u32)>,
) -> ApiResult<Response> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let (content, format): (Vec<u8>, String) = db
                .query_row(
                    "SELECT f.content,f.format FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=? AND b.user_id=?",
                    params![id, user],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Book file not found."))?;
            if format != "cbz" {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Page images are only available for CBZ comics.",
                ));
            }
            let names = comic_page_names(&content)?;
            let name = names.get(page as usize).ok_or(ApiError(
                StatusCode::NOT_FOUND,
                "Page not found.",
            ))?;
            let mime = image_mime(name).ok_or_else(invalid)?;
            let mut archive = zip::ZipArchive::new(Cursor::new(content)).map_err(|_| invalid())?;
            let mut file = archive
                .by_name(name)
                .map_err(|_| ApiError(StatusCode::NOT_FOUND, "Page not found."))?;
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut bytes).map_err(|_| invalid())?;
            Ok(([(header::CONTENT_TYPE, mime)], bytes).into_response())
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Location {
    position: String,
    percentage: f64,
}

pub(super) async fn location(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(value): Json<Location>,
) -> ApiResult<StatusCode> {
    if !valid_position(&value.position)
        || !value.percentage.is_finite()
        || !(0.0..=1.0).contains(&value.percentage)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid reading position.",
        ));
    }
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let changed = db.execute(
                "UPDATE epub_files SET position=?,percentage=? WHERE book_id=? AND book_id IN (SELECT id FROM books WHERE user_id=?)",
                params![value.position, value.percentage, id, user],
            )?;
            if changed == 0 {
                return Err(ApiError(StatusCode::NOT_FOUND, "Book file not found."));
            }
            // Natural completion; unfinished/rereading stay until manage marks finished again.
            if (value.percentage - 1.0).abs() < f64::EPSILON {
                db.execute(
                    "UPDATE books SET reading_status='finished' WHERE id=? AND user_id=?",
                    params![id, user],
                )?;
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn reader(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let checked = headers.clone();
    state.db(move |db| authenticate(db, &checked)).await?;
    // Book Orbit's Vue reader (history mode). Bearer on the first WebView request becomes an HttpOnly cookie.
    reader_shell_response(&headers)
}

pub(super) async fn reader_path(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let checked = headers.clone();
    state.db(move |db| authenticate(db, &checked)).await?;
    reader_shell_response(&headers)
}

pub(super) async fn orbit_asset(Path(path): Path<String>) -> Response {
    let trimmed = path.trim_start_matches('/');
    if let Some(response) = orbit_asset_response(trimmed) {
        return response;
    }
    // SPA fallback is only for /reader; assets under /orbit-reader must exist.
    StatusCode::NOT_FOUND.into_response()
}
