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
    CHECK(reading_status IN ('open','finished','unfinished','rereading')),
  series TEXT,
  series_index REAL,
  -- Same-shelf fold: secondary editions point at the visible primary card.
  merged_into INTEGER REFERENCES books(id) ON DELETE SET NULL
);
-- Index created in ensure_schema after ALTER for existing libraries.
CREATE TABLE IF NOT EXISTS collections (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id),
  name TEXT NOT NULL,
  UNIQUE(user_id, name)
);
CREATE TABLE IF NOT EXISTS collection_books (
  collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  PRIMARY KEY (collection_id, book_id)
);
CREATE TABLE IF NOT EXISTS smart_scopes (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id),
  name TEXT NOT NULL,
  filter TEXT NOT NULL,
  UNIQUE(user_id, name)
);
CREATE TABLE IF NOT EXISTS epub_files (
  book_id INTEGER PRIMARY KEY REFERENCES books(id),
  digest TEXT NOT NULL,
  content BLOB NOT NULL,
  cover BLOB,
  cover_mime TEXT,
  position TEXT,
  percentage REAL NOT NULL DEFAULT 0 CHECK(percentage BETWEEN 0 AND 1),
  format TEXT NOT NULL DEFAULT 'epub' CHECK(format IN ('epub','pdf','cbz','cbr','cb7','djvu','mp3','m4b','opus','ogg','flac','audiozip','aax')),
  -- Original upload for converted comics (CBR/CB7/DjVu) or AAX; NULL when content is the source.
  source BLOB,
  -- When set, book bytes live under $SHELF_DATA/{storage_path} and `content` is empty.
  storage_path TEXT,
  -- Same for oversized `source` blobs (converted comics / AAX).
  source_path TEXT,
  -- Original upload basename from x-shelf-filename / scan path (display only).
  original_filename TEXT,
  -- AAX → M4B job: pending/running/failed/ready; NULL for non-AAX books.
  conversion_status TEXT CHECK(conversion_status IS NULL OR conversion_status IN ('pending','running','failed','ready')),
  conversion_error TEXT,
  -- JSON array of {id,title,startMs,endMs} from M4B/MP4 chapter atoms (empty = probed, none found).
  chapter_markers TEXT
);
CREATE INDEX IF NOT EXISTS epub_digest ON epub_files(digest);
-- Audible activation_bytes keyed by AAX file checksum (account-constant; cache once).
CREATE TABLE IF NOT EXISTS aax_activation_cache (
  checksum TEXT PRIMARY KEY,
  activation_bytes TEXT NOT NULL CHECK(length(activation_bytes) = 8)
);
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
-- OpenAI-compatible TTS backends (Kokoro, LocalAI, etc.). Empty = listen UI hidden.
CREATE TABLE IF NOT EXISTS tts_providers (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id),
  name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  api_key TEXT,
  default_model TEXT,
  enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0, 1)),
  display_order INTEGER NOT NULL DEFAULT 0,
  static_voices TEXT NOT NULL DEFAULT '[]',
  supports_voice_discovery INTEGER NOT NULL DEFAULT 1 CHECK(supports_voice_discovery IN (0, 1)),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX IF NOT EXISTS tts_providers_user ON tts_providers(user_id, display_order, id);

CREATE TABLE IF NOT EXISTS audiobook_playback (
  book_id INTEGER PRIMARY KEY REFERENCES books(id),
  asset_id TEXT NOT NULL,
  position_ms INTEGER NOT NULL CHECK(position_ms >= 0),
  percentage REAL NOT NULL DEFAULT 0 CHECK(percentage BETWEEN 0 AND 1),
  completed INTEGER NOT NULL DEFAULT 0 CHECK(completed IN (0, 1)),
  revision INTEGER NOT NULL DEFAULT 1 CHECK(revision >= 1),
  captured_at TEXT NOT NULL,
  manifest_revision TEXT NOT NULL
);

-- EPUB + audiobook pairing and Storyteller-shaped align jobs (M10 slice 7).
CREATE TABLE IF NOT EXISTS book_works (
  id INTEGER PRIMARY KEY,
  epub_book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  audio_book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  readaloud_book_id INTEGER REFERENCES books(id) ON DELETE SET NULL,
  status TEXT NOT NULL DEFAULT 'idle' CHECK(status IN ('idle','queued','processing','failed','ready')),
  stage TEXT CHECK(stage IS NULL OR stage IN ('SPLIT_TRACKS','TRANSCRIBE_CHAPTERS','SYNC_CHAPTERS')),
  progress REAL NOT NULL DEFAULT 0,
  error TEXT,
  UNIQUE(epub_book_id, audio_book_id)
);

-- M11/M17 bibliographic metadata (Book Orbit–shaped).
CREATE TABLE IF NOT EXISTS book_metadata (
  book_id INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE,
  subtitle TEXT,
  description TEXT,
  isbn10 TEXT,
  isbn13 TEXT,
  publisher TEXT,
  published_date TEXT,
  published_year INTEGER,
  language TEXT,
  page_count INTEGER,
  rating INTEGER CHECK(rating IS NULL OR (rating >= 1 AND rating <= 10)),
  metadata_score INTEGER NOT NULL DEFAULT 0,
  locked_fields TEXT NOT NULL DEFAULT '[]',
  last_fetch_at TEXT,
  google_books_id TEXT,
  goodreads_id TEXT,
  amazon_id TEXT,
  hardcover_id TEXT,
  open_library_id TEXT,
  itunes_id TEXT,
  audible_id TEXT,
  audnexus_id TEXT,
  comicvine_id TEXT,
  cover_source TEXT CHECK(cover_source IS NULL OR cover_source IN ('extracted','custom')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

CREATE TABLE IF NOT EXISTS book_provider_ids (
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  external_id TEXT NOT NULL,
  source_url TEXT,
  PRIMARY KEY (book_id, provider)
);

CREATE TABLE IF NOT EXISTS authors (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  sort_name TEXT,
  description TEXT,
  UNIQUE(user_id, name)
);
CREATE TABLE IF NOT EXISTS book_authors (
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  author_id INTEGER NOT NULL REFERENCES authors(id) ON DELETE CASCADE,
  display_order INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (book_id, author_id)
);

CREATE TABLE IF NOT EXISTS series (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  normalized_name TEXT NOT NULL,
  expected_book_count INTEGER,
  UNIQUE(user_id, normalized_name)
);
CREATE TABLE IF NOT EXISTS book_series_memberships (
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  series_id INTEGER NOT NULL REFERENCES series(id) ON DELETE CASCADE,
  series_index TEXT,
  display_order INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (book_id, series_id)
);

CREATE TABLE IF NOT EXISTS genres (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  UNIQUE(user_id, name)
);
CREATE TABLE IF NOT EXISTS book_genres (
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  genre_id INTEGER NOT NULL REFERENCES genres(id) ON DELETE CASCADE,
  PRIMARY KEY (book_id, genre_id)
);

CREATE TABLE IF NOT EXISTS tags (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  UNIQUE(user_id, name)
);
CREATE TABLE IF NOT EXISTS book_tags (
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (book_id, tag_id)
);

CREATE TABLE IF NOT EXISTS narrators (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  UNIQUE(user_id, name)
);
CREATE TABLE IF NOT EXISTS book_narrators (
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  narrator_id INTEGER NOT NULL REFERENCES narrators(id) ON DELETE CASCADE,
  PRIMARY KEY (book_id, narrator_id)
);

CREATE TABLE IF NOT EXISTS publishers (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  UNIQUE(user_id, name)
);

CREATE TABLE IF NOT EXISTS book_covers (
  id INTEGER PRIMARY KEY,
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  medium TEXT NOT NULL CHECK(medium IN ('ebook','audio')),
  source TEXT NOT NULL CHECK(source IN ('extracted','custom','provider')),
  origin TEXT NOT NULL DEFAULT 'embedded'
    CHECK(origin IN ('embedded','folder_image','provider','upload','url','legacy')),
  provider TEXT,
  mime TEXT,
  bytes BLOB,
  width INTEGER,
  height INTEGER,
  active INTEGER NOT NULL DEFAULT 0 CHECK(active IN (0, 1)),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX IF NOT EXISTS book_covers_book ON book_covers(book_id, medium, active);

CREATE TABLE IF NOT EXISTS comic_metadata (
  book_id INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE,
  issue_number TEXT,
  volume_name TEXT,
  pencillers TEXT NOT NULL DEFAULT '[]',
  inkers TEXT NOT NULL DEFAULT '[]',
  colorists TEXT NOT NULL DEFAULT '[]',
  letterers TEXT NOT NULL DEFAULT '[]',
  cover_artists TEXT NOT NULL DEFAULT '[]',
  characters TEXT NOT NULL DEFAULT '[]',
  teams TEXT NOT NULL DEFAULT '[]',
  locations TEXT NOT NULL DEFAULT '[]',
  story_arcs TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE IF NOT EXISTS custom_metadata_fields (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  label TEXT NOT NULL,
  field_type TEXT NOT NULL DEFAULT 'text'
    CHECK(field_type IN ('text','url','number','date','boolean')),
  display_order INTEGER NOT NULL DEFAULT 0,
  UNIQUE(user_id, key)
);
CREATE TABLE IF NOT EXISTS book_custom_metadata (
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  field_id INTEGER NOT NULL REFERENCES custom_metadata_fields(id) ON DELETE CASCADE,
  value_text TEXT,
  PRIMARY KEY (book_id, field_id)
);

-- Built-in + external metadata providers (credentials per user).
CREATE TABLE IF NOT EXISTS metadata_providers (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  name TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0, 1)),
  kind TEXT NOT NULL DEFAULT 'builtin'
    CHECK(kind IN ('builtin','external')),
  base_url TEXT,
  api_key TEXT,
  config_json TEXT NOT NULL DEFAULT '{}',
  display_order INTEGER NOT NULL DEFAULT 0,
  UNIQUE(user_id, key)
);

-- Field rules, score weights, auto-fetch config (one row per user).
CREATE TABLE IF NOT EXISTS metadata_settings (
  user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  field_rules TEXT NOT NULL DEFAULT '{}',
  score_weights TEXT NOT NULL DEFAULT '{}',
  fetch_config TEXT NOT NULL DEFAULT '{}',
  paused INTEGER NOT NULL DEFAULT 0 CHECK(paused IN (0, 1)),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

CREATE TABLE IF NOT EXISTS metadata_fetch_jobs (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  reason TEXT NOT NULL DEFAULT 'auto'
    CHECK(reason IN ('auto','import','refresh','run','manual')),
  status TEXT NOT NULL DEFAULT 'queued'
    CHECK(status IN ('queued','processing','done','failed','cancelled')),
  attempts INTEGER NOT NULL DEFAULT 0,
  error TEXT,
  ignore_cache INTEGER NOT NULL DEFAULT 0 CHECK(ignore_cache IN (0, 1)),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX IF NOT EXISTS metadata_fetch_jobs_status
  ON metadata_fetch_jobs(status, id);
CREATE INDEX IF NOT EXISTS metadata_fetch_jobs_user
  ON metadata_fetch_jobs(user_id, status, id);


-- M12 capture: reading sessions, daily rollups, progression history, lifecycle events.
CREATE TABLE IF NOT EXISTS reading_sessions (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  client_session_id TEXT NOT NULL,
  started_at TEXT NOT NULL,
  ended_at TEXT NOT NULL,
  duration_seconds INTEGER NOT NULL CHECK(duration_seconds >= 0),
  progress_start REAL,
  progress_end REAL,
  progress_delta REAL,
  session_type TEXT NOT NULL DEFAULT 'read'
    CHECK(session_type IN ('read','tts','listen')),
  source TEXT NOT NULL DEFAULT 'web'
    CHECK(source IN ('web','android','ios','manual','koreader','kobo')),
  format TEXT,
  chars_delta INTEGER,
  pages_delta INTEGER,
  device_id TEXT,
  device_name TEXT,
  timezone_offset_minutes INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  UNIQUE(user_id, client_session_id)
);
CREATE INDEX IF NOT EXISTS reading_sessions_user_ended
  ON reading_sessions(user_id, ended_at DESC);
CREATE INDEX IF NOT EXISTS reading_sessions_book
  ON reading_sessions(book_id, ended_at DESC);

-- Kobo SpentReadingMinutes cursor (session injector idempotency).
CREATE TABLE IF NOT EXISTS reading_session_sync_cursors (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  source TEXT NOT NULL,
  device_key TEXT NOT NULL,
  generation INTEGER NOT NULL DEFAULT 0,
  spent_reading_minutes INTEGER NOT NULL DEFAULT 0,
  last_modified TEXT,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  PRIMARY KEY (user_id, book_id, source, device_key)
);

CREATE TABLE IF NOT EXISTS reading_daily_stats (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  day_local TEXT NOT NULL,
  source TEXT NOT NULL DEFAULT 'web'
    CHECK(source IN ('web','android','ios','manual','koreader','kobo')),
  reading_seconds INTEGER NOT NULL DEFAULT 0 CHECK(reading_seconds >= 0),
  progress_delta_sum REAL NOT NULL DEFAULT 0,
  session_count INTEGER NOT NULL DEFAULT 0 CHECK(session_count >= 0),
  timezone TEXT NOT NULL DEFAULT 'UTC',
  PRIMARY KEY (user_id, day_local, source)
);

CREATE TABLE IF NOT EXISTS progression_history (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  progression REAL NOT NULL CHECK(progression BETWEEN 0 AND 1),
  position TEXT,
  format TEXT NOT NULL DEFAULT 'epub',
  source TEXT NOT NULL DEFAULT 'web'
    CHECK(source IN ('web','android','ios','manual','koreader','kobo')),
  device_id TEXT,
  modified_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX IF NOT EXISTS progression_history_book
  ON progression_history(user_id, book_id, modified_at DESC);

CREATE TABLE IF NOT EXISTS reading_events (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  event_type TEXT NOT NULL
    CHECK(event_type IN ('opened','finished','unfinished','rereading','progress_milestone')),
  at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  source TEXT NOT NULL DEFAULT 'web'
    CHECK(source IN ('web','android','ios','manual','koreader','kobo')),
  payload_json TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS reading_events_user_at
  ON reading_events(user_id, at DESC);
CREATE INDEX IF NOT EXISTS reading_events_book
  ON reading_events(book_id, at DESC);

CREATE TABLE IF NOT EXISTS user_reading_defaults (
  user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  timezone TEXT NOT NULL DEFAULT 'UTC',
  timezone_offset_minutes INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

CREATE TABLE IF NOT EXISTS reading_goals (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  year INTEGER NOT NULL CHECK(year >= 1970 AND year <= 2100),
  target_books INTEGER NOT NULL CHECK(target_books >= 0 AND target_books <= 10000),
  PRIMARY KEY (user_id, year)
);
