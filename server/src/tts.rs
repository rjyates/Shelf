//! TTS: chapter text + OpenAI-compatible provider proxy (Book Orbit shape).

use super::*;
use axum::extract::Query;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use serde_json::{json, Value};
use std::io::Cursor;

/// Block-level tags Foliate / BO use for TTS paragraph segmentation.
const BLOCK_TAGS: &[&str] = &[
    "article",
    "aside",
    "blockquote",
    "caption",
    "details",
    "dialog",
    "div",
    "dl",
    "dt",
    "dd",
    "figure",
    "footer",
    "form",
    "figcaption",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "tr",
];

fn is_block_tag(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    BLOCK_TAGS.iter().any(|t| *t == lower)
}

/// Pull plain-text blocks from XHTML roughly matching Foliate's getBlocks().
pub(crate) fn blocks_from_xhtml(html: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let bytes = html.as_bytes();
    let mut i = 0;
    let mut depth: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_script = false;
    let mut in_style = false;
    // Foliate walks the body; ignore <head> titles and metadata.
    let mut in_body = false;

    while i < bytes.len() {
        if bytes[i] == b'<' {
            let end = bytes[i..]
                .iter()
                .position(|&b| b == b'>')
                .map(|n| i + n)
                .unwrap_or(bytes.len());
            let tag = std::str::from_utf8(&bytes[i + 1..end]).unwrap_or("");
            let trimmed = tag.trim();
            let closing = trimmed.starts_with('/');
            let name = trimmed
                .trim_start_matches('/')
                .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if name == "body" {
                in_body = !closing;
                current.clear();
                depth.clear();
            } else if name == "script" {
                in_script = !closing;
            } else if name == "style" {
                in_style = !closing;
            } else if in_body && !in_script && !in_style && is_block_tag(&name) {
                if closing {
                    if depth.last().map(|d| d.as_str()) == Some(name.as_str()) {
                        depth.pop();
                        let text = current.split_whitespace().collect::<Vec<_>>().join(" ");
                        if !text.is_empty() {
                            blocks.push(text);
                        }
                        current.clear();
                    }
                } else if !trimmed.ends_with('/') {
                    if !current.trim().is_empty() && depth.is_empty() {
                        let text = current.split_whitespace().collect::<Vec<_>>().join(" ");
                        if !text.is_empty() {
                            blocks.push(text);
                        }
                        current.clear();
                    }
                    depth.push(name);
                }
            }
            i = end + 1;
            continue;
        }
        if in_body && !in_script && !in_style {
            current.push(bytes[i] as char);
        }
        i += 1;
    }
    let text = current.split_whitespace().collect::<Vec<_>>().join(" ");
    if !text.is_empty() {
        blocks.push(text);
    }
    blocks
}

fn chapter_xhtml(content: &[u8], chapter_index: u32) -> ApiResult<String> {
    let mut doc = ::epub::doc::EpubDoc::from_reader(Cursor::new(content))
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Could not read this EPUB."))?;
    if chapter_index as usize >= doc.spine.len() {
        return Err(ApiError(StatusCode::NOT_FOUND, "Chapter not found."));
    }
    if !doc.set_current_chapter(chapter_index as usize) {
        return Err(ApiError(StatusCode::NOT_FOUND, "Chapter not found."));
    }
    let (html, _mime) = doc.get_current_str().ok_or(ApiError(
        StatusCode::BAD_REQUEST,
        "This chapter has no readable text.",
    ))?;
    Ok(html)
}

/// `GET /api/v1/tts/text/{id}/{chapter}` — block text for BO TTS (0-based chapter).
pub(super) async fn chapter_text(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, chapter)): Path<(i64, u32)>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let (content, format): (Vec<u8>, String) = db
                .query_row(
                    "SELECT f.content,f.format FROM epub_files f JOIN books b ON b.id=f.book_id WHERE b.id=? AND b.user_id=?",
                    params![id, user],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Book file not found."))?;
            if format != "epub" {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "Text-to-speech chapter text is only available for EPUB books.",
                ));
            }
            let html = chapter_xhtml(&content, chapter)?;
            let sentences: Vec<Value> = blocks_from_xhtml(&html)
                .into_iter()
                .enumerate()
                .map(|(index, text)| json!({ "text": text, "index": index }))
                .collect();
            Ok(Json(json!({
                "chapterIndex": chapter,
                "sentences": sentences,
            })))
        })
        .await
}

#[derive(Clone)]
struct ProviderRow {
    id: i64,
    name: String,
    base_url: String,
    api_key: Option<String>,
    default_model: Option<String>,
    enabled: bool,
    display_order: i64,
    static_voices: Value,
    supports_voice_discovery: bool,
}

fn parse_voices(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| json!([]))
}

fn map_provider(r: &rusqlite::Row<'_>) -> rusqlite::Result<ProviderRow> {
    Ok(ProviderRow {
        id: r.get(0)?,
        name: r.get(1)?,
        base_url: r.get(2)?,
        api_key: r.get(3)?,
        default_model: r.get(4)?,
        enabled: r.get::<_, i64>(5)? != 0,
        display_order: r.get(6)?,
        static_voices: parse_voices(&r.get::<_, String>(7)?),
        supports_voice_discovery: r.get::<_, i64>(8)? != 0,
    })
}

const PROVIDER_SELECT: &str = "SELECT id,name,base_url,api_key,default_model,enabled,display_order,static_voices,supports_voice_discovery FROM tts_providers";

fn provider_json(p: &ProviderRow, include_secret: bool) -> Value {
    json!({
        "id": p.id,
        "name": p.name,
        "type": "openai-compatible",
        "enabled": p.enabled,
        "baseUrl": p.base_url,
        "apiKey": if include_secret { json!(p.api_key) } else { Value::Null },
        "defaultModel": p.default_model,
        "displayOrder": p.display_order,
        "staticVoices": p.static_voices,
        "supportsVoiceDiscovery": p.supports_voice_discovery,
    })
}

fn load_provider(db: &Connection, user: i64, id: i64) -> ApiResult<ProviderRow> {
    db.query_row(
        &format!("{PROVIDER_SELECT} WHERE id=? AND user_id=?"),
        params![id, user],
        map_provider,
    )
    .optional()?
    .ok_or(ApiError(StatusCode::NOT_FOUND, "TTS provider not found."))
}

fn normalize_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn speech_url(base: &str) -> String {
    format!("{}/audio/speech", normalize_base_url(base))
}

fn voices_url(base: &str) -> String {
    format!("{}/audio/voices", normalize_base_url(base))
}

/// Enabled providers for the reader (no secrets).
pub(super) async fn list_providers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let mut stmt = db.prepare(&format!(
                "{PROVIDER_SELECT} WHERE user_id=? AND enabled=1 ORDER BY display_order, id"
            ))?;
            let rows = stmt
                .query_map(params![user], map_provider)?
                .collect::<Result<Vec<_>, _>>()?;
            let list: Vec<Value> = rows
                .into_iter()
                .map(|p| {
                    json!({
                        "id": p.id.to_string(),
                        "name": p.name,
                        "type": "openai-compatible",
                    })
                })
                .collect();
            Ok(Json(Value::Array(list)))
        })
        .await
}

#[derive(Deserialize)]
pub(super) struct VoicesQuery {
    #[serde(rename = "providerId")]
    provider_id: Option<String>,
}

/// Curated / static voices for enabled providers.
pub(super) async fn list_voices(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<VoicesQuery>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let filter = query
                .provider_id
                .as_deref()
                .and_then(|s| s.parse::<i64>().ok());
            let mut stmt = db.prepare(&format!(
                "{PROVIDER_SELECT} WHERE user_id=? AND enabled=1 ORDER BY display_order, id"
            ))?;
            let rows = stmt
                .query_map(params![user], map_provider)?
                .collect::<Result<Vec<_>, _>>()?;
            let mut out = Vec::new();
            for p in rows {
                if let Some(want) = filter {
                    if p.id != want {
                        continue;
                    }
                }
                let Some(voices) = p.static_voices.as_array() else {
                    continue;
                };
                for voice in voices {
                    out.push(json!({
                        "id": voice.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                        "name": voice.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        "shortName": voice.get("shortName").and_then(|v| v.as_str()).unwrap_or(""),
                        "language": voice.get("language").and_then(|v| v.as_str()).unwrap_or(""),
                        "locale": voice.get("locale").and_then(|v| v.as_str()).unwrap_or(""),
                        "gender": voice.get("gender").and_then(|v| v.as_str()).unwrap_or(""),
                        "providerId": p.id.to_string(),
                        "providerName": p.name,
                    }));
                }
            }
            Ok(Json(Value::Array(out)))
        })
        .await
}

#[derive(Deserialize)]
pub(super) struct SynthesizeBody {
    text: String,
    #[serde(rename = "voiceId")]
    voice_id: String,
    #[serde(rename = "providerId")]
    provider_id: String,
    #[serde(default = "default_speed")]
    speed: f64,
    format: Option<String>,
}

fn default_speed() -> f64 {
    1.0
}

#[derive(Deserialize)]
pub(super) struct PreviewBody {
    #[serde(rename = "providerId")]
    provider_id: String,
    #[serde(rename = "voiceId")]
    voice_id: String,
}

async fn upstream_speech(
    provider: &ProviderRow,
    text: &str,
    voice_id: &str,
    speed: f64,
    format: &str,
) -> ApiResult<(String, Vec<u8>)> {
    let model = provider
        .default_model
        .clone()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| "tts-1".to_string());
    let mut req = reqwest::Client::new()
        .post(speech_url(&provider.base_url))
        .json(&json!({
            "model": model,
            "input": text,
            "voice": voice_id,
            "speed": speed.clamp(0.25, 4.0),
            "response_format": format,
        }));
    if let Some(key) = provider.api_key.as_deref().filter(|k| !k.is_empty()) {
        req = req.bearer_auth(key);
    }
    let response = req
        .send()
        .await
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "Could not reach the TTS provider."))?;
    if !response.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "TTS provider rejected the speech request.",
        ));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("audio/mpeg")
        .to_string();
    let bytes = response.bytes().await.map_err(|_| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            "TTS provider returned unreadable audio.",
        )
    })?;
    Ok((content_type, bytes.to_vec()))
}

pub(super) async fn synthesize(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SynthesizeBody>,
) -> ApiResult<Response> {
    if body.text.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Nothing to speak."));
    }
    let provider_id = body
        .provider_id
        .parse::<i64>()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Invalid TTS provider."))?;
    let provider = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                let p = load_provider(db, user, provider_id)?;
                if !p.enabled {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "That TTS provider is disabled.",
                    ));
                }
                Ok(p)
            }
        })
        .await?;
    let format = body.format.unwrap_or_else(|| "mp3".to_string());
    let (content_type, audio) =
        upstream_speech(&provider, &body.text, &body.voice_id, body.speed, &format).await?;
    Ok(([(CONTENT_TYPE, content_type)], audio).into_response())
}

pub(super) async fn preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PreviewBody>,
) -> ApiResult<Response> {
    synthesize(
        State(state),
        headers,
        Json(SynthesizeBody {
            text: "Shelf can read this book aloud once a TTS provider is configured.".to_string(),
            voice_id: body.voice_id,
            provider_id: body.provider_id,
            speed: 1.0,
            format: Some("mp3".to_string()),
        }),
    )
    .await
}

pub(super) async fn admin_list_providers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let mut stmt = db.prepare(&format!(
                "{PROVIDER_SELECT} WHERE user_id=? ORDER BY display_order, id"
            ))?;
            let rows = stmt
                .query_map(params![user], map_provider)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Json(Value::Array(
                rows.iter().map(|p| provider_json(p, true)).collect(),
            )))
        })
        .await
}

#[derive(Deserialize)]
pub(super) struct ProviderWrite {
    name: Option<String>,
    #[serde(rename = "baseUrl")]
    base_url: Option<String>,
    #[serde(rename = "apiKey")]
    api_key: Option<String>,
    #[serde(rename = "defaultModel")]
    default_model: Option<String>,
    enabled: Option<bool>,
    #[serde(rename = "staticVoices")]
    static_voices: Option<Value>,
    #[serde(rename = "supportsVoiceDiscovery")]
    supports_voice_discovery: Option<bool>,
}

pub(super) async fn admin_add_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ProviderWrite>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(ApiError(
            StatusCode::BAD_REQUEST,
            "Provider name is required.",
        ))?
        .to_string();
    let base_url = body
        .base_url
        .as_deref()
        .map(normalize_base_url)
        .filter(|s| !s.is_empty())
        .ok_or(ApiError(StatusCode::BAD_REQUEST, "Base URL is required."))?;
    let voices = body.static_voices.unwrap_or_else(|| json!([]));
    if !voices.is_array() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "staticVoices must be an array.",
        ));
    }
    let voices_json = voices.to_string();
    let api_key = body.api_key.filter(|k| !k.is_empty());
    let default_model = body.default_model.filter(|m| !m.trim().is_empty());
    let discovery = body.supports_voice_discovery.unwrap_or(true);
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let order: i64 = db.query_row(
                "SELECT COALESCE(MAX(display_order), -1) + 1 FROM tts_providers WHERE user_id=?",
                params![user],
                |r| r.get(0),
            )?;
            db.execute(
                "INSERT INTO tts_providers(user_id,name,base_url,api_key,default_model,enabled,display_order,static_voices,supports_voice_discovery)
                 VALUES(?,?,?,?,?,1,?,?,?)",
                params![
                    user,
                    name,
                    base_url,
                    api_key,
                    default_model,
                    order,
                    voices_json,
                    if discovery { 1 } else { 0 },
                ],
            )?;
            let id = db.last_insert_rowid();
            let p = load_provider(db, user, id)?;
            Ok((StatusCode::CREATED, Json(provider_json(&p, true))))
        })
        .await
}

pub(super) async fn admin_update_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<ProviderWrite>,
) -> ApiResult<Json<Value>> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let mut p = load_provider(db, user, id)?;
            if let Some(name) = body.name {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return Err(ApiError(StatusCode::BAD_REQUEST, "Provider name is required."));
                }
                p.name = name;
            }
            if let Some(url) = body.base_url {
                let url = normalize_base_url(&url);
                if url.is_empty() {
                    return Err(ApiError(StatusCode::BAD_REQUEST, "Base URL is required."));
                }
                p.base_url = url;
            }
            if let Some(key) = body.api_key {
                p.api_key = if key.is_empty() { None } else { Some(key) };
            }
            if let Some(model) = body.default_model {
                p.default_model = if model.trim().is_empty() {
                    None
                } else {
                    Some(model)
                };
            }
            if let Some(enabled) = body.enabled {
                p.enabled = enabled;
            }
            if let Some(voices) = body.static_voices {
                if !voices.is_array() {
                    return Err(ApiError(
                        StatusCode::BAD_REQUEST,
                        "staticVoices must be an array.",
                    ));
                }
                p.static_voices = voices;
            }
            if let Some(discovery) = body.supports_voice_discovery {
                p.supports_voice_discovery = discovery;
            }
            db.execute(
                "UPDATE tts_providers SET name=?, base_url=?, api_key=?, default_model=?, enabled=?, static_voices=?, supports_voice_discovery=?, updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=? AND user_id=?",
                params![
                    p.name,
                    p.base_url,
                    p.api_key,
                    p.default_model,
                    if p.enabled { 1 } else { 0 },
                    p.static_voices.to_string(),
                    if p.supports_voice_discovery { 1 } else { 0 },
                    id,
                    user,
                ],
            )?;
            Ok(Json(provider_json(&load_provider(db, user, id)?, true)))
        })
        .await
}

pub(super) async fn admin_delete_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            let n = db.execute(
                "DELETE FROM tts_providers WHERE id=? AND user_id=?",
                params![id, user],
            )?;
            if n == 0 {
                return Err(ApiError(StatusCode::NOT_FOUND, "TTS provider not found."));
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

#[derive(Deserialize)]
pub(super) struct ReorderBody {
    order: Vec<String>,
}

pub(super) async fn admin_reorder_providers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ReorderBody>,
) -> ApiResult<StatusCode> {
    state
        .db(move |db| {
            let user = authenticate(db, &headers)?;
            for (index, id_str) in body.order.iter().enumerate() {
                let id = id_str
                    .parse::<i64>()
                    .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Invalid provider id."))?;
                db.execute(
                    "UPDATE tts_providers SET display_order=? WHERE id=? AND user_id=?",
                    params![index as i64, id, user],
                )?;
            }
            Ok(StatusCode::NO_CONTENT)
        })
        .await
}

pub(super) async fn admin_test_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let provider = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                load_provider(db, user, id)
            }
        })
        .await?;
    let voice = provider
        .static_voices
        .as_array()
        .and_then(|v| v.first())
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("alloy");
    match upstream_speech(&provider, "Shelf TTS connection test.", voice, 1.0, "mp3").await {
        Ok(_) => Ok(Json(json!({
            "id": id.to_string(),
            "name": provider.name,
            "connected": true,
            "voiceCount": provider.static_voices.as_array().map(|a| a.len()).unwrap_or(0),
        }))),
        Err(_) => Ok(Json(json!({
            "id": id.to_string(),
            "name": provider.name,
            "connected": false,
            "voiceCount": 0,
            "error": "Could not reach or speak through this TTS provider.",
        }))),
    }
}

pub(super) async fn admin_discover_voices(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let provider = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                load_provider(db, user, id)
            }
        })
        .await?;
    if !provider.supports_voice_discovery {
        return Ok(Json(json!({ "voices": [], "supported": false })));
    }
    let mut req = reqwest::Client::new().get(voices_url(&provider.base_url));
    if let Some(key) = provider.api_key.as_deref().filter(|k| !k.is_empty()) {
        req = req.bearer_auth(key);
    }
    let response = match req.send().await {
        Ok(r) if r.status().is_success() => r,
        _ => {
            return Ok(Json(json!({ "voices": [], "supported": true })));
        }
    };
    let payload: Value = response.json().await.unwrap_or_else(|_| json!([]));
    let entries = if let Some(arr) = payload.as_array() {
        arr.clone()
    } else if let Some(arr) = payload.get("voices").and_then(|v| v.as_array()) {
        arr.clone()
    } else {
        Vec::new()
    };
    let voices: Vec<Value> = entries
        .into_iter()
        .filter_map(|entry| {
            let id = if let Some(s) = entry.as_str() {
                s.to_string()
            } else {
                entry.get("id")?.as_str()?.to_string()
            };
            if id.is_empty() {
                return None;
            }
            Some(json!({
                "id": id,
                "name": entry.get("name").and_then(|v| v.as_str()).unwrap_or(&id),
                "shortName": entry.get("shortName").and_then(|v| v.as_str()).unwrap_or(&id),
                "language": entry.get("language").and_then(|v| v.as_str()).unwrap_or(""),
                "locale": entry.get("locale").and_then(|v| v.as_str()).unwrap_or(""),
                "gender": entry.get("gender").and_then(|v| v.as_str()).unwrap_or(""),
                "providerId": provider.id.to_string(),
                "providerName": provider.name,
            }))
        })
        .collect();
    Ok(Json(json!({ "voices": voices, "supported": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_follow_paragraphs() {
        let html =
            r#"<html><body><h1>Title</h1><p>First paragraph.</p><p>Second one.</p></body></html>"#;
        assert_eq!(
            blocks_from_xhtml(html),
            vec![
                "Title".to_string(),
                "First paragraph.".to_string(),
                "Second one.".to_string()
            ]
        );
    }
}
