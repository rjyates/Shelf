CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY,
  username TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
  token_hash TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id),
  expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS books (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id),
  title TEXT NOT NULL,
  author TEXT NOT NULL,
  total_pages INTEGER NOT NULL CHECK(total_pages > 0),
  pages_read INTEGER NOT NULL DEFAULT 0 CHECK(pages_read >= 0 AND pages_read <= total_pages),
  -- Catalogue lifecycle for later stats (distinct from raw % / pages).
  reading_status TEXT NOT NULL DEFAULT 'open'
    CHECK(reading_status IN ('open','finished','unfinished','rereading'))
);
CREATE TABLE IF NOT EXISTS epub_files (
  book_id INTEGER PRIMARY KEY REFERENCES books(id),
  digest TEXT NOT NULL,
  content BLOB NOT NULL,
  cover BLOB,
  cover_mime TEXT,
  position TEXT,
  percentage REAL NOT NULL DEFAULT 0 CHECK(percentage BETWEEN 0 AND 1),
  format TEXT NOT NULL DEFAULT 'epub' CHECK(format IN ('epub','pdf'))
);
CREATE INDEX IF NOT EXISTS epub_digest ON epub_files(digest);
CREATE TABLE IF NOT EXISTS bookmarks (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id),
  book_id INTEGER NOT NULL REFERENCES books(id),
  cfi TEXT,
  title TEXT NOT NULL,
  note TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX IF NOT EXISTS bookmarks_book ON bookmarks(user_id, book_id);
CREATE TABLE IF NOT EXISTS annotations (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id),
  book_id INTEGER NOT NULL REFERENCES books(id),
  cfi TEXT,
  text TEXT NOT NULL DEFAULT '',
  color TEXT NOT NULL DEFAULT '#FACC15',
  style TEXT NOT NULL DEFAULT 'highlight',
  note TEXT,
  chapter_title TEXT,
  -- JSON AnnotationPdfPosition for EmbedPDF highlights; NULL for EPUB/CFI rows.
  pdf TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX IF NOT EXISTS annotations_book ON annotations(user_id, book_id);
CREATE TABLE IF NOT EXISTS reader_preferences (
  user_id INTEGER NOT NULL REFERENCES users(id),
  book_id INTEGER NOT NULL REFERENCES books(id),
  settings TEXT NOT NULL,
  PRIMARY KEY (user_id, book_id)
);
CREATE TABLE IF NOT EXISTS reader_defaults (
  user_id INTEGER PRIMARY KEY REFERENCES users(id),
  settings TEXT NOT NULL
);
