use super::*;
use axum::{body::to_bytes, extract::Request};
use std::io::Cursor;

static IMPORTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

fn invalid() -> ApiError {
    ApiError(
        StatusCode::BAD_REQUEST,
        "Not a supported EPUB. Use an unencrypted EPUB.",
    )
}

pub(super) async fn import(State(state): State<AppState>, request: Request) -> ApiResult<Response> {
    let headers = request.headers().clone();
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
                "Could not receive the EPUB upload.",
            )
        })?
        .to_vec();
    state.db(move |db| {
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some(id) = tx.query_row("SELECT b.id FROM books b JOIN epub_files f ON f.book_id=b.id WHERE b.user_id=? AND f.digest=?", params![user,digest], |r| r.get::<_,i64>(0)).optional()? {
            return Ok((StatusCode::OK, Json(serde_json::json!({"id":id,"duplicate":true}))).into_response());
        }
        // Bound decompression before handing the archive to the EPUB parser. Never extract paths.
        let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).map_err(|_| invalid())?;
        if archive.len() > 4096 { return Err(invalid()); }
        for index in 0..archive.len() {
            let file = archive.by_index(index).map_err(|_| invalid())?;
            // Reject extreme compression ratios without limiting the size of ordinary books.
            if file.size() > file.compressed_size().saturating_mul(200).saturating_add(1024 * 1024) {
                return Err(ApiError(StatusCode::BAD_REQUEST, "This EPUB contains an unusually compressed resource."));
            }
            if file.enclosed_name().is_none() || file.is_symlink() { return Err(invalid()); }
        }
        if archive.by_name("META-INF/encryption.xml").is_ok() { return Err(ApiError(StatusCode::BAD_REQUEST, "Encrypted EPUB resources are not supported yet.")); }
        drop(archive);
        let mut doc = ::epub::doc::EpubDoc::from_reader(Cursor::new(&bytes)).map_err(|_| invalid())?;
        let title = doc.mdata("title").map(|v| v.value.trim().to_owned()).filter(|s| !s.is_empty() && s.len() <= 500).ok_or_else(invalid)?;
        let author = doc.mdata("creator").map(|v| v.value.trim().to_owned()).unwrap_or_default();
        if author.len() > 300 || doc.spine.is_empty() { return Err(invalid()); }
        let cover = doc.get_cover().and_then(|(data, _)| {
            let mime = if data.starts_with(b"\x89PNG\r\n\x1a\n") { "image/png" } else if data.starts_with(b"\xff\xd8\xff") { "image/jpeg" } else { return None; };
            Some((data, mime))
        });
        // Legacy manual entries retain their page constraint; EPUBs expose null and use a separate location.
        tx.execute("INSERT INTO books(user_id,title,author,total_pages) VALUES (?,?,?,1)", params![user,title,author])?;
        let id = tx.last_insert_rowid();
        tx.execute("INSERT INTO epub_files(book_id,digest,content,cover,cover_mime) VALUES (?,?,?,?,?)", params![id,digest,bytes,cover.as_ref().map(|v| &v.0),cover.as_ref().map(|v| v.1)])?;
        tx.commit()?;
        Ok((StatusCode::CREATED, Json(serde_json::json!({"id":id,"duplicate":false}))).into_response())
    }).await
}

pub(super) async fn file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Response> {
    state.db(move |db| {
        let user = authenticate(db, &headers)?;
        let content = db.query_row("SELECT f.content FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=? AND b.user_id=?", params![id,user], |r| r.get::<_,Vec<u8>>(0)).optional()?.ok_or(ApiError(StatusCode::NOT_FOUND,"EPUB not found."))?;
        Ok(([(header::CONTENT_TYPE,"application/epub+zip"),(header::CONTENT_DISPOSITION,"attachment; filename=book.epub")],content).into_response())
    }).await
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
    if value.position.len() > 4096
        || !value.position.starts_with("epubcfi(")
        || !value.position.ends_with(')')
        || value.position.chars().any(char::is_control)
        || !value.percentage.is_finite()
        || !(0.0..=1.0).contains(&value.percentage)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid reading position.",
        ));
    }
    state.db(move |db| {
        let user=authenticate(db,&headers)?;
        let changed=db.execute("UPDATE epub_files SET position=?,percentage=? WHERE book_id=? AND book_id IN (SELECT id FROM books WHERE user_id=?)",params![value.position,value.percentage,id,user])?;
        if changed==0 { return Err(ApiError(StatusCode::NOT_FOUND,"EPUB not found.")); }
        Ok(StatusCode::NO_CONTENT)
    }).await
}

pub(super) async fn reader(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let checked = headers.clone();
    state.db(move |db| authenticate(db, &checked)).await?;
    let mut response = Html(include_str!("../../web/reader.html")).into_response();
    // The native WebView sends a bearer only on its initial request. Exchange it for an HttpOnly cookie.
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
    response.headers_mut().insert(header::CONTENT_SECURITY_POLICY,"default-src 'none'; script-src 'self'; style-src 'self' blob: 'unsafe-inline'; img-src 'self' blob: data:; font-src 'self' blob: data:; connect-src 'self'; frame-src blob:; frame-ancestors 'self'; base-uri 'none'; form-action 'none'; object-src 'none'".parse().unwrap());
    response
        .headers_mut()
        .insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    Ok(response)
}
