//! Persistable alignment jobs and EPUB+audio "works" (unified catalogue row).

use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignStage {
    SplitTracks,
    TranscribeChapters,
    SyncChapters,
}

impl AlignStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SplitTracks => "SPLIT_TRACKS",
            Self::TranscribeChapters => "TRANSCRIBE_CHAPTERS",
            Self::SyncChapters => "SYNC_CHAPTERS",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "SPLIT_TRACKS" => Some(Self::SplitTracks),
            "TRANSCRIBE_CHAPTERS" => Some(Self::TranscribeChapters),
            "SYNC_CHAPTERS" => Some(Self::SyncChapters),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignJobStatus {
    Idle,
    Queued,
    Processing,
    Failed,
    Ready,
}

impl AlignJobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Queued => "queued",
            Self::Processing => "processing",
            Self::Failed => "failed",
            Self::Ready => "ready",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "idle" => Some(Self::Idle),
            "queued" => Some(Self::Queued),
            "processing" => Some(Self::Processing),
            "failed" => Some(Self::Failed),
            "ready" => Some(Self::Ready),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BookWork {
    pub id: i64,
    pub epub_book_id: i64,
    pub audio_book_id: i64,
    pub readaloud_book_id: Option<i64>,
    pub status: AlignJobStatus,
    pub stage: Option<AlignStage>,
    pub progress: f32,
    pub error: Option<String>,
}

pub fn create_work(
    conn: &Connection,
    epub_book_id: i64,
    audio_book_id: i64,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO book_works(epub_book_id, audio_book_id, status, progress) VALUES (?1,?2,'idle',0)",
        params![epub_book_id, audio_book_id],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_work(conn: &Connection, id: i64) -> rusqlite::Result<Option<BookWork>> {
    conn.query_row(
        "SELECT id, epub_book_id, audio_book_id, readaloud_book_id, status, stage, progress, error
         FROM book_works WHERE id=?1",
        params![id],
        |row| {
            Ok(BookWork {
                id: row.get(0)?,
                epub_book_id: row.get(1)?,
                audio_book_id: row.get(2)?,
                readaloud_book_id: row.get(3)?,
                status: AlignJobStatus::parse(&row.get::<_, String>(4)?)
                    .unwrap_or(AlignJobStatus::Idle),
                stage: row
                    .get::<_, Option<String>>(5)?
                    .and_then(|s| AlignStage::parse(&s)),
                progress: row.get::<_, f64>(6)? as f32,
                error: row.get(7)?,
            })
        },
    )
    .optional()
}

pub fn list_works(conn: &Connection) -> rusqlite::Result<Vec<BookWork>> {
    let mut stmt = conn.prepare(
        "SELECT id, epub_book_id, audio_book_id, readaloud_book_id, status, stage, progress, error
         FROM book_works ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(BookWork {
            id: row.get(0)?,
            epub_book_id: row.get(1)?,
            audio_book_id: row.get(2)?,
            readaloud_book_id: row.get(3)?,
            status: AlignJobStatus::parse(&row.get::<_, String>(4)?)
                .unwrap_or(AlignJobStatus::Idle),
            stage: row
                .get::<_, Option<String>>(5)?
                .and_then(|s| AlignStage::parse(&s)),
            progress: row.get::<_, f64>(6)? as f32,
            error: row.get(7)?,
        })
    })?;
    rows.collect()
}

pub fn start_align_job(conn: &Connection, work_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE book_works SET status='queued', stage='SPLIT_TRACKS', progress=0, error=NULL WHERE id=?1",
        params![work_id],
    )?;
    Ok(())
}

/// Persist status/stage/progress while the align worker runs.
pub fn align_job_tick(
    conn: &Connection,
    work_id: i64,
    status: AlignJobStatus,
    stage: AlignStage,
    progress: f32,
    error: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE book_works SET status=?2, stage=?3, progress=?4, error=?5 WHERE id=?1",
        params![
            work_id,
            status.as_str(),
            stage.as_str(),
            progress as f64,
            error
        ],
    )?;
    Ok(())
}

pub fn finish_work(
    conn: &Connection,
    work_id: i64,
    readaloud_book_id: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE book_works SET status='ready', stage='SYNC_CHAPTERS', progress=1, error=NULL, readaloud_book_id=?2 WHERE id=?1",
        params![work_id, readaloud_book_id],
    )?;
    Ok(())
}

pub fn fail_work(conn: &Connection, work_id: i64, error: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE book_works SET status='failed', progress=1, error=?2 WHERE id=?1",
        params![work_id, error],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE book_works (
                id INTEGER PRIMARY KEY,
                epub_book_id INTEGER NOT NULL,
                audio_book_id INTEGER NOT NULL,
                readaloud_book_id INTEGER,
                status TEXT NOT NULL DEFAULT 'idle',
                stage TEXT,
                progress REAL NOT NULL DEFAULT 0,
                error TEXT
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn create_start_and_tick_job() {
        let conn = mem();
        let id = create_work(&conn, 1, 2).unwrap();
        start_align_job(&conn, id).unwrap();
        let w = get_work(&conn, id).unwrap().unwrap();
        assert_eq!(w.status, AlignJobStatus::Queued);
        align_job_tick(
            &conn,
            id,
            AlignJobStatus::Processing,
            AlignStage::TranscribeChapters,
            0.4,
            None,
        )
        .unwrap();
        let w = get_work(&conn, id).unwrap().unwrap();
        assert_eq!(w.status, AlignJobStatus::Processing);
        assert_eq!(w.stage, Some(AlignStage::TranscribeChapters));
    }
}
