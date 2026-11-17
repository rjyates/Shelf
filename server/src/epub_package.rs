//! Book Orbit package-file streaming for Foliate (`/info` + `/file/{*path}`).
//! Lets the web reader fetch EPUB zip entries on demand instead of the whole blob.

use super::media_overlay::{
    attr_owned, opf_path_from_container, parse_manifest_and_spine_refs, resolve_href, zip_bytes,
    zip_entry_name, zip_size, zip_string, ManifestItem,
};
use super::*;
use axum::body::Body;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::io::{Cursor, Read, Seek};
use std::path::{Component, Path as FsPath, PathBuf};
use zip::ZipArchive;

const OPTIONAL_META_INF: &[&str] = &[
    "META-INF/encryption.xml",
    "META-INF/com.apple.ibooks.display-options.xml",
    "META-INF/com.kobobooks.display-options.xml",
    "META-INF/calibre_bookmarks.txt",
];

fn normalize_zip_path(path: &str) -> Option<String> {
    let cleaned = path.replace('\\', "/");
    if cleaned.is_empty() {
        return None;
    }
    let mut out = PathBuf::new();
    for component in FsPath::new(&cleaned).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    let s = out.to_string_lossy().replace('\\', "/");
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn decode_package_path(raw: &str) -> ApiResult<String> {
    let decoded = raw
        .split('/')
        .map(|segment| {
            percent_decode(segment).ok_or(ApiError(
                StatusCode::BAD_REQUEST,
                "Invalid package file path.",
            ))
        })
        .collect::<ApiResult<Vec<_>>>()?
        .join("/");
    normalize_zip_path(&decoded).ok_or(ApiError(
        StatusCode::BAD_REQUEST,
        "Invalid package file path.",
    ))
}

fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = from_hex(bytes[i + 1])?;
                let lo = from_hex(bytes[i + 2])?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn guess_content_type(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "xhtml" | "html" | "htm" => "application/xhtml+xml",
        "css" => "text/css",
        "js" => "application/javascript",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "xml" => "application/xml",
        "ncx" => "application/x-dtbncx+xml",
        "opf" => "application/oebps-package+xml",
        "smil" => "application/smil+xml",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "opus" | "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

fn dc_text(opf: &str, local: &str) -> Option<String> {
    let open = format!("<dc:{local}");
    let start = opf.find(&open)?;
    let after = &opf[start..];
    let gt = after.find('>')?;
    let rest = &after[gt + 1..];
    let close = format!("</dc:{local}>");
    let end = rest.find(&close)?;
    let text = rest[..end].trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn cover_from_meta(
    opf: &str,
    by_id: &std::collections::BTreeMap<&str, &ManifestItem>,
) -> Option<String> {
    let mut search = opf;
    while let Some(start) = search.find("<meta") {
        let rest = &search[start..];
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        if attr_owned(tag, "name").as_deref() == Some("cover") {
            if let Some(content) = attr_owned(tag, "content") {
                return by_id.get(content.as_str()).map(|m| m.href.clone());
            }
        }
        search = &rest[end.max(1)..];
    }
    None
}

fn parse_nav_toc(nav: &str, nav_href: &str) -> Option<Value> {
    let nav_start = nav
        .find("epub:type=\"toc\"")
        .or_else(|| nav.find("epub:type='toc'"))?;
    let slice = &nav[nav_start..];
    let ol_start = slice.find("<ol")?;
    let ol = &slice[ol_start..];
    let children = parse_toc_list(ol, nav_href);
    if children.is_empty() {
        return None;
    }
    Some(json!({
        "label": "",
        "children": children,
    }))
}

fn parse_toc_list(ol: &str, base: &str) -> Vec<Value> {
    let mut items = Vec::new();
    let mut search = ol;
    // Only walk top-level `<li>` under this `<ol>` by scanning until the matching close.
    let body_end = find_matching_close(ol, "ol").unwrap_or(ol.len());
    while let Some(li_at) = search.find("<li") {
        if li_at >= body_end {
            break;
        }
        let rest = &search[li_at..];
        let li_end = find_matching_close(rest, "li").unwrap_or(rest.len());
        let block = &rest[..li_end];
        if let Some(a_at) = block.find("<a") {
            let a_tag_end = block[a_at..].find('>').map(|e| a_at + e).unwrap_or(a_at);
            let href = attr_owned(&block[a_at..a_tag_end], "href");
            let label_start = a_tag_end + 1;
            let label_end = block[label_start..]
                .find("</a>")
                .map(|e| label_start + e)
                .unwrap_or(block.len());
            let label = block[label_start..label_end].trim().to_string();
            if !label.is_empty() {
                let mut item = Map::new();
                item.insert("label".into(), Value::String(label));
                if let Some(href) = href {
                    let path = href.split('#').next().unwrap_or(&href);
                    let resolved = resolve_href(base, path);
                    let with_frag = if let Some((_, frag)) = href.split_once('#') {
                        format!("{resolved}#{frag}")
                    } else {
                        resolved
                    };
                    item.insert("href".into(), Value::String(with_frag));
                }
                if let Some(nested_ol) = block.find("<ol") {
                    let nested = parse_toc_list(&block[nested_ol..], base);
                    if !nested.is_empty() {
                        item.insert("children".into(), Value::Array(nested));
                    }
                }
                items.push(Value::Object(item));
            }
        }
        search = &rest[li_end.max(1)..];
    }
    items
}

fn find_matching_close(src: &str, tag: &str) -> Option<usize> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut depth = 0usize;
    let mut i = 0usize;
    while i < src.len() {
        if src[i..].starts_with(&open) {
            depth += 1;
            i += open.len();
            continue;
        }
        if src[i..].starts_with(&close) {
            depth = depth.saturating_sub(1);
            i += close.len();
            if depth == 0 {
                return Some(i);
            }
            continue;
        }
        i += 1;
    }
    None
}

struct PackageInfo {
    info: Value,
    valid_paths: HashSet<String>,
}

fn package_info_from_archive<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> ApiResult<PackageInfo> {
    let container = zip_string(archive, "META-INF/container.xml")?;
    let opf_path = opf_path_from_container(&container).ok_or(ApiError(
        StatusCode::BAD_REQUEST,
        "This EPUB is missing a package document.",
    ))?;
    let opf_path = normalize_zip_path(&opf_path).ok_or(ApiError(
        StatusCode::BAD_REQUEST,
        "This EPUB is missing a package document.",
    ))?;
    let root_path = opf_path
        .rfind('/')
        .map(|i| opf_path[..=i].to_string())
        .unwrap_or_default();
    let opf = zip_string(archive, &opf_path)?;
    let (manifest_items, spine_refs) = parse_manifest_and_spine_refs(&opf);
    let by_id: std::collections::BTreeMap<&str, &ManifestItem> = manifest_items
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect();

    let mut valid_paths = HashSet::new();
    valid_paths.insert("META-INF/container.xml".to_string());
    valid_paths.insert(opf_path.clone());

    let mut manifest_out = Vec::new();
    for item in &manifest_items {
        let full_href = normalize_zip_path(&resolve_href(&opf_path, &item.href))
            .unwrap_or_else(|| resolve_href(&opf_path, &item.href));
        valid_paths.insert(full_href.clone());
        let mut entry = json!({
            "id": item.id,
            "href": full_href,
            "mediaType": if item.media_type.is_empty() {
                guess_content_type(&item.href).to_string()
            } else {
                item.media_type.clone()
            },
            "size": zip_size(archive, &full_href),
        });
        if let Some(props) = &item.properties {
            entry["properties"] = json!(props);
        }
        if let Some(mo) = &item.media_overlay {
            entry["mediaOverlay"] = json!(mo);
        }
        manifest_out.push(entry);
    }

    let mut spine_out = Vec::new();
    for spine in &spine_refs {
        let Some(item) = by_id.get(spine.idref.as_str()) else {
            continue;
        };
        let full_href = normalize_zip_path(&resolve_href(&opf_path, &item.href))
            .unwrap_or_else(|| resolve_href(&opf_path, &item.href));
        spine_out.push(json!({
            "idref": spine.idref,
            "href": full_href,
            "mediaType": if item.media_type.is_empty() {
                guess_content_type(&item.href).to_string()
            } else {
                item.media_type.clone()
            },
            "linear": spine.linear,
        }));
    }

    let optional_files: Vec<String> = OPTIONAL_META_INF
        .iter()
        .filter_map(|path| zip_entry_name(archive, path).map(|_| (*path).to_string()))
        .collect();
    for path in &optional_files {
        valid_paths.insert(path.clone());
    }

    let mut metadata = Map::new();
    if let Some(title) = dc_text(&opf, "title") {
        metadata.insert("title".into(), Value::String(title));
    }
    if let Some(creator) = dc_text(&opf, "creator") {
        metadata.insert("creator".into(), Value::String(creator));
    }
    if let Some(language) = dc_text(&opf, "language") {
        metadata.insert("language".into(), Value::String(language));
    }
    if let Some(publisher) = dc_text(&opf, "publisher") {
        metadata.insert("publisher".into(), Value::String(publisher));
    }
    if let Some(identifier) = dc_text(&opf, "identifier") {
        metadata.insert("identifier".into(), Value::String(identifier));
    }

    let cover_path = manifest_items
        .iter()
        .find(|m| {
            m.properties
                .as_ref()
                .is_some_and(|p| p.iter().any(|x| x == "cover-image"))
        })
        .map(|m| {
            normalize_zip_path(&resolve_href(&opf_path, &m.href))
                .unwrap_or_else(|| resolve_href(&opf_path, &m.href))
        })
        .or_else(|| {
            cover_from_meta(&opf, &by_id).map(|href| {
                normalize_zip_path(&resolve_href(&opf_path, &href))
                    .unwrap_or_else(|| resolve_href(&opf_path, &href))
            })
        });

    let toc = manifest_items
        .iter()
        .find(|m| {
            m.properties
                .as_ref()
                .is_some_and(|p| p.iter().any(|x| x == "nav"))
        })
        .and_then(|nav_item| {
            let nav_href = normalize_zip_path(&resolve_href(&opf_path, &nav_item.href))
                .unwrap_or_else(|| resolve_href(&opf_path, &nav_item.href));
            zip_string(archive, &nav_href)
                .ok()
                .and_then(|nav| parse_nav_toc(&nav, &nav_href))
        });

    Ok(PackageInfo {
        info: json!({
            "containerPath": opf_path,
            "rootPath": root_path,
            "spine": spine_out,
            "manifest": manifest_out,
            "optionalFiles": optional_files,
            "toc": toc,
            "metadata": metadata,
            "coverPath": cover_path,
        }),
        valid_paths,
    })
}

fn package_info_from_bytes(content: &[u8]) -> ApiResult<PackageInfo> {
    let mut archive = ZipArchive::new(Cursor::new(content))
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Could not read this EPUB."))?;
    package_info_from_archive(&mut archive)
}

fn package_info_from_path(absolute: &FsPath) -> ApiResult<PackageInfo> {
    let file = std::fs::File::open(absolute).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read the on-disk book file.",
        )
    })?;
    let mut archive = ZipArchive::new(file)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Could not read this EPUB."))?;
    package_info_from_archive(&mut archive)
}

fn entry_bytes_from_archive<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
    valid_paths: &HashSet<String>,
) -> ApiResult<(Vec<u8>, String)> {
    let normalized = normalize_zip_path(path).ok_or(ApiError(
        StatusCode::BAD_REQUEST,
        "Invalid package file path.",
    ))?;
    if !valid_paths.contains(&normalized) {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "Package file not found in this EPUB.",
        ));
    }
    let bytes = zip_bytes(archive, &normalized).map_err(|_| {
        ApiError(
            StatusCode::NOT_FOUND,
            "Package file not found in this EPUB.",
        )
    })?;
    Ok((bytes, normalized))
}

fn entry_from_bytes(
    content: &[u8],
    path: &str,
    valid_paths: &HashSet<String>,
) -> ApiResult<(Vec<u8>, String)> {
    let mut archive = ZipArchive::new(Cursor::new(content))
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Could not read this EPUB."))?;
    entry_bytes_from_archive(&mut archive, path, valid_paths)
}

fn entry_from_path(
    absolute: &FsPath,
    path: &str,
    valid_paths: &HashSet<String>,
) -> ApiResult<(Vec<u8>, String)> {
    let file = std::fs::File::open(absolute).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read the on-disk book file.",
        )
    })?;
    let mut archive = ZipArchive::new(file)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Could not read this EPUB."))?;
    entry_bytes_from_archive(&mut archive, path, valid_paths)
}

/// `GET /api/v1/epub/{id}/info?fileId=`
pub(super) async fn book_info(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    axum::extract::Query(_query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
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
            "Package streaming is only available for EPUB books.",
        ));
    }
    let info = if let Some(relative) = storage_path.filter(|p| !p.is_empty()) {
        if relative.contains("..") || PathBuf::from(&relative).is_absolute() {
            return Err(ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid on-disk book path.",
            ));
        }
        let absolute = data_dir.join(relative);
        tokio::task::spawn_blocking(move || package_info_from_path(&absolute))
            .await
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Request failed."))??
            .info
    } else {
        tokio::task::spawn_blocking(move || package_info_from_bytes(&inline))
            .await
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Request failed."))??
            .info
    };
    Ok(Json(info))
}

/// `GET /api/v1/epub/{id}/file/{*path}?fileId=`
pub(super) async fn package_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, raw_path)): Path<(i64, String)>,
    axum::extract::Query(_query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> ApiResult<Response> {
    let path = decode_package_path(&raw_path)?;
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
            "Package streaming is only available for EPUB books.",
        ));
    }

    let (bytes, resolved) = if let Some(relative) = storage_path.filter(|p| !p.is_empty()) {
        if relative.contains("..") || PathBuf::from(&relative).is_absolute() {
            return Err(ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid on-disk book path.",
            ));
        }
        let absolute = data_dir.join(relative);
        let path = path.clone();
        tokio::task::spawn_blocking(move || {
            let packaged = package_info_from_path(&absolute)?;
            entry_from_path(&absolute, &path, &packaged.valid_paths)
        })
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Request failed."))??
    } else {
        let path = path.clone();
        tokio::task::spawn_blocking(move || {
            let packaged = package_info_from_bytes(&inline)?;
            entry_from_bytes(&inline, &path, &packaged.valid_paths)
        })
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Request failed."))??
    };

    let content_type = guess_content_type(&resolved);
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(content_type),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("private, max-age=3600"),
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn fixture_epub() -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in [
            ("mimetype", "application/epub+zip".as_bytes()),
            (
                "META-INF/container.xml",
                br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OEBPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            ),
            (
                "OEBPS/package.opf",
                br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">stream</dc:identifier><dc:title>Stream Me</dc:title><dc:creator>Shelf</dc:creator><dc:language>en-US</dc:language></metadata><manifest><item id="chapter" href="text/chapter.xhtml" media-type="application/xhtml+xml"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest><spine><itemref idref="chapter"/></spine></package>"#,
            ),
            (
                "OEBPS/text/chapter.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Hi</p></body></html>"#,
            ),
            (
                "OEBPS/nav.xhtml",
                br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="text/chapter.xhtml">Chapter</a></li></ol></nav></body></html>"#,
            ),
            ("META-INF/encryption.xml", br#"<encryption/>"#),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn info_resolves_root_relative_hrefs_and_optional_files() {
        let info = package_info_from_bytes(&fixture_epub()).unwrap();
        assert_eq!(info.info["containerPath"], "OEBPS/package.opf");
        assert_eq!(info.info["rootPath"], "OEBPS/");
        assert_eq!(info.info["metadata"]["language"], "en-US");
        assert_eq!(info.info["spine"][0]["href"], "OEBPS/text/chapter.xhtml");
        assert_eq!(info.info["manifest"][0]["href"], "OEBPS/text/chapter.xhtml");
        assert!(info.info["optionalFiles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "META-INF/encryption.xml"));
        assert_eq!(info.info["toc"]["children"][0]["label"], "Chapter");
        assert!(info.valid_paths.contains("OEBPS/text/chapter.xhtml"));
    }

    #[test]
    fn package_file_rejects_unknown_and_parent_paths() {
        let bytes = fixture_epub();
        let packaged = package_info_from_bytes(&bytes).unwrap();
        assert!(entry_from_bytes(&bytes, "../etc/passwd", &packaged.valid_paths).is_err());
        assert!(entry_from_bytes(&bytes, "missing.xhtml", &packaged.valid_paths).is_err());
        let (chapter, path) =
            entry_from_bytes(&bytes, "OEBPS/text/chapter.xhtml", &packaged.valid_paths).unwrap();
        assert_eq!(path, "OEBPS/text/chapter.xhtml");
        assert!(String::from_utf8_lossy(&chapter).contains("<p>Hi</p>"));
    }

    #[test]
    fn decode_preserves_literal_percent_after_single_decode() {
        // Axum may already decode once; a second pass must still accept plain paths.
        assert_eq!(
            decode_package_path("OEBPS/text/chapter.xhtml").unwrap(),
            "OEBPS/text/chapter.xhtml"
        );
        assert_eq!(
            decode_package_path("OEBPS/text/chapter%201.xhtml").unwrap(),
            "OEBPS/text/chapter 1.xhtml"
        );
    }
}
