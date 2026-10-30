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
  pages_read INTEGER NOT NULL DEFAULT 0 CHECK(pages_read >= 0 AND pages_read <= total_pages)
);
CREATE TABLE IF NOT EXISTS epub_files (
  book_id INTEGER PRIMARY KEY REFERENCES books(id),
  digest TEXT NOT NULL,
  content BLOB NOT NULL,
  cover BLOB,
  cover_mime TEXT,
  position TEXT,
  percentage REAL NOT NULL DEFAULT 0 CHECK(percentage BETWEEN 0 AND 1)
);
CREATE INDEX IF NOT EXISTS epub_digest ON epub_files(digest);
