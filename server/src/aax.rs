//! AAX → M4B conversion (audible-tools pipeline): checksum @653, rcrack, ffmpeg -c copy.

use super::*;
use std::process::Command;

/// Audible Tools / Dropzone reads the ADRM file checksum at this offset.
const AAX_CHECKSUM_OFFSET: usize = 653;
const AAX_CHECKSUM_LEN: usize = 20;

pub(crate) fn is_aax(bytes: &[u8], filename: Option<&str>) -> bool {
    let ext = filename
        .and_then(|n| std::path::Path::new(n).extension())
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    if ext.as_deref() != Some("aax") {
        return false;
    }
    // AAX is an encrypted MP4; brand is often `aavd`.
    bytes.len() >= 12 && &bytes[4..8] == b"ftyp"
}

pub(crate) fn file_checksum(bytes: &[u8]) -> ApiResult<String> {
    if bytes.len() < AAX_CHECKSUM_OFFSET + AAX_CHECKSUM_LEN {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "This AAX file is too small to read its checksum.",
        ));
    }
    Ok(
        bytes[AAX_CHECKSUM_OFFSET..AAX_CHECKSUM_OFFSET + AAX_CHECKSUM_LEN]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

pub(crate) fn parse_rcrack_hex(output: &str) -> Option<String> {
    for token in output.split_whitespace() {
        if let Some(hex) = token.strip_prefix("hex:") {
            let hex = hex.trim();
            if hex.len() == 8 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(hex.to_ascii_lowercase());
            }
        }
    }
    // Also accept "hex:XXXXXXXX" glued to other text on a line.
    for line in output.lines() {
        if let Some(idx) = line.find("hex:") {
            let rest = &line[idx + 4..];
            let hex: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
            if hex.len() == 8 {
                return Some(hex.to_ascii_lowercase());
            }
        }
    }
    None
}

fn tables_dir() -> Option<PathBuf> {
    std::env::var_os("SHELF_AAX_TABLES").map(PathBuf::from)
}

fn rcrack_bin() -> PathBuf {
    std::env::var_os("SHELF_AAX_RCRACK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rcrack"))
}

fn ffmpeg_bin() -> PathBuf {
    std::env::var_os("SHELF_AAX_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

fn resolve_activation_bytes(db: &Connection, checksum: &str) -> ApiResult<String> {
    if let Some(cached) = db
        .query_row(
            "SELECT activation_bytes FROM aax_activation_cache WHERE checksum=?",
            [checksum],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(cached);
    }
    let Some(tables) = tables_dir() else {
        return Err(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "AAX conversion needs SHELF_AAX_TABLES (inAudible-NG rainbow tables).",
        ));
    };
    if !tables.is_dir() {
        return Err(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "SHELF_AAX_TABLES is not a directory.",
        ));
    }
    let output = Command::new(rcrack_bin())
        .current_dir(&tables)
        .args([".", "-h", checksum])
        .output()
        .map_err(|_| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not run rcrack. Install inAudible-NG tables and ensure rcrack is on PATH (or set SHELF_AAX_RCRACK).",
            )
        })?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let Some(bytes) = parse_rcrack_hex(&combined) else {
        return Err(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rcrack did not return activation bytes for this AAX checksum.",
        ));
    };
    let _ = db.execute(
        "INSERT OR REPLACE INTO aax_activation_cache(checksum,activation_bytes) VALUES (?,?)",
        params![checksum, bytes],
    );
    Ok(bytes)
}

fn set_conversion(
    db: &Connection,
    book_id: i64,
    status: &str,
    error: Option<&str>,
) -> rusqlite::Result<()> {
    db.execute(
        "UPDATE epub_files SET conversion_status=?, conversion_error=? WHERE book_id=?",
        params![status, error, book_id],
    )?;
    Ok(())
}

fn convert_one(
    database: &std::path::Path,
    data_dir: &std::path::Path,
    book_id: i64,
) -> ApiResult<()> {
    let mut db = Connection::open(database)?;
    configure_connection(&db)?;
    set_conversion(&db, book_id, "running", None)?;

    let (source, source_path): (Vec<u8>, Option<String>) = db
        .query_row(
            "SELECT COALESCE(source, X''), source_path FROM epub_files WHERE book_id=? AND format='aax'",
            [book_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(ApiError(
            StatusCode::NOT_FOUND,
            "AAX book not found for conversion.",
        ))?;
    let aax_bytes = load_book_bytes(data_dir, source, source_path)?;
    let checksum = match file_checksum(&aax_bytes) {
        Ok(c) => c,
        Err(e) => {
            let _ = set_conversion(&db, book_id, "failed", Some(e.1));
            return Err(e);
        }
    };
    let activation = match resolve_activation_bytes(&db, &checksum) {
        Ok(a) => a,
        Err(e) => {
            let _ = set_conversion(&db, book_id, "failed", Some(e.1));
            return Err(e);
        }
    };

    let work = data_dir.join("files");
    std::fs::create_dir_all(&work).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not create conversion workspace.",
        )
    })?;
    let in_path = work.join(format!("aax-{book_id}.aax"));
    let out_path = work.join(format!("aax-{book_id}.m4b"));
    std::fs::write(&in_path, &aax_bytes).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not write temporary AAX for conversion.",
        )
    })?;
    let ffmpeg = Command::new(ffmpeg_bin())
        .args([
            "-y",
            "-activation_bytes",
            &activation,
            "-i",
            in_path.to_str().unwrap_or(""),
            "-c",
            "copy",
            out_path.to_str().unwrap_or(""),
        ])
        .output();
    let _ = std::fs::remove_file(&in_path);
    let output = match ffmpeg {
        Ok(o) if o.status.success() && out_path.is_file() => o,
        Ok(o) => {
            let _ = std::fs::remove_file(&out_path);
            let detail = String::from_utf8_lossy(&o.stderr);
            let msg = if detail.to_ascii_lowercase().contains("activation") {
                "ffmpeg rejected the activation bytes for this AAX."
            } else {
                "ffmpeg failed to convert this AAX to M4B."
            };
            let _ = set_conversion(&db, book_id, "failed", Some(msg));
            return Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, msg));
        }
        Err(_) => {
            let _ = set_conversion(
                &db,
                book_id,
                "failed",
                Some("ffmpeg is not available. Install ffmpeg or set SHELF_AAX_FFMPEG."),
            );
            return Err(ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ffmpeg is not available. Install ffmpeg or set SHELF_AAX_FFMPEG.",
            ));
        }
    };
    let _ = output;
    let m4b = std::fs::read(&out_path).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read converted M4B.",
        )
    })?;
    let _ = std::fs::remove_file(&out_path);
    let digest = format!("{:x}", Sha256::digest(&m4b));
    let (content, storage_path) = store_book_bytes(data_dir, &digest, "m4b", &m4b)?;
    let tx = db.transaction()?;
    // Drop prior empty/placeholder content storage if any.
    let old_storage: Option<String> = tx.query_row(
        "SELECT storage_path FROM epub_files WHERE book_id=?",
        [book_id],
        |r| r.get(0),
    )?;
    tx.execute(
        "UPDATE epub_files SET content=?, storage_path=?, digest=?, format='m4b',
         conversion_status='ready', conversion_error=NULL, chapter_markers=NULL WHERE book_id=?",
        params![content, storage_path, digest, book_id],
    )?;
    tx.commit()?;
    if old_storage.as_deref() != storage_path.as_deref() {
        remove_book_file(data_dir, old_storage.as_deref());
    }
    let _ = audio_meta::enrich_audio_book(&db, data_dir, book_id);
    Ok(())
}

pub(crate) fn spawn_convert(state: AppState, book_id: i64) {
    tokio::spawn(async move {
        let database = state.database.clone();
        let data_dir = state.data_dir.clone();
        let _ =
            tokio::task::spawn_blocking(move || convert_one(&database, &data_dir, book_id)).await;
    });
}

/// Re-queue AAX books left pending/running after a restart.
pub(crate) fn resume_pending(state: &AppState) {
    let database = state.database.clone();
    let Ok(db) = Connection::open(&database) else {
        return;
    };
    let Ok(()) = configure_connection(&db) else {
        return;
    };
    let Ok(mut stmt) = db.prepare(
        "SELECT book_id FROM epub_files WHERE format='aax' AND conversion_status IN ('pending','running')",
    ) else {
        return;
    };
    let Ok(ids) = stmt
        .query_map([], |r| r.get::<_, i64>(0))
        .map(|rows| rows.filter_map(Result::ok).collect::<Vec<_>>())
    else {
        return;
    };
    for id in ids {
        spawn_convert(state.clone(), id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_at_offset_653() {
        let mut bytes = vec![0u8; 700];
        for i in 0..20 {
            bytes[653 + i] = i as u8;
        }
        assert_eq!(
            file_checksum(&bytes).unwrap(),
            "000102030405060708090a0b0c0d0e0f10111213"
        );
    }

    #[test]
    fn parse_rcrack_finds_hex() {
        let out = "result\n999a hex:CAFED00D\n";
        assert_eq!(parse_rcrack_hex(out).as_deref(), Some("cafed00d"));
    }
}
