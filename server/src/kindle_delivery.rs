//! Send-to-Kindle (M12 slice 8): email a shelf EPUB/PDF to a Kindle address.
//!
//! BO reference @ c77966cc: subject "convert" for Kindle domains, EPUB/PDF only,
//! SMTP provider + recipient. Shelf keeps one SMTP+recipient row per user (no
//! multi-provider/template/group port).

use super::*;
use lettre::message::{header::ContentType, Attachment, Mailbox, Message, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use serde_json::{json, Value};

/// Amazon Personal Documents email attachment limit.
const KINDLE_MAX_ATTACHMENT_BYTES: usize = 50 * 1024 * 1024;
const KINDLE_CONVERT_SUBJECT: &str = "convert";

pub(crate) fn migrate(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS kindle_delivery_settings (
           user_id INTEGER PRIMARY KEY REFERENCES users(id),
           recipient_email TEXT NOT NULL DEFAULT '',
           smtp_host TEXT NOT NULL DEFAULT '',
           smtp_port INTEGER NOT NULL DEFAULT 587,
           smtp_username TEXT NOT NULL DEFAULT '',
           smtp_password TEXT NOT NULL DEFAULT '',
           from_address TEXT NOT NULL DEFAULT '',
           from_name TEXT NOT NULL DEFAULT '',
           start_tls INTEGER NOT NULL DEFAULT 1,
           ssl INTEGER NOT NULL DEFAULT 0,
           updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         )",
    )?;
    Ok(())
}

fn ensure_row(db: &Connection, user: i64) -> ApiResult<()> {
    db.execute(
        "INSERT OR IGNORE INTO kindle_delivery_settings(user_id) VALUES (?)",
        params![user],
    )?;
    Ok(())
}

/// Kindle Personal Documents domains (Amazon send-to-device).
pub(crate) fn is_kindle_address(email: &str) -> bool {
    let domain = email
        .rsplit_once('@')
        .map(|(_, d)| d.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if domain.is_empty() {
        return false;
    }
    domain == "kindle.com"
        || domain.ends_with(".kindle.com")
        || domain == "kindle.cn"
        || domain.ends_with(".kindle.cn")
}

pub(crate) fn is_sendable_format(format: &str) -> bool {
    matches!(format.to_ascii_lowercase().as_str(), "epub" | "pdf")
}

pub(crate) fn kindle_subject(recipient: &str, book_title: &str) -> String {
    if is_kindle_address(recipient) {
        KINDLE_CONVERT_SUBJECT.to_string()
    } else {
        let title = book_title.trim();
        if title.is_empty() {
            "Shelf book".to_string()
        } else {
            title.chars().take(200).collect()
        }
    }
}

pub(crate) fn attachment_filename(original: Option<&str>, format: &str, title: &str) -> String {
    if let Some(name) = original.map(str::trim).filter(|s| !s.is_empty()) {
        let base = std::path::Path::new(name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(name);
        if !base.contains("..") && !base.contains('/') && !base.contains('\\') {
            return base.to_string();
        }
    }
    let stem: String = title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(80)
        .collect();
    let stem = if stem.trim_matches('_').is_empty() {
        "book".to_string()
    } else {
        stem
    };
    let ext = match format.to_ascii_lowercase().as_str() {
        "pdf" => "pdf",
        _ => "epub",
    };
    format!("{stem}.{ext}")
}

fn attachment_content_type(format: &str) -> ApiResult<ContentType> {
    match format.to_ascii_lowercase().as_str() {
        "pdf" => ContentType::parse("application/pdf").map_err(|_| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not build attachment type.",
            )
        }),
        _ => ContentType::parse("application/epub+zip").map_err(|_| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not build attachment type.",
            )
        }),
    }
}

fn valid_email(raw: &str) -> bool {
    let s = raw.trim();
    if s.is_empty() || s.len() > 254 || s.contains(' ') {
        return false;
    }
    let Some((local, domain)) = s.rsplit_once('@') else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

#[derive(Clone, Debug)]
struct KindleSettings {
    recipient_email: String,
    smtp_host: String,
    smtp_port: i64,
    smtp_username: String,
    smtp_password: String,
    from_address: String,
    from_name: String,
    start_tls: bool,
    ssl: bool,
}

fn load_settings(db: &Connection, user: i64) -> ApiResult<KindleSettings> {
    ensure_row(db, user)?;
    db.query_row(
        "SELECT recipient_email,smtp_host,smtp_port,smtp_username,smtp_password,
                from_address,from_name,start_tls,ssl
         FROM kindle_delivery_settings WHERE user_id=?",
        params![user],
        |r| {
            Ok(KindleSettings {
                recipient_email: r.get(0)?,
                smtp_host: r.get(1)?,
                smtp_port: r.get(2)?,
                smtp_username: r.get(3)?,
                smtp_password: r.get(4)?,
                from_address: r.get(5)?,
                from_name: r.get(6)?,
                start_tls: r.get::<_, i64>(7)? != 0,
                ssl: r.get::<_, i64>(8)? != 0,
            })
        },
    )
    .map_err(ApiError::from)
}

fn settings_json(s: &KindleSettings) -> Value {
    json!({
        "recipientEmail": s.recipient_email,
        "smtpHost": s.smtp_host,
        "smtpPort": s.smtp_port,
        "smtpUsername": s.smtp_username,
        "fromAddress": s.from_address,
        "fromName": s.from_name,
        "startTls": s.start_tls,
        "ssl": s.ssl,
        "hasPassword": !s.smtp_password.is_empty(),
        "configured": settings_ready(s),
    })
}

fn settings_ready(s: &KindleSettings) -> bool {
    valid_email(&s.recipient_email)
        && !s.smtp_host.trim().is_empty()
        && (1..=65535).contains(&s.smtp_port)
        && valid_email(&s.from_address)
}

fn stub_smtp() -> bool {
    std::env::var("SHELF_KINDLE_SMTP_STUB").as_deref() == Ok("1")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KindleSettingsBody {
    #[serde(default)]
    recipient_email: Option<String>,
    #[serde(default)]
    smtp_host: Option<String>,
    #[serde(default)]
    smtp_port: Option<i64>,
    #[serde(default)]
    smtp_username: Option<String>,
    /// Omit to keep; empty string clears.
    #[serde(default)]
    smtp_password: Option<String>,
    #[serde(default)]
    from_address: Option<String>,
    #[serde(default)]
    from_name: Option<String>,
    #[serde(default)]
    start_tls: Option<bool>,
    #[serde(default)]
    ssl: Option<bool>,
}

pub(super) async fn get_kindle_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let settings = load_settings(db, user)?;
            Ok(Json(settings_json(&settings)))
        })
        .await
}

pub(super) async fn put_kindle_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<KindleSettingsBody>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let mut settings = load_settings(db, user)?;
            if let Some(v) = body.recipient_email {
                let v = v.trim().to_string();
                if !v.is_empty() && !valid_email(&v) {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "recipientEmail is not a valid address.",
                    ));
                }
                settings.recipient_email = v;
            }
            if let Some(v) = body.smtp_host {
                settings.smtp_host = v.trim().to_string();
            }
            if let Some(port) = body.smtp_port {
                if !(1..=65535).contains(&port) {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "smtpPort must be 1–65535.",
                    ));
                }
                settings.smtp_port = port;
            }
            if let Some(v) = body.smtp_username {
                settings.smtp_username = v.trim().to_string();
            }
            if let Some(v) = body.smtp_password {
                // shortcut: SMTP password stored plaintext in SQLite (single-user);
                // encrypt at rest when outbound secrets get a shared vault.
                settings.smtp_password = v;
            }
            if let Some(v) = body.from_address {
                let v = v.trim().to_string();
                if !v.is_empty() && !valid_email(&v) {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "fromAddress is not a valid address.",
                    ));
                }
                settings.from_address = v;
            }
            if let Some(v) = body.from_name {
                settings.from_name = v.trim().chars().take(120).collect();
            }
            if let Some(v) = body.start_tls {
                settings.start_tls = v;
            }
            if let Some(v) = body.ssl {
                settings.ssl = v;
            }
            db.execute(
                "UPDATE kindle_delivery_settings SET
                   recipient_email=?, smtp_host=?, smtp_port=?, smtp_username=?,
                   smtp_password=?, from_address=?, from_name=?,
                   start_tls=?, ssl=?,
                   updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE user_id=?",
                params![
                    settings.recipient_email,
                    settings.smtp_host,
                    settings.smtp_port,
                    settings.smtp_username,
                    settings.smtp_password,
                    settings.from_address,
                    settings.from_name,
                    if settings.start_tls { 1 } else { 0 },
                    if settings.ssl { 1 } else { 0 },
                    user,
                ],
            )?;
            Ok(Json(settings_json(&settings)))
        })
        .await
}

pub(super) async fn post_kindle_send(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let data_dir = state.data_dir.clone();
    let prepared = state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let settings = load_settings(db, user)?;
            if !settings_ready(&settings) {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Configure Kindle delivery (recipient + SMTP) in Settings first.",
                ));
            }
            let (title, original_filename): (String, Option<String>) = db
                .query_row(
                    "SELECT b.title, f.original_filename
                     FROM books b JOIN epub_files f ON f.book_id=b.id
                     WHERE b.id=? AND b.user_id=?",
                    params![id, user],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Book not found."))?;
            let (bytes, format) = load_owned_book_file(db, &data_dir, user, id)?;
            if !is_sendable_format(&format) {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Only EPUB and PDF can be sent to Kindle.",
                ));
            }
            if bytes.len() > KINDLE_MAX_ATTACHMENT_BYTES {
                return Err(ApiError(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Book exceeds Kindle's 50 MiB email attachment limit.",
                ));
            }
            let filename = attachment_filename(original_filename.as_deref(), &format, &title);
            let subject = kindle_subject(&settings.recipient_email, &title);
            Ok((settings, title, format, filename, subject, bytes))
        })
        .await?;

    let (settings, title, format, filename, subject, bytes) = prepared;
    deliver(&settings, &subject, &filename, &format, &bytes).await?;
    Ok(Json(json!({
        "sent": true,
        "to": settings.recipient_email,
        "subject": subject,
        "format": format,
        "fileName": filename,
        "title": title,
        "bytes": bytes.len(),
    })))
}

async fn deliver(
    settings: &KindleSettings,
    subject: &str,
    filename: &str,
    format: &str,
    bytes: &[u8],
) -> ApiResult<()> {
    let from: Mailbox = if settings.from_name.trim().is_empty() {
        settings
            .from_address
            .parse()
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "fromAddress is invalid."))?
    } else {
        format!(
            "{} <{}>",
            settings.from_name.replace(['<', '>', '"'], ""),
            settings.from_address
        )
        .parse()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "fromAddress is invalid."))?
    };
    let to: Mailbox = settings
        .recipient_email
        .parse()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "recipientEmail is invalid."))?;

    let attachment = Attachment::new(filename.to_string())
        .body(bytes.to_vec(), attachment_content_type(format)?);
    let message = Message::builder()
        .from(from)
        .to(to)
        .subject(subject)
        .multipart(
            MultiPart::mixed()
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(String::from("Sent from Shelf.")),
                )
                .singlepart(attachment),
        )
        .map_err(|_| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not build Kindle email.",
            )
        })?;

    if stub_smtp() {
        return Ok(());
    }

    let host = settings.smtp_host.trim();
    let port = settings.smtp_port as u16;
    let mut builder = if settings.ssl || port == 465 {
        AsyncSmtpTransport::<Tokio1Executor>::relay(host).map_err(|_| {
            ApiError(
                StatusCode::BAD_REQUEST,
                "SMTP host is invalid for TLS relay.",
            )
        })?
    } else if settings.start_tls || port == 587 {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host).map_err(|_| {
            ApiError(
                StatusCode::BAD_REQUEST,
                "SMTP host is invalid for STARTTLS.",
            )
        })?
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
    };
    builder = builder.port(port);
    if !settings.smtp_username.is_empty() {
        builder = builder.credentials(Credentials::new(
            settings.smtp_username.clone(),
            settings.smtp_password.clone(),
        ));
    }
    let mailer = builder.build();
    mailer.send(message).await.map_err(|_| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            "SMTP send failed. Check host, credentials, and that your From address is approved for Kindle.",
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kindle_domains_get_convert_subject() {
        assert!(is_kindle_address("me@kindle.com"));
        assert!(is_kindle_address("me@free.kindle.com"));
        assert!(is_kindle_address("me@kindle.cn"));
        assert!(!is_kindle_address("me@example.com"));
        assert_eq!(kindle_subject("a@kindle.com", "RotK"), "convert");
        assert_eq!(kindle_subject("a@mail.test", "RotK"), "RotK");
    }

    #[test]
    fn only_epub_pdf_sendable() {
        assert!(is_sendable_format("epub"));
        assert!(is_sendable_format("PDF"));
        assert!(!is_sendable_format("mp3"));
        assert!(!is_sendable_format("cbz"));
    }

    #[test]
    fn attachment_name_prefers_original() {
        assert_eq!(
            attachment_filename(Some("Fellowship.epub"), "epub", "Title"),
            "Fellowship.epub"
        );
        assert_eq!(attachment_filename(None, "pdf", "A Book!"), "A_Book_.pdf");
    }
}
