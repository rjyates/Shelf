use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rand::{rngs::OsRng, RngCore};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone)]
struct AppState {
    database: PathBuf,
    login_attempts: Arc<Mutex<VecDeque<Instant>>>,
}

#[derive(Debug)]
struct ApiError(StatusCode, &'static str);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error": self.1}))).into_response()
    }
}
impl From<rusqlite::Error> for ApiError {
    fn from(_: rusqlite::Error) -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database operation failed.",
        )
    }
}
type ApiResult<T> = Result<T, ApiError>;

impl AppState {
    fn limit_login(&self) -> ApiResult<()> {
        let mut attempts = self
            .login_attempts
            .lock()
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Sign in failed."))?;
        let now = Instant::now();
        while attempts
            .front()
            .is_some_and(|time| now.duration_since(*time) >= Duration::from_secs(60))
        {
            attempts.pop_front();
        }
        if attempts.len() >= 20 {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many sign-in attempts. Try again in a minute.",
            ));
        }
        attempts.push_back(now);
        Ok(())
    }
    async fn db<T: Send + 'static>(
        &self,
        action: impl FnOnce(&mut Connection) -> ApiResult<T> + Send + 'static,
    ) -> ApiResult<T> {
        let path = self.database.clone();
        tokio::task::spawn_blocking(move || {
            let mut db = Connection::open(path)?;
            db.busy_timeout(std::time::Duration::from_secs(5))?;
            db.pragma_update(None, "foreign_keys", "ON")?;
            action(&mut db)
        })
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Request failed."))?
    }
}

pub fn app(database: PathBuf) -> rusqlite::Result<Router> {
    let db = Connection::open(&database)?;
    db.pragma_update(None, "journal_mode", "WAL")?;
    db.execute_batch(include_str!("../schema.sql"))?;
    Ok(Router::new()
        .route("/", get(|| async { Html(include_str!("../../web/index.html")) }))
        .route("/app.css", get(|| async { ([(header::CONTENT_TYPE, "text/css")], include_str!("../../web/app.css")) }))
        .route("/app.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], include_str!("../../web/app.js")) }))
        .route("/api.mjs", get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], include_str!("../../web/api.mjs")) }))
        .route("/fonts/serif.ttf", get(|| async { ([(header::CONTENT_TYPE, "font/ttf")], include_bytes!("../../web/fonts/serif.ttf").as_slice()) }))
        .route("/fonts/sans.ttf", get(|| async { ([(header::CONTENT_TYPE, "font/ttf")], include_bytes!("../../web/fonts/sans.ttf").as_slice()) }))
        .route("/fonts/mono.ttf", get(|| async { ([(header::CONTENT_TYPE, "font/ttf")], include_bytes!("../../web/fonts/mono.ttf").as_slice()) }))
        .route("/api/v1/health", get(|| async { Json(serde_json::json!({"status":"ok", "api_version":1, "version":option_env!("SHELF_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))})) }))
        .route("/api/v1/setup", get(setup_status).post(setup))
        .route("/api/v1/login", post(login))
        .route("/api/v1/logout", post(logout))
        .route("/api/v1/books", get(books).post(add_book))
        .route("/api/v1/books/{id}/progress", post(update_progress))
        .route("/api/v1/stats", get(stats))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(axum::middleware::map_response(|mut response: Response| async {
            let headers = response.headers_mut();
            headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
            headers.insert(header::CONTENT_SECURITY_POLICY, "default-src 'self'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'".parse().unwrap());
            response
        }))
        .with_state(AppState { database, login_attempts: Arc::new(Mutex::new(VecDeque::new())) }))
}

async fn setup_status(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    state
        .db(|db| {
            let count: i64 = db.query_row("SELECT count(*) FROM users", [], |r| r.get(0))?;
            Ok(Json(serde_json::json!({"setup_required":count == 0})))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    username: String,
    password: String,
}

fn validate_credentials(value: &Credentials) -> ApiResult<()> {
    if value.username.trim().is_empty()
        || value.username.len() > 100
        || value.password.len() < 12
        || value.password.len() > 1024
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Use a username of 1–100 characters and a password of 12–1024 bytes.",
        ));
    }
    Ok(())
}

async fn setup(
    State(state): State<AppState>,
    Json(value): Json<Credentials>,
) -> ApiResult<StatusCode> {
    validate_credentials(&value)?;
    state
        .db(move |db| {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let count: i64 = tx.query_row("SELECT count(*) FROM users", [], |r| r.get(0))?;
            if count != 0 {
                return Err(ApiError(StatusCode::CONFLICT, "Setup is already complete."));
            }
            let salt = SaltString::generate(&mut OsRng);
            let hash = Argon2::default()
                .hash_password(value.password.as_bytes(), &salt)
                .map_err(|_| {
                    ApiError(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Account creation failed.",
                    )
                })?
                .to_string();
            tx.execute(
                "INSERT INTO users(username, password_hash) VALUES (?, ?)",
                params![value.username.trim(), hash],
            )?;
            tx.commit()?;
            Ok(StatusCode::CREATED)
        })
        .await
}

fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

fn authenticate(db: &Connection, headers: &HeaderMap) -> ApiResult<i64> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .find_map(|item| item.trim().strip_prefix("shelf_session="))
        });
    let token = bearer
        .or(cookie)
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "Please sign in."))?;
    db.query_row(
        "SELECT user_id FROM sessions WHERE token_hash=? AND expires_at > unixepoch()",
        [token_hash(token)],
        |r| r.get(0),
    )
    .optional()?
    .ok_or(ApiError(
        StatusCode::UNAUTHORIZED,
        "Your session has expired. Please sign in.",
    ))
}

async fn login(
    State(state): State<AppState>,
    Json(value): Json<Credentials>,
) -> ApiResult<Response> {
    state.limit_login()?;
    if value.username.len() > 100 || value.password.len() > 1024 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid credentials."));
    }
    state.db(move |db| {
        let record: Option<(i64, String)> = db.query_row("SELECT id,password_hash FROM users WHERE username=?", [value.username.trim()], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        // Always perform a password verification, including for unknown usernames.
        let dummy_salt = SaltString::encode_b64(b"shelf-login-padding").map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Sign in failed."))?;
        let dummy = Argon2::default().hash_password(b"not-a-user-password", &dummy_salt).map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Sign in failed."))?.to_string();
        let hash = record.as_ref().map(|r| r.1.as_str()).unwrap_or(&dummy);
        let parsed = PasswordHash::new(hash).map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Sign in failed."))?;
        if Argon2::default().verify_password(value.password.as_bytes(), &parsed).is_err() || record.is_none() {
            return Err(ApiError(StatusCode::UNAUTHORIZED, "Username or password is incorrect."));
        }
        let mut bytes = [0u8; 32]; OsRng.fill_bytes(&mut bytes);
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        db.execute("DELETE FROM sessions WHERE expires_at <= unixepoch()", [])?;
        db.execute("INSERT INTO sessions(token_hash,user_id,expires_at) VALUES (?,?,unixepoch()+604800)", params![token_hash(&token), record.unwrap().0])?;
        let secure = if std::env::var("SHELF_SECURE_COOKIE").as_deref() == Ok("true") { "; Secure" } else { "" };
        let cookie = format!("shelf_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=604800{secure}");
        Ok(([(header::SET_COOKIE, cookie), (header::CACHE_CONTROL, "no-store".into())], Json(serde_json::json!({"token":token}))).into_response())
    }).await
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            // Explicit sign out revokes all sessions for this initial single-account edition.
            db.execute("DELETE FROM sessions WHERE user_id=?", [user])?;
            Ok((
                [(
                    header::SET_COOKIE,
                    "shelf_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
                )],
                StatusCode::NO_CONTENT,
            )
                .into_response())
        })
        .await
}

#[derive(Serialize)]
struct Book {
    id: i64,
    title: String,
    author: String,
    total_pages: i64,
    pages_read: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewBook {
    title: String,
    author: String,
    total_pages: i64,
}

async fn books(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Vec<Book>>> {
    state.db(move |db| {
        let user = authenticate(db, &headers)?;
        let mut query = db.prepare("SELECT id,title,author,total_pages,pages_read FROM books WHERE user_id=? ORDER BY id DESC")?;
        let books = query.query_map([user], |r| Ok(Book { id:r.get(0)?,title:r.get(1)?,author:r.get(2)?,total_pages:r.get(3)?,pages_read:r.get(4)? }))?.collect::<Result<Vec<_>,_>>()?;
        Ok(Json(books))
    }).await
}

async fn add_book(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(value): Json<NewBook>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    if value.title.trim().is_empty()
        || value.title.len() > 500
        || value.author.len() > 300
        || !(1..=1_000_000).contains(&value.total_pages)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Enter a title, an author up to 300 characters, and 1–1,000,000 pages.",
        ));
    }
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            db.execute(
                "INSERT INTO books(user_id,title,author,total_pages) VALUES (?,?,?,?)",
                params![
                    user,
                    value.title.trim(),
                    value.author.trim(),
                    value.total_pages
                ],
            )?;
            Ok((
                StatusCode::CREATED,
                Json(serde_json::json!({"id":db.last_insert_rowid()})),
            ))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Progress {
    pages_read: i64,
}
async fn update_progress(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(value): Json<Progress>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let total: Option<i64> = db
                .query_row(
                    "SELECT total_pages FROM books WHERE id=? AND user_id=?",
                    params![id, user],
                    |r| r.get(0),
                )
                .optional()?;
            let total = total.ok_or(ApiError(StatusCode::NOT_FOUND, "Book not found."))?;
            if !(0..=total).contains(&value.pages_read) {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Progress must be between zero and the book's page count.",
                ));
            }
            db.execute(
                "UPDATE books SET pages_read=? WHERE id=? AND user_id=?",
                params![value.pages_read, id, user],
            )?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

async fn stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    state.db(move |db| {
        let user = authenticate(db, &headers)?;
        let values: (i64,i64,i64,i64) = db.query_row("SELECT count(*),coalesce(sum(pages_read),0),coalesce(sum(pages_read=total_pages),0),coalesce(sum(pages_read>0 AND pages_read<total_pages),0) FROM books WHERE user_id=?", [user], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        Ok(Json(serde_json::json!({"books":values.0,"pages_read":values.1,"finished":values.2,"reading":values.3})))
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_in_limit_rejects_bursts_and_expires_old_attempts() {
        let state = AppState {
            database: PathBuf::new(),
            login_attempts: Arc::new(Mutex::new(VecDeque::new())),
        };
        for _ in 0..20 {
            assert!(state.limit_login().is_ok());
        }
        assert!(matches!(
            state.limit_login(),
            Err(ApiError(StatusCode::TOO_MANY_REQUESTS, _))
        ));
        let old = Instant::now() - Duration::from_secs(61);
        *state.login_attempts.lock().unwrap() = VecDeque::from(vec![old; 20]);
        assert!(state.limit_login().is_ok());
    }
}
