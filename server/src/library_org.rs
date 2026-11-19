use super::*;
use serde_json::Value;
use std::path::{Component, Path as FsPath};

const SCAN_FILE_CAP: usize = 500;
const SCAN_EXTENSIONS: &[&str] = &[
    "epub", "pdf", "cbz", "cbr", "cb7", "djvu", "mp3", "m4b", "opus", "ogg", "flac", "aax", "zip",
];

fn owned_collection(db: &Connection, user: i64, id: i64) -> ApiResult<()> {
    let ok: bool = db
        .query_row(
            "SELECT 1 FROM collections WHERE id=? AND user_id=?",
            params![id, user],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, "Collection not found."))
    }
}

fn owned_scope(db: &Connection, user: i64, id: i64) -> ApiResult<()> {
    let ok: bool = db
        .query_row(
            "SELECT 1 FROM smart_scopes WHERE id=? AND user_id=?",
            params![id, user],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, "Scope not found."))
    }
}

fn owned_book(db: &Connection, user: i64, book_id: i64) -> ApiResult<()> {
    let ok: bool = db
        .query_row(
            "SELECT 1 FROM books WHERE id=? AND user_id=?",
            params![book_id, user],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, "Book not found."))
    }
}

fn validate_name(name: &str) -> ApiResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 120 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Enter a name up to 120 characters.",
        ));
    }
    Ok(trimmed.to_string())
}

const FILTER_KEYS: &[&str] = &[
    "author",
    "series",
    "status",
    "format",
    "query",
    "collection_id",
];

fn validate_filter(filter: &Value) -> ApiResult<String> {
    let obj = filter.as_object().ok_or(ApiError(
        StatusCode::BAD_REQUEST,
        "Filter must be a JSON object.",
    ))?;
    for key in obj.keys() {
        if !FILTER_KEYS.contains(&key.as_str()) {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "Filter contains an unknown key.",
            ));
        }
    }
    if let Some(v) = obj.get("collection_id") {
        if !v.is_null() && !v.is_number() {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "collection_id must be a number.",
            ));
        }
    }
    for key in ["author", "series", "status", "format", "query"] {
        if let Some(v) = obj.get(key) {
            if !v.is_null() && !v.is_string() {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Filter string fields must be strings.",
                ));
            }
        }
    }
    serde_json::to_string(filter)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Could not serialize filter."))
}

#[derive(Serialize)]
pub(super) struct AuthorRow {
    name: String,
    book_count: i64,
}

pub(super) async fn list_authors(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<AuthorRow>>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let mut query = db.prepare(
                "SELECT author, count(*) FROM books WHERE user_id=? GROUP BY author ORDER BY author COLLATE NOCASE",
            )?;
            let rows = query
                .query_map([user], |r| {
                    Ok(AuthorRow {
                        name: r.get(0)?,
                        book_count: r.get(1)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Json(rows))
        })
        .await
}

#[derive(Serialize)]
pub(super) struct CollectionRow {
    id: i64,
    name: String,
    book_ids: Vec<i64>,
    book_count: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NewCollection {
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PatchCollection {
    name: Option<String>,
    #[serde(default)]
    add_book_ids: Vec<i64>,
    #[serde(default)]
    remove_book_ids: Vec<i64>,
}

pub(super) async fn list_collections(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<CollectionRow>>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let mut query = db.prepare(
                "SELECT id, name FROM collections WHERE user_id=? ORDER BY name COLLATE NOCASE",
            )?;
            let collections = query
                .query_map([user], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut out = Vec::with_capacity(collections.len());
            for (id, name) in collections {
                let mut books = db.prepare(
                    "SELECT book_id FROM collection_books WHERE collection_id=? ORDER BY book_id",
                )?;
                let book_ids = books
                    .query_map([id], |r| r.get(0))?
                    .collect::<Result<Vec<i64>, _>>()?;
                let book_count = book_ids.len() as i64;
                out.push(CollectionRow {
                    id,
                    name,
                    book_ids,
                    book_count,
                });
            }
            Ok(Json(out))
        })
        .await
}

pub(super) async fn create_collection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(value): Json<NewCollection>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let name = validate_name(&value.name)?;
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            match db.execute(
                "INSERT INTO collections(user_id,name) VALUES (?,?)",
                params![user, name],
            ) {
                Ok(_) => Ok((
                    StatusCode::CREATED,
                    Json(serde_json::json!({"id": db.last_insert_rowid()})),
                )),
                Err(rusqlite::Error::SqliteFailure(code, _))
                    if code.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    Err(ApiError(
                        StatusCode::CONFLICT,
                        "A collection with that name already exists.",
                    ))
                }
                Err(err) => Err(err.into()),
            }
        })
        .await
}

pub(super) async fn patch_collection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(value): Json<PatchCollection>,
) -> ApiResult<StatusCode> {
    let new_name = match value.name {
        Some(ref n) => Some(validate_name(n)?),
        None => None,
    };
    if new_name.is_none() && value.add_book_ids.is_empty() && value.remove_book_ids.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Provide a name and/or book id changes.",
        ));
    }
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_collection(db, user, id)?;
            if let Some(name) = new_name {
                match db.execute(
                    "UPDATE collections SET name=? WHERE id=? AND user_id=?",
                    params![name, id, user],
                ) {
                    Ok(_) => {}
                    Err(rusqlite::Error::SqliteFailure(code, _))
                        if code.code == rusqlite::ErrorCode::ConstraintViolation =>
                    {
                        return Err(ApiError(
                            StatusCode::CONFLICT,
                            "A collection with that name already exists.",
                        ));
                    }
                    Err(err) => return Err(err.into()),
                }
            }
            for book_id in &value.add_book_ids {
                owned_book(db, user, *book_id)?;
                db.execute(
                    "INSERT OR IGNORE INTO collection_books(collection_id,book_id) VALUES (?,?)",
                    params![id, book_id],
                )?;
            }
            for book_id in &value.remove_book_ids {
                db.execute(
                    "DELETE FROM collection_books WHERE collection_id=? AND book_id=?",
                    params![id, book_id],
                )?;
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn delete_collection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let changed = db.execute(
                "DELETE FROM collections WHERE id=? AND user_id=?",
                params![id, user],
            )?;
            if changed == 0 {
                return Err(ApiError(StatusCode::NOT_FOUND, "Collection not found."));
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

#[derive(Serialize)]
pub(super) struct ScopeRow {
    id: i64,
    name: String,
    filter: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NewScope {
    name: String,
    filter: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PatchScope {
    name: Option<String>,
    filter: Option<Value>,
}

pub(super) async fn list_scopes(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<ScopeRow>>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let mut query = db.prepare(
                "SELECT id, name, filter FROM smart_scopes WHERE user_id=? ORDER BY name COLLATE NOCASE",
            )?;
            let rows = query
                .query_map([user], |r| {
                    let raw: String = r.get(2)?;
                    let filter = serde_json::from_str(&raw).unwrap_or(Value::Object(Default::default()));
                    Ok(ScopeRow {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        filter,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Json(rows))
        })
        .await
}

pub(super) async fn create_scope(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(value): Json<NewScope>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let name = validate_name(&value.name)?;
    let filter = validate_filter(&value.filter)?;
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            match db.execute(
                "INSERT INTO smart_scopes(user_id,name,filter) VALUES (?,?,?)",
                params![user, name, filter],
            ) {
                Ok(_) => Ok((
                    StatusCode::CREATED,
                    Json(serde_json::json!({"id": db.last_insert_rowid()})),
                )),
                Err(rusqlite::Error::SqliteFailure(code, _))
                    if code.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    Err(ApiError(
                        StatusCode::CONFLICT,
                        "A scope with that name already exists.",
                    ))
                }
                Err(err) => Err(err.into()),
            }
        })
        .await
}

pub(super) async fn patch_scope(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(value): Json<PatchScope>,
) -> ApiResult<StatusCode> {
    let new_name = match value.name {
        Some(ref n) => Some(validate_name(n)?),
        None => None,
    };
    let new_filter = match value.filter {
        Some(ref f) => Some(validate_filter(f)?),
        None => None,
    };
    if new_name.is_none() && new_filter.is_none() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Provide a name and/or filter.",
        ));
    }
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_scope(db, user, id)?;
            if let Some(name) = new_name {
                match db.execute(
                    "UPDATE smart_scopes SET name=? WHERE id=? AND user_id=?",
                    params![name, id, user],
                ) {
                    Ok(_) => {}
                    Err(rusqlite::Error::SqliteFailure(code, _))
                        if code.code == rusqlite::ErrorCode::ConstraintViolation =>
                    {
                        return Err(ApiError(
                            StatusCode::CONFLICT,
                            "A scope with that name already exists.",
                        ));
                    }
                    Err(err) => return Err(err.into()),
                }
            }
            if let Some(filter) = new_filter {
                db.execute(
                    "UPDATE smart_scopes SET filter=? WHERE id=? AND user_id=?",
                    params![filter, id, user],
                )?;
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn delete_scope(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let changed = db.execute(
                "DELETE FROM smart_scopes WHERE id=? AND user_id=?",
                params![id, user],
            )?;
            if changed == 0 {
                return Err(ApiError(StatusCode::NOT_FOUND, "Scope not found."));
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) fn resolve_library_roots(data_dir: &FsPath) -> Vec<String> {
    if let Ok(raw) = std::env::var("SHELF_LIBRARY_ROOTS") {
        return raw
            .split(':')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
    }
    let books = FsPath::new("/books");
    if books.is_dir() {
        return vec!["/books".to_string()];
    }
    let data_books = data_dir.join("books");
    if data_books.is_dir() {
        return vec![data_books.to_string_lossy().into_owned()];
    }
    Vec::new()
}

pub(super) async fn library_roots(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    state
        .db(move |db| {
            let _user = authenticate(db, &headers)?;
            Ok(())
        })
        .await?;
    Ok(Json(serde_json::json!({
        "roots": resolve_library_roots(&state.data_dir)
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ScanRequest {
    root: Option<String>,
}

fn path_under_root(path: &FsPath, root: &FsPath) -> bool {
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    path.starts_with(&root)
}

fn collect_scan_files(root: &FsPath, cap: usize) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                // Skip symlink escapes: only push real dirs under root.
                if path_under_root(&path, root) {
                    stack.push(path);
                }
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase());
            let Some(ext) = ext else {
                continue;
            };
            if !SCAN_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }
            out.push(path);
            if out.len() >= cap {
                return out;
            }
        }
    }
    out
}

pub(super) async fn library_scan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(value): Json<ScanRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let user = state
        .db({
            let headers = headers.clone();
            move |db| authenticate(db, &headers)
        })
        .await?;
    let allowed = resolve_library_roots(&state.data_dir);
    if allowed.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "No library roots are configured.",
        ));
    }
    let requested = value.root.clone().unwrap_or_else(|| allowed[0].clone());
    let requested_path = FsPath::new(&requested);
    // Reject relative escape components before canonicalize.
    if requested_path
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Scan path is outside the allowed library roots.",
        ));
    }
    let Ok(canon) = requested_path.canonicalize() else {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Scan path could not be resolved.",
        ));
    };
    let allowed_ok = allowed.iter().any(|root| {
        let Ok(root_canon) = FsPath::new(root).canonicalize() else {
            return false;
        };
        canon == root_canon || canon.starts_with(&root_canon)
    });
    if !allowed_ok {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Scan path is outside the allowed library roots.",
        ));
    }
    let files = collect_scan_files(&canon, SCAN_FILE_CAP);
    let data_dir = state.data_dir.clone();
    let mut scanned = 0u64;
    let mut imported = 0u64;
    let mut duplicates = 0u64;
    let mut failed = Vec::new();
    let mut convert_ids = Vec::new();
    for path in files {
        scanned += 1;
        let path_str = path.to_string_lossy().into_owned();
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => {
                failed.push(serde_json::json!({
                    "path": path_str,
                    "error": "Could not read file."
                }));
                continue;
            }
        };
        let filename = path.file_name().and_then(|n| n.to_str()).map(str::to_owned);
        let data_dir = data_dir.clone();
        match state
            .db(move |db| epub::ingest_bytes(db, user, &bytes, filename.as_deref(), &data_dir))
            .await
        {
            Ok(result) => {
                if result.duplicate {
                    duplicates += 1;
                } else {
                    imported += 1;
                    if let Some(id) = result.convert_id {
                        convert_ids.push(id);
                    }
                }
            }
            Err(ApiError(_, msg)) => {
                failed.push(serde_json::json!({
                    "path": path_str,
                    "error": msg
                }));
            }
        }
    }
    for id in convert_ids {
        aax::spawn_convert(state.clone(), id);
    }
    if imported > 0 {
        crate::metadata::kick(state.clone());
    }
    Ok(Json(serde_json::json!({
        "scanned": scanned,
        "imported": imported,
        "duplicates": duplicates,
        "failed": failed,
    })))
}
