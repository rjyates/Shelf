//! EPUB3 media-overlay playlist for Book Orbit's resume / chapter timing UI.
//! Playback itself uses Foliate against the whole-file EPUB blob.

use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Seek};
use std::path::{Component, Path, PathBuf};
use zip::ZipArchive;

fn zip_entry_name<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str) -> Option<String> {
    if archive.by_name(name).is_ok() {
        return Some(name.to_string());
    }
    let wanted = name.replace('\\', "/").to_ascii_lowercase();
    (0..archive.len()).find_map(|i| {
        let entry = archive.by_index(i).ok()?;
        let n = entry.name().replace('\\', "/");
        (n.to_ascii_lowercase() == wanted).then_some(n)
    })
}

fn zip_bytes<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str) -> ApiResult<Vec<u8>> {
    let entry_name = zip_entry_name(archive, name).ok_or(ApiError(
        StatusCode::BAD_REQUEST,
        "Could not read this EPUB.",
    ))?;
    let mut file = archive
        .by_name(&entry_name)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Could not read this EPUB."))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Could not read this EPUB."))?;
    Ok(buf)
}

fn zip_string<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str) -> ApiResult<String> {
    let bytes = zip_bytes(archive, name)?;
    String::from_utf8(bytes)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "EPUB XML is not valid UTF-8."))
}

/// Uncompressed size from the central directory — do not inflate audio just for metadata.
fn zip_size<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str) -> u64 {
    let Some(entry_name) = zip_entry_name(archive, name) else {
        return 0;
    };
    archive.by_name(&entry_name).map(|f| f.size()).unwrap_or(0)
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

fn attr_owned(tag: &str, name: &str) -> Option<String> {
    attr(tag, name).map(html_unescape)
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn resolve_href(base: &str, href: &str) -> String {
    if href.starts_with('/') {
        return href.trim_start_matches('/').replace('\\', "/");
    }
    if href.contains("://") {
        return href.to_string();
    }
    let base_path = Path::new(base);
    let parent = base_path.parent().unwrap_or_else(|| Path::new(""));
    let joined = parent.join(href);
    let mut out = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            Component::Normal(part) => out.push(part),
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    out.to_string_lossy().replace('\\', "/")
}

/// Foliate-compatible SMIL / EPUB clock values (`12.5`, `12.5s`, `0:01:30.5`, …).
pub(crate) fn parse_clock(raw: &str) -> Option<f64> {
    let str = raw
        .trim()
        .trim_start_matches("npt=")
        .trim_matches(|c: char| c == '"' || c == '\'');
    if str.is_empty() {
        return None;
    }
    let parts: Vec<&str> = str.split(':').collect();
    if parts.len() == 3 {
        let h: f64 = parts[0].parse().ok()?;
        let m: f64 = parts[1].parse().ok()?;
        let s: f64 = parts[2].parse().ok()?;
        return Some(h * 3600.0 + m * 60.0 + s);
    }
    if parts.len() == 2 {
        let m: f64 = parts[0].parse().ok()?;
        let s: f64 = parts[1].parse().ok()?;
        return Some(m * 60.0 + s);
    }
    let (num, unit) = str
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map(|i| (&str[..i], &str[i..]))
        .unwrap_or((str, ""));
    let n: f64 = num.parse().ok()?;
    let factor = match unit {
        "h" => 3600.0,
        "min" => 60.0,
        "ms" => 0.001,
        _ => 1.0, // bare number or trailing `s`
    };
    Some(n * factor)
}

struct ManifestItem {
    id: String,
    href: String,
    media_type: String,
    media_overlay: Option<String>,
}

fn parse_manifest_and_spine(opf: &str) -> (Vec<ManifestItem>, Vec<String>) {
    let mut items = Vec::new();
    let mut spine = Vec::new();
    let lower = opf; // keep original for attr quotes
    let mut search = lower;
    while let Some(start) = search.find("<item") {
        let rest = &search[start..];
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        if let (Some(id), Some(href)) = (attr_owned(tag, "id"), attr_owned(tag, "href")) {
            items.push(ManifestItem {
                id,
                href,
                media_type: attr_owned(tag, "media-type").unwrap_or_default(),
                media_overlay: attr_owned(tag, "media-overlay"),
            });
        }
        search = &rest[end.max(1)..];
    }
    search = lower;
    while let Some(start) = search.find("<itemref") {
        let rest = &search[start..];
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        if let Some(idref) = attr_owned(tag, "idref") {
            spine.push(idref);
        }
        search = &rest[end.max(1)..];
    }
    (items, spine)
}

struct SmilPar {
    text_href: String,
    text_fragment: Option<String>,
    audio_href: String,
    clip_begin: f64,
    clip_end: Option<f64>,
}

fn parse_smil_pars(smil: &str, smil_href: &str) -> Vec<SmilPar> {
    let mut pars = Vec::new();
    let mut search = smil;
    while let Some(start) = search.find("<par") {
        let rest = &search[start..];
        let close = rest
            .find("</par>")
            .map(|i| i + "</par>".len())
            .or_else(|| rest.find("/>").map(|i| i + 2))
            .unwrap_or(rest.len());
        let block = &rest[..close];
        let text_src = {
            let mut s = block;
            let mut found = None;
            while let Some(i) = s.find("<text") {
                let Some(tag_end) = s[i..].find('>').map(|e| i + e) else {
                    break;
                };
                if let Some(src) = attr_owned(&s[i..tag_end], "src") {
                    found = Some(src);
                    break;
                }
                s = &s[tag_end.max(1)..];
            }
            found
        };
        let audio = {
            let mut s = block;
            let mut found = None;
            while let Some(i) = s.find("<audio") {
                let Some(tag_end) = s[i..].find('>').map(|e| i + e) else {
                    break;
                };
                let tag = &s[i..tag_end];
                if let Some(src) = attr_owned(tag, "src") {
                    found = Some((
                        src,
                        attr(tag, "clipBegin").and_then(parse_clock).unwrap_or(0.0),
                        attr(tag, "clipEnd").and_then(parse_clock),
                    ));
                    break;
                }
                s = &s[tag_end.max(1)..];
            }
            found
        };
        if let (Some(text), Some((audio_src, begin, end))) = (text_src, audio) {
            let resolved_text = resolve_href(smil_href, &text);
            let (path, frag) = match resolved_text.split_once('#') {
                Some((p, f)) => (p.to_string(), Some(f.to_string())),
                None => (resolved_text, None),
            };
            pars.push(SmilPar {
                text_href: path,
                text_fragment: frag,
                audio_href: resolve_href(smil_href, &audio_src),
                clip_begin: begin,
                clip_end: end,
            });
        }
        search = &rest[close.max(1)..];
    }
    pars
}

fn opf_path_from_container(container: &str) -> Option<String> {
    let mut search = container;
    while let Some(start) = search.find("<rootfile") {
        let rest = &search[start..];
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        if let Some(full) = attr_owned(tag, "full-path") {
            return Some(full.replace('\\', "/"));
        }
        search = &rest[end.max(1)..];
    }
    None
}

fn playlist_from_archive<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    book_id: i64,
    file_id: Option<i64>,
) -> ApiResult<Value> {
    let container = zip_string(archive, "META-INF/container.xml")?;
    let opf_path = opf_path_from_container(&container).ok_or(ApiError(
        StatusCode::BAD_REQUEST,
        "This EPUB is missing a package document.",
    ))?;
    let opf = zip_string(archive, &opf_path)?;
    let (manifest, spine) = parse_manifest_and_spine(&opf);
    let by_id: BTreeMap<&str, &ManifestItem> = manifest
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect();

    let mut items_out = Vec::new();
    let mut sections_out = Vec::new();
    let mut resources: BTreeMap<String, (String, u64)> = BTreeMap::new();
    let mut global_seconds = 0.0;
    let mut item_index = 0usize;

    for (section_index, idref) in spine.iter().enumerate() {
        let Some(spine_item) = by_id.get(idref.as_str()) else {
            continue;
        };
        let Some(mo_id) = spine_item.media_overlay.as_deref() else {
            continue;
        };
        let Some(mo_item) = by_id.get(mo_id) else {
            continue;
        };
        let smil_href = resolve_href(&opf_path, &mo_item.href);
        let Ok(smil) = zip_string(archive, &smil_href) else {
            continue;
        };
        let pars = parse_smil_pars(&smil, &smil_href);
        if pars.is_empty() {
            continue;
        }
        let section_start = global_seconds;
        let mut section_duration = 0.0;
        let text_href = resolve_href(&opf_path, &spine_item.href);
        for par in &pars {
            let duration = match par.clip_end {
                Some(end) if end > par.clip_begin => Some(end - par.clip_begin),
                _ => None,
            };
            if let Some(d) = duration {
                section_duration += d;
            }
            let audio_mime = resources
                .get(&par.audio_href)
                .map(|(mime, _)| mime.clone())
                .unwrap_or_else(|| {
                    manifest
                        .iter()
                        .find(|m| resolve_href(&opf_path, &m.href) == par.audio_href)
                        .map(|m| m.media_type.clone())
                        .filter(|m| !m.is_empty())
                        .unwrap_or_else(|| "audio/mpeg".to_string())
                });
            let size = resources
                .get(&par.audio_href)
                .map(|(_, size)| *size)
                .unwrap_or_else(|| zip_size(archive, &par.audio_href));
            resources.insert(par.audio_href.clone(), (audio_mime.clone(), size));
            items_out.push(json!({
                "index": item_index,
                "sectionIndex": section_index,
                "smilHref": smil_href,
                "textHref": par.text_href,
                "textFragment": par.text_fragment,
                "audioHref": par.audio_href,
                "audioMimeType": audio_mime,
                "clipBeginSeconds": par.clip_begin,
                "clipEndSeconds": par.clip_end,
                "durationSeconds": duration,
                "label": par.text_fragment,
            }));
            item_index += 1;
        }
        global_seconds += section_duration;
        sections_out.push(json!({
            "index": section_index,
            "href": text_href,
            "label": null,
            "smilHref": smil_href,
            "startSeconds": section_start,
            "durationSeconds": if section_duration > 0.0 { Value::from(section_duration) } else { Value::Null },
        }));
    }

    let resources_out: Vec<Value> = resources
        .into_iter()
        .map(|(href, (media_type, size))| {
            json!({
                "href": href,
                "mediaType": media_type,
                "size": size,
            })
        })
        .collect();

    Ok(json!({
        "bookId": book_id,
        "fileId": file_id.unwrap_or(book_id),
        "durationSeconds": if global_seconds > 0.0 { Value::from(global_seconds) } else { Value::Null },
        "items": items_out,
        "sections": sections_out,
        "resources": resources_out,
    }))
}

pub(crate) fn playlist_from_epub(
    content: &[u8],
    book_id: i64,
    file_id: Option<i64>,
) -> ApiResult<Value> {
    let mut archive = ZipArchive::new(Cursor::new(content))
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Could not read this EPUB."))?;
    playlist_from_archive(&mut archive, book_id, file_id)
}

fn playlist_from_epub_path(
    absolute: &Path,
    book_id: i64,
    file_id: Option<i64>,
) -> ApiResult<Value> {
    let file = std::fs::File::open(absolute).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read the on-disk book file.",
        )
    })?;
    let mut archive = ZipArchive::new(file)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Could not read this EPUB."))?;
    playlist_from_archive(&mut archive, book_id, file_id)
}

/// `GET /api/v1/epub/{id}/media-overlay?fileId=`
pub(super) async fn media_overlay_playlist(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<i64>,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let file_id = query
        .get("fileId")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(id);
    let data_dir = state.data_dir.clone();
    let (inline, storage_path, format) = state
        .db({
            let headers = headers.clone();
            move |db| {
                let user = authenticate(db, &headers)?;
                owned_book_file_row(db, user, id)
            }
        })
        .await?;
    if format != "epub" {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Media overlays are only available for EPUB books.",
        ));
    }
    // Parse SMIL/OPF only — never inflate multi‑GB audio into RAM for the playlist.
    let playlist = if let Some(relative) = storage_path.filter(|p| !p.is_empty()) {
        if relative.contains("..") || PathBuf::from(&relative).is_absolute() {
            return Err(ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid on-disk book path.",
            ));
        }
        let absolute = data_dir.join(relative);
        tokio::task::spawn_blocking(move || playlist_from_epub_path(&absolute, id, Some(file_id)))
            .await
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Request failed."))??
    } else {
        tokio::task::spawn_blocking(move || playlist_from_epub(&inline, id, Some(file_id)))
            .await
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Request failed."))??
    };
    Ok(Json(playlist))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_values_match_foliate() {
        assert_eq!(parse_clock("12.5"), Some(12.5));
        assert_eq!(parse_clock("12.5s"), Some(12.5));
        assert_eq!(parse_clock("500ms"), Some(0.5));
        assert_eq!(parse_clock("1:02.5"), Some(62.5));
        assert_eq!(parse_clock("1:01:02"), Some(3662.0));
        assert_eq!(parse_clock("npt=3s"), Some(3.0));
    }

    #[test]
    fn resolve_relative_audio_paths() {
        assert_eq!(
            resolve_href("OEBPS/chapter1.smil", "audio/ch1.mp3"),
            "OEBPS/audio/ch1.mp3"
        );
        assert_eq!(
            resolve_href("OEBPS/smil/ch.smil", "../audio/a.mp3"),
            "OEBPS/audio/a.mp3"
        );
    }
}
