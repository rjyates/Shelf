//! M12 library-health charts: format / language / storage / metadata completeness.

use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

const HIDE_WORK_EDITIONS: &str = "b.id NOT IN (
  SELECT audio_book_id FROM book_works
  UNION
  SELECT readaloud_book_id FROM book_works WHERE readaloud_book_id IS NOT NULL
)";

fn file_bytes(data_dir: &Path, content_len: i64, storage_path: Option<String>) -> i64 {
    if let Some(rel) = storage_path.filter(|p| !p.is_empty()) {
        if let Ok(meta) = std::fs::metadata(data_dir.join(rel)) {
            return meta.len() as i64;
        }
        return 0;
    }
    content_len.max(0)
}

/// `GET /api/v1/insights/library` — catalogue health charts for Insights.
pub(super) async fn library_insights(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let data_dir = state.data_dir.clone();
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;

            let mut format_counts: BTreeMap<String, i64> = BTreeMap::new();
            let mut storage_by_format: BTreeMap<String, i64> = BTreeMap::new();
            {
                let sql = format!(
                    "SELECT coalesce(f.format, 'unknown'),
                            coalesce(length(f.content), 0),
                            f.storage_path
                     FROM books b
                     LEFT JOIN epub_files f ON f.book_id = b.id
                     WHERE b.user_id=? AND {HIDE_WORK_EDITIONS}"
                );
                let mut stmt = db.prepare(&sql)?;
                let rows = stmt.query_map(params![user], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, Option<String>>(2)?,
                    ))
                })?;
                for row in rows {
                    let (format, content_len, storage_path) = row?;
                    *format_counts.entry(format.clone()).or_insert(0) += 1;
                    let bytes = file_bytes(&data_dir, content_len, storage_path);
                    *storage_by_format.entry(format).or_insert(0) += bytes;
                }
            }

            let mut language_counts: BTreeMap<String, i64> = BTreeMap::new();
            {
                let sql = format!(
                    "SELECT coalesce(nullif(trim(m.language), ''), 'unknown')
                     FROM books b
                     LEFT JOIN book_metadata m ON m.book_id = b.id
                     WHERE b.user_id=? AND {HIDE_WORK_EDITIONS}"
                );
                let mut stmt = db.prepare(&sql)?;
                for lang in stmt.query_map(params![user], |r| r.get::<_, String>(0))? {
                    *language_counts.entry(lang?).or_insert(0) += 1;
                }
            }

            let mut status_counts: BTreeMap<String, i64> = BTreeMap::new();
            {
                let sql = format!(
                    "SELECT coalesce(b.reading_status, 'unread')
                     FROM books b
                     WHERE b.user_id=? AND {HIDE_WORK_EDITIONS}"
                );
                let mut stmt = db.prepare(&sql)?;
                for status in stmt.query_map(params![user], |r| r.get::<_, String>(0))? {
                    *status_counts.entry(status?).or_insert(0) += 1;
                }
            }

            let completeness: (i64, i64, i64, i64, i64, i64) = {
                let sql = format!(
                    "SELECT count(*),
                            coalesce(sum(CASE WHEN m.language IS NOT NULL AND trim(m.language) != '' THEN 1 ELSE 0 END),0),
                            coalesce(sum(CASE WHEN m.isbn13 IS NOT NULL OR m.isbn10 IS NOT NULL THEN 1 ELSE 0 END),0),
                            coalesce(sum(CASE WHEN m.description IS NOT NULL AND trim(m.description) != '' THEN 1 ELSE 0 END),0),
                            coalesce(sum(CASE WHEN f.cover IS NOT NULL OR m.cover_source IS NOT NULL THEN 1 ELSE 0 END),0),
                            coalesce(sum(CASE WHEN m.metadata_score >= 50 THEN 1 ELSE 0 END),0)
                     FROM books b
                     LEFT JOIN book_metadata m ON m.book_id = b.id
                     LEFT JOIN epub_files f ON f.book_id = b.id
                     WHERE b.user_id=? AND {HIDE_WORK_EDITIONS}"
                );
                db.query_row(&sql, params![user], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                })?
            };

            let format_slices: Vec<Value> = format_counts
                .into_iter()
                .map(|(format, count)| json!({ "key": format, "count": count }))
                .collect();
            let language_slices: Vec<Value> = language_counts
                .into_iter()
                .map(|(language, count)| json!({ "key": language, "count": count }))
                .collect();
            let storage_slices: Vec<Value> = storage_by_format
                .into_iter()
                .map(|(format, bytes)| json!({ "key": format, "bytes": bytes }))
                .collect();
            let status_slices: Vec<Value> = status_counts
                .into_iter()
                .map(|(status, count)| json!({ "key": status, "count": count }))
                .collect();

            let total = completeness.0;
            Ok(Json(json!({
                "formatDistribution": { "slices": format_slices, "total": total },
                "languageDistribution": { "slices": language_slices, "total": total },
                "storageByFormat": { "slices": storage_slices },
                "readingStatus": { "slices": status_slices, "total": total },
                "metadataCompleteness": {
                    "total": total,
                    "withLanguage": completeness.1,
                    "withIsbn": completeness.2,
                    "withDescription": completeness.3,
                    "withCover": completeness.4,
                    "scoreAtLeast50": completeness.5,
                }
            })))
        })
        .await
}
