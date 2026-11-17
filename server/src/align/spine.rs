//! EPUB spine discovery for multi-chapter align.
//!
//! Lists every spine itemref (XHTML and other media). Exclusions classify
//! nav/cover/toc — this module does not hard-filter by filename.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use regex::Regex;

use crate::align::exclusions::SpineItemMeta;

/// Compat alias — same as [`SpineItemMeta`].
pub type SpineChapter = SpineItemMeta;

#[derive(Debug, Clone)]
struct ManifestEntry {
    href: String,
    media_type: String,
    properties: String,
}

/// All spine itemrefs in order, with linear / media-type / properties / index.
pub fn list_spine_chapters(epub: &Path) -> Result<Vec<SpineItemMeta>, String> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(epub).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let container = read_zip_str(&mut archive, "META-INF/container.xml")?;
    let opf_href = {
        let re = Regex::new(r#"full-path="([^"]+)""#).unwrap();
        re.captures(&container)
            .and_then(|c| c.get(1).map(|m| m.as_str().replace('\\', "/")))
            .ok_or_else(|| "OPF path missing from container".to_string())?
    };
    let opf = read_zip_str(&mut archive, &opf_href)?;
    let opf_dir = Path::new(&opf_href)
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();

    let item_re = Regex::new(r#"(?is)<item\b([^>]*?)/?>"#).unwrap();
    let mut id_to_item: HashMap<String, ManifestEntry> = HashMap::new();
    for caps in item_re.captures_iter(&opf) {
        let attrs = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let id = attr(attrs, "id");
        let href = attr(attrs, "href");
        let media_type = attr(attrs, "media-type").unwrap_or_default();
        let properties = attr(attrs, "properties").unwrap_or_default();
        if let (Some(id), Some(href)) = (id, href) {
            id_to_item.insert(
                id,
                ManifestEntry {
                    href,
                    media_type,
                    properties,
                },
            );
        }
    }

    let spine_re = Regex::new(r#"(?is)<itemref\b([^>]*?)/?>"#).unwrap();
    let mut refs: Vec<(String, bool)> = Vec::new();
    for caps in spine_re.captures_iter(&opf) {
        let attrs = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let Some(id) = attr(attrs, "idref") else {
            continue;
        };
        let linear = attr(attrs, "linear")
            .map(|v| !v.eq_ignore_ascii_case("no"))
            .unwrap_or(true);
        refs.push((id, linear));
    }
    if refs.is_empty() {
        return Err("No spine itemrefs found".into());
    }

    let spine_len = refs.len();
    let mut out = Vec::with_capacity(spine_len);
    for (index, (id, linear)) in refs.into_iter().enumerate() {
        let Some(entry) = id_to_item.get(&id) else {
            continue;
        };
        let joined = if opf_dir.is_empty() {
            entry.href.clone()
        } else {
            format!("{opf_dir}/{}", entry.href).replace("//", "/")
        };
        out.push(SpineItemMeta {
            index,
            spine_len,
            id,
            href: joined,
            linear,
            media_type: if entry.media_type.is_empty() {
                "application/xhtml+xml".into()
            } else {
                entry.media_type.clone()
            },
            properties: entry.properties.clone(),
        });
    }
    if out.is_empty() {
        return Err("No spine items resolved from manifest".into());
    }
    Ok(out)
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(r#"{name}\s*=\s*"([^"]+)""#)).ok()?;
    re.captures(attrs)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

fn read_zip_str<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<String, String> {
    let mut file = archive
        .by_name(name)
        .map_err(|_| format!("Missing {name}"))?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    #[test]
    fn lists_all_spine_itemrefs_including_nav() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.epub");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default();
        zip.start_file("META-INF/container.xml", opts).unwrap();
        zip.write_all(br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        zip.start_file("OEBPS/content.opf", opts).unwrap();
        zip.write_all(br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="uid">x</dc:identifier><dc:title>T</dc:title><dc:language>en</dc:language></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="ch2.xhtml" media-type="application/xhtml+xml"/><item id="img" href="plate.jpg" media-type="image/jpeg"/></manifest><spine><itemref idref="nav"/><itemref idref="c1"/><itemref idref="c2" linear="no"/><itemref idref="img"/></spine></package>"#).unwrap();
        zip.start_file("OEBPS/nav.xhtml", opts).unwrap();
        zip.write_all(b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body/></html>")
            .unwrap();
        zip.start_file("OEBPS/ch1.xhtml", opts).unwrap();
        zip.write_all(
            b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>One. Two.</p></body></html>",
        )
        .unwrap();
        zip.start_file("OEBPS/ch2.xhtml", opts).unwrap();
        zip.write_all(
            b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>Three. Four.</p></body></html>",
        )
        .unwrap();
        zip.start_file("OEBPS/plate.jpg", opts).unwrap();
        zip.write_all(&[0xff, 0xd8, 0xff, 0xd9]).unwrap();
        zip.finish().unwrap();

        let chapters = list_spine_chapters(&path).unwrap();
        assert_eq!(chapters.len(), 4);
        assert_eq!(chapters[0].id, "nav");
        assert_eq!(chapters[0].href, "OEBPS/nav.xhtml");
        assert!(chapters[0]
            .properties
            .split_whitespace()
            .any(|p| p == "nav"));
        assert_eq!(chapters[1].href, "OEBPS/ch1.xhtml");
        assert!(chapters[1].linear);
        assert!(!chapters[2].linear);
        assert_eq!(chapters[3].media_type, "image/jpeg");
        assert_eq!(chapters[0].spine_len, 4);
        assert_eq!(chapters[3].index, 3);
    }
}
