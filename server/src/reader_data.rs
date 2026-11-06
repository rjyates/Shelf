use super::*;
use axum::extract::Query;
use serde_json::{json, Map, Value};

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

fn parse_settings(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| json!({}))
}

fn merge_object(base: Value, patch: Value) -> Value {
    match (base, patch) {
        (Value::Object(mut left), Value::Object(right)) => {
            for (key, value) in right {
                let next = match left.remove(&key) {
                    Some(existing) => merge_object(existing, value),
                    None => value,
                };
                left.insert(key, next);
            }
            Value::Object(left)
        }
        (_, patch) => patch,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NewBookmark {
    cfi: Option<String>,
    title: Option<String>,
    chapter_title: Option<String>,
    note: Option<String>,
    #[serde(default)]
    book_file_id: Option<i64>,
}

fn null_as_clear<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<String>>, D::Error> {
    // Distinguishes omitted vs JSON null so PATCH can clear notes.
    Ok(Some(Option::<String>::deserialize(d)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BookmarkPatch {
    title: Option<String>,
    #[serde(default, deserialize_with = "null_as_clear")]
    note: Option<Option<String>>,
}

#[derive(Deserialize, Serialize, Clone)]
struct AnnotationRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Deserialize, Serialize, Clone)]
struct AnnotationPdfPosition {
    page: i64,
    rect: AnnotationRect,
    rects: Vec<AnnotationRect>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NewAnnotation {
    cfi: Option<String>,
    text: Option<String>,
    color: Option<String>,
    style: Option<String>,
    note: Option<String>,
    chapter_title: Option<String>,
    #[serde(default)]
    book_file_id: Option<i64>,
    /// EmbedPDF highlight geometry; required for PDF reader highlights to round-trip.
    pdf: Option<AnnotationPdfPosition>,
}

fn finite_rect(rect: &AnnotationRect) -> bool {
    [rect.x, rect.y, rect.width, rect.height]
        .into_iter()
        .all(f64::is_finite)
}

fn validate_pdf(pdf: &AnnotationPdfPosition) -> ApiResult<()> {
    if !(0..1_000_000).contains(&pdf.page)
        || pdf.rects.is_empty()
        || pdf.rects.len() > 512
        || !finite_rect(&pdf.rect)
        || pdf.rects.iter().any(|rect| !finite_rect(rect))
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid PDF annotation position.",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AnnotationPatch {
    #[serde(default, deserialize_with = "null_as_clear")]
    note: Option<Option<String>>,
    color: Option<String>,
    style: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct PageQuery {
    page: Option<i64>,
    #[serde(rename = "pageSize")]
    page_size: Option<i64>,
    #[serde(rename = "readerFileId")]
    reader_file_id: Option<i64>,
}

pub(super) async fn list_bookmarks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(book_id): Path<i64>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, book_id)?;
            let mut stmt = db.prepare(
                "SELECT id,book_id,cfi,title,note,created_at FROM bookmarks WHERE user_id=? AND book_id=? ORDER BY id",
            )?;
            let rows = stmt
                .query_map(params![user, book_id], |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "bookId": r.get::<_, i64>(1)?,
                        "cfi": r.get::<_, Option<String>>(2)?,
                        "title": r.get::<_, String>(3)?,
                        "note": r.get::<_, Option<String>>(4)?,
                        "createdAt": r.get::<_, String>(5)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Json(Value::Array(rows)))
        })
        .await
}

pub(super) async fn create_bookmark(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(book_id): Path<i64>,
    Json(body): Json<NewBookmark>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let title = body
        .title
        .or(body.chapter_title)
        .unwrap_or_else(|| "Bookmark".into());
    if title.trim().is_empty() || title.len() > 500 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid bookmark title."));
    }
    if body
        .cfi
        .as_ref()
        .is_some_and(|c| c.len() > 4096 || c.chars().any(char::is_control))
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid bookmark position.",
        ));
    }
    let _ = body.book_file_id;
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, book_id)?;
            db.execute(
                "INSERT INTO bookmarks(user_id,book_id,cfi,title,note) VALUES (?,?,?,?,?)",
                params![user, book_id, body.cfi, title.trim(), body.note],
            )?;
            let id = db.last_insert_rowid();
            let row = db.query_row(
                "SELECT id,book_id,cfi,title,note,created_at FROM bookmarks WHERE id=? AND user_id=?",
                params![id, user],
                |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "bookId": r.get::<_, i64>(1)?,
                        "cfi": r.get::<_, Option<String>>(2)?,
                        "title": r.get::<_, String>(3)?,
                        "note": r.get::<_, Option<String>>(4)?,
                        "createdAt": r.get::<_, String>(5)?,
                    }))
                },
            )?;
            Ok((StatusCode::CREATED, Json(row)))
        })
        .await
}

pub(super) async fn patch_bookmark(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((book_id, mark_id)): Path<(i64, i64)>,
    Json(body): Json<BookmarkPatch>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, book_id)?;
            let (mut title, mut note): (String, Option<String>) = db
                .query_row(
                    "SELECT title,note FROM bookmarks WHERE id=? AND user_id=? AND book_id=?",
                    params![mark_id, user, book_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Bookmark not found."))?;
            if let Some(next) = body.title {
                if next.trim().is_empty() || next.len() > 500 {
                    return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid bookmark title."));
                }
                title = next.trim().into();
            }
            if let Some(next) = body.note {
                note = next;
            }
            db.execute(
                "UPDATE bookmarks SET title=?, note=? WHERE id=? AND user_id=? AND book_id=?",
                params![title, note, mark_id, user, book_id],
            )?;
            let row = db.query_row(
                "SELECT id,book_id,cfi,title,note,created_at FROM bookmarks WHERE id=? AND user_id=?",
                params![mark_id, user],
                |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "bookId": r.get::<_, i64>(1)?,
                        "cfi": r.get::<_, Option<String>>(2)?,
                        "title": r.get::<_, String>(3)?,
                        "note": r.get::<_, Option<String>>(4)?,
                        "createdAt": r.get::<_, String>(5)?,
                    }))
                },
            )?;
            Ok(Json(row))
        })
        .await
}

pub(super) async fn delete_bookmark(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((book_id, mark_id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let changed = db.execute(
                "DELETE FROM bookmarks WHERE id=? AND user_id=? AND book_id=?",
                params![mark_id, user, book_id],
            )?;
            if changed == 0 {
                return Err(ApiError(StatusCode::NOT_FOUND, "Bookmark not found."));
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

#[allow(clippy::too_many_arguments)]
fn annotation_json(
    id: i64,
    book_id: i64,
    cfi: Option<String>,
    text: String,
    color: String,
    style: String,
    note: Option<String>,
    chapter_title: Option<String>,
    created_at: String,
    pdf_raw: Option<String>,
) -> Value {
    let pdf: Option<AnnotationPdfPosition> = pdf_raw
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok());
    let pageno = pdf.as_ref().map(|position| position.page);
    // Foliate/PDF readers expect this BO-shaped object; keep fields flat rather than a row type.
    json!({
        "id": id,
        "bookId": book_id,
        "cfi": cfi,
        "jumpFileId": book_id,
        "pageno": pageno,
        "text": text,
        "color": color,
        "style": style,
        "note": note,
        "chapterTitle": chapter_title,
        "origin": "web",
        "positionStatus": null,
        "chapterIndex": null,
        "highlightedAt": created_at,
        "createdAt": created_at,
        "updatedAt": null,
        "pdf": pdf,
    })
}

fn annotation_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(annotation_json(
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
    ))
}

pub(super) async fn list_annotations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(book_id): Path<i64>,
    Query(query): Query<PageQuery>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, book_id)?;
            let _ = query.reader_file_id;
            let mut stmt = db.prepare(
                "SELECT id,book_id,cfi,text,color,style,note,chapter_title,created_at,pdf FROM annotations WHERE user_id=? AND book_id=? ORDER BY id",
            )?;
            let rows = stmt
                .query_map(params![user, book_id], annotation_row)?
                .collect::<Result<Vec<_>, _>>()?;
            if query.page.is_some() || query.reader_file_id.is_some() {
                let total = rows.len();
                return Ok(Json(json!({
                    "items": rows,
                    "page": query.page.unwrap_or(1).max(1),
                    "pageSize": query.page_size.unwrap_or(total as i64).max(1),
                    "total": total,
                })));
            }
            Ok(Json(Value::Array(rows)))
        })
        .await
}

pub(super) async fn create_annotation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(book_id): Path<i64>,
    Json(body): Json<NewAnnotation>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let text = body.text.unwrap_or_default();
    let color = body.color.unwrap_or_else(|| "#FACC15".into());
    let style = body.style.unwrap_or_else(|| "highlight".into());
    if text.len() > 16_384 || color.len() > 64 || style.len() > 64 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid annotation."));
    }
    if body
        .cfi
        .as_ref()
        .is_some_and(|c| c.len() > 4096 || c.chars().any(char::is_control))
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid annotation position.",
        ));
    }
    if let Some(pdf) = &body.pdf {
        validate_pdf(pdf)?;
    }
    let _ = body.book_file_id;
    let pdf_json = body
        .pdf
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Invalid PDF annotation position."))?;
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, book_id)?;
            db.execute(
                "INSERT INTO annotations(user_id,book_id,cfi,text,color,style,note,chapter_title,pdf) VALUES (?,?,?,?,?,?,?,?,?)",
                params![
                    user,
                    book_id,
                    body.cfi,
                    text,
                    color,
                    style,
                    body.note,
                    body.chapter_title,
                    pdf_json
                ],
            )?;
            let id = db.last_insert_rowid();
            let row = db.query_row(
                "SELECT id,book_id,cfi,text,color,style,note,chapter_title,created_at,pdf FROM annotations WHERE id=? AND user_id=?",
                params![id, user],
                annotation_row,
            )?;
            Ok((StatusCode::CREATED, Json(row)))
        })
        .await
}

pub(super) async fn patch_annotation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((book_id, annotation_id)): Path<(i64, i64)>,
    Json(body): Json<AnnotationPatch>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, book_id)?;
            let (mut note, mut color, mut style): (Option<String>, String, String) = db
                .query_row(
                    "SELECT note,color,style FROM annotations WHERE id=? AND user_id=? AND book_id=?",
                    params![annotation_id, user, book_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Annotation not found."))?;
            if let Some(next) = body.note {
                note = next;
            }
            if let Some(next) = body.color {
                if next.is_empty() || next.len() > 64 {
                    return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid annotation."));
                }
                color = next;
            }
            if let Some(next) = body.style {
                if next.is_empty() || next.len() > 64 {
                    return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid annotation."));
                }
                style = next;
            }
            db.execute(
                "UPDATE annotations SET note=?, color=?, style=? WHERE id=? AND user_id=? AND book_id=?",
                params![note, color, style, annotation_id, user, book_id],
            )?;
            let row = db.query_row(
                "SELECT id,book_id,cfi,text,color,style,note,chapter_title,created_at,pdf FROM annotations WHERE id=? AND user_id=?",
                params![annotation_id, user],
                annotation_row,
            )?;
            Ok(Json(row))
        })
        .await
}

pub(super) async fn delete_annotation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((book_id, annotation_id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let changed = db.execute(
                "DELETE FROM annotations WHERE id=? AND user_id=? AND book_id=?",
                params![annotation_id, user, book_id],
            )?;
            if changed == 0 {
                return Err(ApiError(StatusCode::NOT_FOUND, "Annotation not found."));
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn get_preferences(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(book_id): Path<i64>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, book_id)?;
            let settings: Option<String> = db
                .query_row(
                    "SELECT settings FROM reader_preferences WHERE user_id=? AND book_id=?",
                    params![user, book_id],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(Json(
                settings.map(|s| parse_settings(&s)).unwrap_or(Value::Null),
            ))
        })
        .await
}

pub(super) async fn put_preferences(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(book_id): Path<i64>,
    Json(patch): Json<Value>,
) -> ApiResult<Json<Value>> {
    if !patch.is_object() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Preferences must be an object.",
        ));
    }
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            owned_book(db, user, book_id)?;
            let current: Value = db
                .query_row(
                    "SELECT settings FROM reader_preferences WHERE user_id=? AND book_id=?",
                    params![user, book_id],
                    |r| Ok(parse_settings(&r.get::<_, String>(0)?)),
                )
                .optional()?
                .unwrap_or_else(|| json!({}));
            let merged = merge_object(current, patch);
            let encoded = merged.to_string();
            if encoded.len() > 64 * 1024 {
                return Err(ApiError(StatusCode::BAD_REQUEST, "Preferences are too large."));
            }
            db.execute(
                "INSERT INTO reader_preferences(user_id,book_id,settings) VALUES (?,?,?) ON CONFLICT(user_id,book_id) DO UPDATE SET settings=excluded.settings",
                params![user, book_id, encoded],
            )?;
            Ok(Json(merged))
        })
        .await
}

pub(super) async fn delete_preferences(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(book_id): Path<i64>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            db.execute(
                "DELETE FROM reader_preferences WHERE user_id=? AND book_id=?",
                params![user, book_id],
            )?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

fn load_defaults(db: &Connection, user: i64) -> ApiResult<Map<String, Value>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT settings FROM reader_defaults WHERE user_id=?",
            [user],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match raw.map(|s| parse_settings(&s)).unwrap_or(json!({})) {
        Value::Object(map) => map,
        _ => Map::new(),
    })
}

fn save_defaults(db: &Connection, user: i64, settings: &Map<String, Value>) -> ApiResult<()> {
    let encoded = Value::Object(settings.clone()).to_string();
    if encoded.len() > 64 * 1024 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Defaults are too large."));
    }
    db.execute(
        "INSERT INTO reader_defaults(user_id,settings) VALUES (?,?) ON CONFLICT(user_id) DO UPDATE SET settings=excluded.settings",
        params![user, encoded],
    )?;
    Ok(())
}

pub(super) async fn get_defaults(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            Ok(Json(Value::Object(load_defaults(db, user)?)))
        })
        .await
}

pub(super) async fn put_defaults(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(patch): Json<Value>,
) -> ApiResult<Json<Value>> {
    if !patch.is_object() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Defaults must be an object.",
        ));
    }
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let current = Value::Object(load_defaults(db, user)?);
            let merged = merge_object(current, patch);
            let Value::Object(map) = &merged else {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Defaults must be an object.",
                ));
            };
            save_defaults(db, user, map)?;
            Ok(Json(merged))
        })
        .await
}

pub(super) async fn delete_defaults(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            db.execute("DELETE FROM reader_defaults WHERE user_id=?", [user])?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn get_default_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group): Path<String>,
) -> ApiResult<Json<Value>> {
    if group.is_empty() || group.len() > 64 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid defaults group."));
    }
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let defaults = load_defaults(db, user)?;
            Ok(Json(defaults.get(&group).cloned().unwrap_or(json!({}))))
        })
        .await
}

pub(super) async fn put_default_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group): Path<String>,
    Json(patch): Json<Value>,
) -> ApiResult<Json<Value>> {
    if group.is_empty() || group.len() > 64 || !patch.is_object() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid defaults group."));
    }
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let mut defaults = load_defaults(db, user)?;
            let current = defaults.remove(&group).unwrap_or_else(|| json!({}));
            let merged = merge_object(current, patch);
            defaults.insert(group, merged.clone());
            save_defaults(db, user, &defaults)?;
            Ok(Json(merged))
        })
        .await
}

pub(super) async fn delete_default_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group): Path<String>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let mut defaults = load_defaults(db, user)?;
            defaults.remove(&group);
            if defaults.is_empty() {
                db.execute("DELETE FROM reader_defaults WHERE user_id=?", [user])?;
            } else {
                save_defaults(db, user, &defaults)?;
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}
