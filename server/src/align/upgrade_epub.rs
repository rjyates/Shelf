//! EPUB 2 → 3 upgrade (Storyteller `Epub.upgrade` / `upgrade.tsx`).
//!
//! Called on Combine `source.epub` before spine/align. EPUB 3.x is a no-op.

use std::collections::HashMap;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use xmltree::{Element, EmitterConfig, XMLNode};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

const OPF_NS: &str = "http://www.idpf.org/2007/opf";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
const NCX_MIME: &str = "application/x-dtbncx+xml";
const XHTML_MIME: &str = "application/xhtml+xml";

/// Guide `type` → EPUB3 `epub:type` (ST `GUIDE_TO_EPUBTYPE`). Empty string = skip.
const GUIDE_TO_EPUBTYPE: &[(&str, &str)] = &[
    ("acknowledgements", "acknowledgments"),
    ("other.afterword", "afterword"),
    ("other.appendix", "appendix"),
    ("other.backmatter", "backmatter"),
    ("bibliography", "bibliography"),
    ("text", "bodymatter"),
    ("other.chapter", "chapter"),
    ("colophon", "colophon"),
    ("other.conclusion", "conclusion"),
    ("other.contributors", "contributors"),
    ("copyright-page", "copyright-page"),
    ("cover", "cover"),
    ("dedication", "dedication"),
    ("other.division", "division"),
    ("epigraph", "epigraph"),
    ("other.epilogue", "epilogue"),
    ("other.errata", "errata"),
    ("other.footnotes", "footnotes"),
    ("foreword", "foreword"),
    ("other.frontmatter", "frontmatter"),
    ("glossary", "glossary"),
    ("other.halftitlepage", "halftitlepage"),
    ("other.imprint", "imprint"),
    ("other.imprimatur", "imprimatur"),
    ("index", "index"),
    ("other.introduction", "introduction"),
    ("other.landmarks", "landmarks"),
    ("other.loa", "loa"),
    ("loi", "loi"),
    ("lot", "lot"),
    ("other.lov", "lov"),
    ("notes", ""),
    ("other.notice", "notice"),
    ("other.other-credits", "other-credits"),
    ("other.part", "part"),
    ("other.preamble", "preamble"),
    ("preface", "preface"),
    ("other.prologue", "prologue"),
    ("other.rearnotes", "rearnotes"),
    ("other.subchapter", "subchapter"),
    ("title-page", "titlepage"),
    ("toc", "toc"),
    ("other.volume", "volume"),
    ("other.warning", "warning"),
];

#[derive(Debug, Clone)]
struct Landmark {
    href: String,
    title: String,
    type_: String,
}

#[derive(Debug, Clone)]
struct NavEntry {
    title: String,
    href: Option<String>,
    children: Vec<NavEntry>,
}

/// Upgrade EPUB 2 → 3 in place (ST `upgradeEpub`, default `removeNcx=false`).
/// Returns `true` when the package was rewritten; `false` when already 3.x.
pub fn upgrade_epub_in_place(path: &Path, remove_ncx: bool) -> Result<bool, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(Cursor::new(data)).map_err(|e| e.to_string())?;

    let mut entries: HashMap<String, Vec<u8>> = HashMap::new();
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().replace('\\', "/");
        if name.ends_with('/') {
            continue;
        }
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        entries.insert(name, buf);
    }

    let opf_name = find_opf_path(&entries)?;
    let opf_dir = opf_name.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let opf_body = std::str::from_utf8(
        entries
            .get(&opf_name)
            .ok_or_else(|| "OPF missing from zip".to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let mut package =
        Element::parse(opf_body.as_bytes()).map_err(|e| format!("OPF XML parse: {e}"))?;

    let version = package
        .attributes
        .get("version")
        .map(|v| v.as_str())
        .unwrap_or("");
    if version.starts_with("3.") {
        return Ok(false);
    }

    let toc_entries = parse_ncx_toc(&package, &entries, opf_dir);
    let landmarks = extract_guide_landmarks(&package);

    upgrade_package_metadata(&mut package);
    fix_font_mime_types(&mut package);
    remove_guide(&mut package);
    if remove_ncx {
        remove_spine_toc_ref(&mut package);
    }
    package.attributes.insert("version".into(), "3.0".into());

    // ST collectManifestProperties — EPUB3 early-return skips this (same as ST).
    collect_manifest_properties(&mut package, &entries, opf_dir);

    let nav_href = choose_nav_href(&package);
    let first_spine_href = first_spine_href(&package).unwrap_or_else(|| "#".into());
    let nav_xml = build_nav_document(&toc_entries, &landmarks, &first_spine_href);

    let opf_ns = package.namespace.clone();
    if let Some(manifest) = find_child_mut(&mut package, "manifest") {
        let mut item = Element::new("item");
        item.namespace = opf_ns.clone();
        item.attributes.insert("id".into(), "nav".into());
        item.attributes.insert("href".into(), nav_href.clone());
        item.attributes
            .insert("media-type".into(), XHTML_MIME.into());
        item.attributes.insert("properties".into(), "nav".into());
        manifest.children.push(XMLNode::Element(item));
    }

    let mut ncx_zip_path: Option<String> = None;
    if remove_ncx {
        ncx_zip_path = take_ncx_manifest_item(&mut package, opf_dir);
        // also drop spine toc already handled
    }

    // Normalize XHTML doctypes in zip entries (ST normalizeDoctype).
    let xhtml_hrefs = xhtml_manifest_hrefs(&package);
    for rel in &xhtml_hrefs {
        let zip_path = join_opf(opf_dir, rel);
        if let Some(buf) = entries.get_mut(&zip_path) {
            if let Ok(s) = std::str::from_utf8(buf) {
                *buf = normalize_doctype(s).into_bytes();
            }
        }
    }

    let nav_zip = join_opf(opf_dir, &nav_href);
    entries.insert(nav_zip, nav_xml.into_bytes());

    if let Some(ncx) = ncx_zip_path {
        entries.remove(&ncx);
    }

    let mut opf_out = Vec::new();
    let cfg = EmitterConfig::new()
        .perform_indent(true)
        .write_document_declaration(true);
    package
        .write_with_config(&mut opf_out, cfg)
        .map_err(|e| format!("OPF XML write: {e}"))?;
    entries.insert(opf_name, opf_out);

    write_zip(path, &entries)?;
    Ok(true)
}

fn find_opf_path(entries: &HashMap<String, Vec<u8>>) -> Result<String, String> {
    if let Some(container) = entries.get("META-INF/container.xml") {
        if let Ok(s) = std::str::from_utf8(container) {
            if let Some(full) = attr_after(s, "full-path=") {
                let path = full.replace('\\', "/");
                if entries.contains_key(&path) {
                    return Ok(path);
                }
            }
        }
    }
    entries
        .keys()
        .find(|k| k.ends_with(".opf"))
        .cloned()
        .ok_or_else(|| "No OPF in EPUB".into())
}

fn attr_after(s: &str, key: &str) -> Option<String> {
    let i = s.find(key)?;
    let rest = &s[i + key.len()..];
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let end = rest[1..].find(quote)?;
    Some(rest[1..1 + end].to_string())
}

fn join_opf(opf_dir: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or(href).replace('\\', "/");
    if opf_dir.is_empty() {
        href
    } else {
        format!("{opf_dir}/{href}").replace("//", "/")
    }
}

fn resolve_relative(base_href: &str, src: &str) -> String {
    let (src_path, frag) = match src.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (src, None),
    };
    if src_path.is_empty() {
        return src.to_string();
    }
    if src_path.starts_with('/') {
        let p = src_path.trim_start_matches('/');
        return match frag {
            Some(f) => format!("{p}#{f}"),
            None => p.to_string(),
        };
    }
    let base_dir = Path::new(base_href)
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let joined = if base_dir.is_empty() {
        PathBuf::from(src_path)
    } else {
        PathBuf::from(&base_dir).join(src_path)
    };
    let mut stack: Vec<String> = Vec::new();
    for c in joined.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                stack.pop();
            }
            std::path::Component::Normal(s) => stack.push(s.to_string_lossy().into_owned()),
            _ => {}
        }
    }
    let cleaned = stack.join("/");
    match frag {
        Some(f) => format!("{cleaned}#{f}"),
        None => cleaned,
    }
}

fn find_child_mut<'a>(parent: &'a mut Element, name: &str) -> Option<&'a mut Element> {
    parent.children.iter_mut().find_map(|n| match n {
        XMLNode::Element(e) if e.name == name => Some(e),
        _ => None,
    })
}

fn find_child<'a>(parent: &'a Element, name: &str) -> Option<&'a Element> {
    parent.children.iter().find_map(|n| match n {
        XMLNode::Element(e) if e.name == name => Some(e),
        _ => None,
    })
}

fn element_text(el: &Element) -> String {
    el.get_text().map(|c| c.into_owned()).unwrap_or_default()
}

fn ensure_id(el: &mut Element) -> String {
    if let Some(id) = el.attributes.get("id") {
        if !id.is_empty() {
            return id.clone();
        }
    }
    let seed = element_text(el);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut h);
    el.name.hash(&mut h);
    let id = format!("id-{:08x}", h.finish() as u32);
    el.attributes.insert("id".into(), id.clone());
    id
}

fn meta_element(
    property: &str,
    refines: Option<&str>,
    text: &str,
    opf_ns: Option<&str>,
) -> Element {
    let mut meta = Element::new("meta");
    meta.namespace = opf_ns.map(|s| s.to_string());
    meta.attributes.insert("property".into(), property.into());
    if let Some(r) = refines {
        meta.attributes.insert("refines".into(), r.into());
    }
    meta.children.push(XMLNode::Text(text.to_string()));
    meta
}

fn upgrade_package_metadata(pkg: &mut Element) {
    upgrade_identifiers(pkg);
    upgrade_title(pkg);
    upgrade_languages(pkg);
    upgrade_authors(pkg);
    upgrade_date(pkg);
    upgrade_meta(pkg);
    upgrade_cover(pkg);
    remove_invalid_dc_attrs(pkg);
    set_last_modified(pkg);
}

fn upgrade_identifiers(pkg: &mut Element) {
    let Some(metadata) = find_child_mut(pkg, "metadata") else {
        return;
    };
    for node in metadata.children.iter_mut() {
        let XMLNode::Element(ident) = node else {
            continue;
        };
        if ident.name != "identifier" {
            continue;
        }
        let mut val = element_text(ident);
        let mut scheme = ident.attributes.get("scheme").cloned().or_else(|| {
            // xmltree may keep Clark notation for namespaced attrs
            ident
                .attributes
                .iter()
                .find(|(k, _)| k.ends_with("}scheme") || *k == "scheme")
                .map(|(_, v)| v.clone())
        });

        if val.to_ascii_lowercase().starts_with("urn:") {
            let rest = &val[4..];
            if let Some(colon) = rest.find(':') {
                scheme = Some(rest[..colon].to_string());
                val = rest[colon + 1..].to_string();
            }
        }

        if let Some(ref s) = scheme {
            if !val.is_empty() && !s.to_ascii_lowercase().starts_with("uri") {
                val = format!("{s}:{val}");
            }
        }

        // Strip non-id attrs; keep id.
        let id = ident.attributes.get("id").cloned();
        ident.attributes.clear();
        if let Some(id) = id {
            ident.attributes.insert("id".into(), id);
        }
        ident.children.clear();
        ident.children.push(XMLNode::Text(val));
        ident.prefix = Some("dc".into());
        ident.namespace = Some(DC_NS.into());
    }
}

fn upgrade_title(pkg: &mut Element) {
    let Some(metadata) = find_child_mut(pkg, "metadata") else {
        return;
    };
    metadata.children.retain(|n| match n {
        XMLNode::Element(e) if e.name == "title" => !element_text(e).trim().is_empty(),
        _ => true,
    });
    let mut title_id = None;
    for node in metadata.children.iter_mut() {
        let XMLNode::Element(title) = node else {
            continue;
        };
        if title.name != "title" {
            continue;
        }
        title_id = Some(ensure_id(title));
        break;
    }
    let Some(title_id) = title_id else {
        return;
    };
    let opf_ns = metadata.namespace.clone().or_else(|| Some(OPF_NS.into()));
    metadata.children.push(XMLNode::Element(meta_element(
        "title-type",
        Some(&format!("#{title_id}")),
        "main",
        opf_ns.as_deref(),
    )));
}

fn upgrade_languages(pkg: &mut Element) {
    let Some(metadata) = find_child_mut(pkg, "metadata") else {
        return;
    };
    let mut has_lang = false;
    for node in metadata.children.iter_mut() {
        let XMLNode::Element(lang) = node else {
            continue;
        };
        if lang.name != "language" {
            continue;
        }
        has_lang = true;
        let id = lang.attributes.get("id").cloned();
        let text = element_text(lang);
        lang.attributes.clear();
        if let Some(id) = id {
            lang.attributes.insert("id".into(), id);
        }
        lang.children.clear();
        lang.children.push(XMLNode::Text(text));
        lang.prefix = Some("dc".into());
        lang.namespace = Some(DC_NS.into());
    }
    if !has_lang {
        let mut lang = Element::new("language");
        lang.prefix = Some("dc".into());
        lang.namespace = Some(DC_NS.into());
        lang.children.push(XMLNode::Text("und".into()));
        metadata.children.push(XMLNode::Element(lang));
    }
}

fn upgrade_authors(pkg: &mut Element) {
    let Some(metadata) = find_child_mut(pkg, "metadata") else {
        return;
    };
    let opf_ns = metadata.namespace.clone().or_else(|| Some(OPF_NS.into()));
    let mut extras: Vec<Element> = Vec::new();
    for node in metadata.children.iter_mut() {
        let XMLNode::Element(elem) = node else {
            continue;
        };
        if elem.name != "creator" && elem.name != "contributor" {
            continue;
        }
        let role = elem
            .attributes
            .iter()
            .find(|(k, _)| k.ends_with("role") || *k == "role")
            .map(|(_, v)| v.clone());
        let sort = elem
            .attributes
            .iter()
            .find(|(k, _)| k.ends_with("file-as") || *k == "file-as")
            .map(|(_, v)| v.clone());

        let need_id = role.is_some() || sort.is_some();
        let elem_id = if need_id {
            Some(ensure_id(elem))
        } else {
            elem.attributes.get("id").cloned()
        };

        let text = element_text(elem);
        elem.attributes.clear();
        if let Some(id) = &elem_id {
            elem.attributes.insert("id".into(), id.clone());
        }
        elem.children.clear();
        elem.children.push(XMLNode::Text(text));
        elem.prefix = Some("dc".into());
        elem.namespace = Some(DC_NS.into());

        if let (Some(role), Some(id)) = (&role, &elem_id) {
            let mut m = meta_element("role", Some(&format!("#{id}")), role, opf_ns.as_deref());
            m.attributes.insert("scheme".into(), "marc:relators".into());
            extras.push(m);
        }
        if let (Some(sort), Some(id)) = (&sort, &elem_id) {
            extras.push(meta_element(
                "file-as",
                Some(&format!("#{id}")),
                sort,
                opf_ns.as_deref(),
            ));
        }
    }
    for e in extras {
        metadata.children.push(XMLNode::Element(e));
    }
}

fn upgrade_date(pkg: &mut Element) {
    let Some(metadata) = find_child_mut(pkg, "metadata") else {
        return;
    };
    let mut kept = false;
    let mut remove: Vec<usize> = Vec::new();
    for (i, node) in metadata.children.iter_mut().enumerate() {
        let XMLNode::Element(date) = node else {
            continue;
        };
        if date.name != "date" {
            continue;
        }
        let text = element_text(date);
        if text.is_empty() || kept {
            remove.push(i);
            continue;
        }
        kept = true;
        let id = date.attributes.get("id").cloned();
        date.attributes.clear();
        if let Some(id) = id {
            date.attributes.insert("id".into(), id);
        }
        date.children.clear();
        date.children.push(XMLNode::Text(text));
        date.prefix = Some("dc".into());
        date.namespace = Some(DC_NS.into());
    }
    for i in remove.into_iter().rev() {
        metadata.children.remove(i);
    }
}

fn upgrade_meta(pkg: &mut Element) {
    let Some(metadata) = find_child_mut(pkg, "metadata") else {
        return;
    };
    for node in metadata.children.iter_mut() {
        let XMLNode::Element(meta) = node else {
            continue;
        };
        if meta.name != "meta" {
            continue;
        }
        let name = meta.attributes.get("name").cloned().unwrap_or_default();
        let content = meta.attributes.get("content").cloned().unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let clean = name.strip_prefix("rendition:").unwrap_or(name.as_str());
        let (prop, value) = if matches!(clean, "orientation" | "layout" | "spread") {
            (format!("rendition:{clean}"), content)
        } else if name == "fixed-layout" {
            (
                "rendition:layout".into(),
                if content.eq_ignore_ascii_case("true") {
                    "pre-paginated".into()
                } else {
                    "reflowable".into()
                },
            )
        } else if name == "orientation-lock" {
            let v = match content.to_ascii_lowercase().as_str() {
                "portrait" => "portrait",
                "landscape" => "landscape",
                _ => "auto",
            };
            ("rendition:orientation".into(), v.into())
        } else {
            continue;
        };
        meta.attributes.remove("name");
        meta.attributes.remove("content");
        meta.attributes.insert("property".into(), prop);
        meta.children.clear();
        meta.children.push(XMLNode::Text(value));
    }
}

fn upgrade_cover(pkg: &mut Element) {
    let cover_id = {
        let Some(metadata) = find_child(pkg, "metadata") else {
            return;
        };
        metadata.children.iter().find_map(|n| {
            let XMLNode::Element(meta) = n else {
                return None;
            };
            if meta.name != "meta" {
                return None;
            }
            if meta.attributes.get("name").map(|s| s.as_str()) != Some("cover") {
                return None;
            }
            meta.attributes.get("content").cloned()
        })
    };
    let Some(item_id) = cover_id else {
        return;
    };
    let Some(manifest) = find_child_mut(pkg, "manifest") else {
        return;
    };
    for node in manifest.children.iter_mut() {
        let XMLNode::Element(item) = node else {
            continue;
        };
        if item.name != "item" {
            continue;
        }
        if item.attributes.get("id").map(|s| s.as_str()) != Some(item_id.as_str()) {
            continue;
        }
        let media = item
            .attributes
            .get("media-type")
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if media.is_empty() || media.contains("xml") || media.contains("html") {
            continue;
        }
        let mut props: Vec<String> = item
            .attributes
            .get("properties")
            .map(|p| p.split_whitespace().map(|s| s.to_string()).collect())
            .unwrap_or_default();
        if !props.iter().any(|p| p == "cover-image") {
            props.push("cover-image".into());
        }
        props.sort();
        item.attributes.insert("properties".into(), props.join(" "));
    }
}

fn remove_invalid_dc_attrs(pkg: &mut Element) {
    let Some(metadata) = find_child_mut(pkg, "metadata") else {
        return;
    };
    for node in metadata.children.iter_mut() {
        let XMLNode::Element(el) = node else {
            continue;
        };
        let is_dc = el.namespace.as_deref() == Some(DC_NS)
            || el.prefix.as_deref() == Some("dc")
            || matches!(
                el.name.as_str(),
                "identifier"
                    | "title"
                    | "language"
                    | "creator"
                    | "contributor"
                    | "date"
                    | "publisher"
                    | "subject"
                    | "description"
                    | "rights"
            );
        if !is_dc {
            continue;
        }
        let id = el.attributes.get("id").cloned();
        el.attributes
            .retain(|k, _| k == "id" || k.starts_with("xmlns"));
        if let Some(id) = id {
            el.attributes.insert("id".into(), id);
        }
    }
}

fn set_last_modified(pkg: &mut Element) {
    let Some(metadata) = find_child_mut(pkg, "metadata") else {
        return;
    };
    metadata.children.retain(|n| match n {
        XMLNode::Element(e) if e.name == "meta" => {
            e.attributes.get("property").map(|p| p.as_str()) != Some("dcterms:modified")
        }
        _ => true,
    });
    let now = chrono_like_now();
    let opf_ns = metadata.namespace.clone().or_else(|| Some(OPF_NS.into()));
    metadata.children.push(XMLNode::Element(meta_element(
        "dcterms:modified",
        None,
        &now,
        opf_ns.as_deref(),
    )));
}

fn chrono_like_now() -> String {
    // ISO-8601 without fractional seconds (ST: toISOString().replace(/\.\d+/, "")).
    // Prefer `date` CLI; fall back to a fixed epoch-shaped stamp if unavailable.
    if let Ok(out) = std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    "1970-01-01T00:00:00Z".into()
}

fn extract_guide_landmarks(pkg: &Element) -> Vec<Landmark> {
    let Some(guide) = find_child(pkg, "guide") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for node in &guide.children {
        let XMLNode::Element(r) = node else {
            continue;
        };
        if r.name != "reference" {
            continue;
        }
        let href = r.attributes.get("href").cloned().unwrap_or_default();
        let title = r.attributes.get("title").cloned().unwrap_or_default();
        let guide_type = r
            .attributes
            .get("type")
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let epub_type = GUIDE_TO_EPUBTYPE
            .iter()
            .find(|(k, _)| *k == guide_type.as_str())
            .map(|(_, v)| *v);
        let Some(epub_type) = epub_type else {
            continue;
        };
        if epub_type.is_empty() || href.is_empty() {
            continue;
        }
        out.push(Landmark {
            href,
            title,
            type_: epub_type.into(),
        });
    }
    out
}

fn remove_guide(pkg: &mut Element) {
    pkg.children.retain(|n| match n {
        XMLNode::Element(e) => e.name != "guide",
        _ => true,
    });
}

fn remove_spine_toc_ref(pkg: &mut Element) {
    if let Some(spine) = find_child_mut(pkg, "spine") {
        spine.attributes.remove("toc");
    }
}

fn fix_font_mime_types(pkg: &mut Element) {
    let Some(manifest) = find_child_mut(pkg, "manifest") else {
        return;
    };
    for node in manifest.children.iter_mut() {
        let XMLNode::Element(item) = node else {
            continue;
        };
        if item.name != "item" {
            continue;
        }
        let declared = item
            .attributes
            .get("media-type")
            .cloned()
            .unwrap_or_default();
        if !is_font_mime(&declared) {
            continue;
        }
        let href = item.attributes.get("href").cloned().unwrap_or_default();
        let Some(corrected) = font_mime_from_path(&href) else {
            continue;
        };
        if corrected != declared {
            item.attributes
                .insert("media-type".into(), corrected.into());
        }
    }
}

fn is_font_mime(m: &str) -> bool {
    let m = m.to_ascii_lowercase();
    m.starts_with("font/")
        || m.contains("font-")
        || m == "application/vnd.ms-opentype"
        || m == "application/x-font-ttf"
        || m == "application/x-font-otf"
        || m == "application/font-sfnt"
        || m == "application/font-woff"
        || m == "application/font-woff2"
}

fn font_mime_from_path(href: &str) -> Option<&'static str> {
    let leaf = href.rsplit('/').next().unwrap_or(href);
    let ext = Path::new(leaf)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())?;
    match ext.as_str() {
        "ttf" | "ttc" => Some("font/ttf"),
        "otf" => Some("font/otf"),
        "woff" => Some("font/woff"),
        "woff2" => Some("font/woff2"),
        _ => None,
    }
}

fn parse_ncx_toc(
    pkg: &Element,
    entries: &HashMap<String, Vec<u8>>,
    opf_dir: &str,
) -> Vec<NavEntry> {
    let ncx_rel = ncx_href_from_package(pkg);
    let Some(ncx_rel) = ncx_rel else {
        return Vec::new();
    };
    let zip_path = join_opf(opf_dir, &ncx_rel);
    let Some(buf) = entries.get(&zip_path) else {
        return Vec::new();
    };
    let Ok(xml) = std::str::from_utf8(buf) else {
        return Vec::new();
    };
    let Ok(ncx) = Element::parse(xml.as_bytes()) else {
        return Vec::new();
    };
    if ncx.name != "ncx" {
        return Vec::new();
    }
    let nav_map = find_child(&ncx, "navMap").or_else(|| find_child(&ncx, "navmap"));
    let Some(nav_map) = nav_map else {
        return Vec::new();
    };
    parse_nav_points(nav_map, &ncx_rel)
}

fn ncx_href_from_package(pkg: &Element) -> Option<String> {
    let spine_toc = find_child(pkg, "spine").and_then(|s| s.attributes.get("toc").cloned());
    let manifest = find_child(pkg, "manifest")?;
    if let Some(toc_id) = spine_toc {
        for node in &manifest.children {
            let XMLNode::Element(item) = node else {
                continue;
            };
            if item.name == "item"
                && item.attributes.get("id").map(|s| s.as_str()) == Some(toc_id.as_str())
            {
                return item.attributes.get("href").cloned();
            }
        }
    }
    for node in &manifest.children {
        let XMLNode::Element(item) = node else {
            continue;
        };
        if item.name != "item" {
            continue;
        }
        if item.attributes.get("media-type").map(|s| s.as_str()) == Some(NCX_MIME) {
            return item.attributes.get("href").cloned();
        }
    }
    None
}

fn parse_nav_points(parent: &Element, ncx_href: &str) -> Vec<NavEntry> {
    let mut entries = Vec::new();
    for node in &parent.children {
        let XMLNode::Element(np) = node else {
            continue;
        };
        let name = np.name.to_ascii_lowercase();
        if name != "navpoint" {
            continue;
        }
        let mut title = None;
        if let Some(label) = find_child(np, "navLabel").or_else(|| find_child(np, "navlabel")) {
            if let Some(text_el) = find_child(label, "text") {
                let t = element_text(text_el).trim().to_string();
                if !t.is_empty() {
                    title = Some(t);
                }
            }
        }
        let href = find_child(np, "content")
            .and_then(|c| c.attributes.get("src").cloned())
            .map(|src| resolve_relative(ncx_href, &src));
        let children = parse_nav_points(np, ncx_href);
        entries.push(NavEntry {
            title: title.unwrap_or_else(|| format!("{}", entries.len())),
            href,
            children,
        });
    }
    entries
}

fn choose_nav_href(pkg: &Element) -> String {
    let mut existing = std::collections::HashSet::new();
    if let Some(manifest) = find_child(pkg, "manifest") {
        for node in &manifest.children {
            let XMLNode::Element(item) = node else {
                continue;
            };
            if let Some(href) = item.attributes.get("href") {
                existing.insert(href.clone());
            }
        }
    }
    let mut candidate = "nav.xhtml".to_string();
    let mut i = 1;
    while existing.contains(&candidate) {
        candidate = format!("nav{i}.xhtml");
        i += 1;
    }
    candidate
}

fn first_spine_href(pkg: &Element) -> Option<String> {
    let spine = find_child(pkg, "spine")?;
    let manifest = find_child(pkg, "manifest")?;
    let idref = spine.children.iter().find_map(|n| {
        let XMLNode::Element(e) = n else {
            return None;
        };
        if e.name == "itemref" {
            e.attributes.get("idref").cloned()
        } else {
            None
        }
    })?;
    for node in &manifest.children {
        let XMLNode::Element(item) = node else {
            continue;
        };
        if item.attributes.get("id").map(|s| s.as_str()) == Some(idref.as_str()) {
            return item.attributes.get("href").cloned();
        }
    }
    None
}

fn build_nav_document(toc: &[NavEntry], landmarks: &[Landmark], first_spine_href: &str) -> String {
    let toc_ol = if toc.is_empty() {
        format!(
            "<ol><li><a href=\"{}\">Start</a></li></ol>",
            xml_escape_attr(first_spine_href)
        )
    } else {
        build_toc_ol(toc)
    };
    let mut body = format!("<nav epub:type=\"toc\"><h1>Table of Contents</h1>{toc_ol}</nav>");
    let valid: Vec<_> = landmarks.iter().filter(|l| !l.type_.is_empty()).collect();
    if !valid.is_empty() {
        let mut lm = String::from("<ol>");
        for l in valid {
            let label = if l.title.is_empty() {
                l.type_.as_str()
            } else {
                l.title.as_str()
            };
            lm.push_str(&format!(
                "<li><a epub:type=\"{}\" href=\"{}\">{}</a></li>",
                xml_escape_attr(&l.type_),
                xml_escape_attr(&l.href),
                xml_escape_text(label)
            ));
        }
        lm.push_str("</ol>");
        body.push_str(&format!(
            "<nav epub:type=\"landmarks\" hidden=\"\">{lm}</nav>"
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Navigation</title></head>
<body>{body}</body>
</html>"#
    )
}

fn build_toc_ol(entries: &[NavEntry]) -> String {
    let mut ol = String::from("<ol>");
    for e in entries {
        let label = e.title.split_whitespace().collect::<Vec<_>>().join(" ");
        ol.push_str("<li>");
        if let Some(href) = &e.href {
            ol.push_str(&format!(
                "<a href=\"{}\">{}</a>",
                xml_escape_attr(href),
                xml_escape_text(&label)
            ));
        } else {
            ol.push_str(&format!("<span>{}</span>", xml_escape_text(&label)));
        }
        if !e.children.is_empty() {
            ol.push_str(&build_toc_ol(&e.children));
        }
        ol.push_str("</li>");
    }
    ol.push_str("</ol>");
    ol
}

fn xml_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

fn xml_escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn take_ncx_manifest_item(pkg: &mut Element, opf_dir: &str) -> Option<String> {
    let href = {
        let manifest = find_child(pkg, "manifest")?;
        let mut found = None;
        for node in &manifest.children {
            let XMLNode::Element(item) = node else {
                continue;
            };
            if item.attributes.get("media-type").map(|s| s.as_str()) == Some(NCX_MIME) {
                found = item.attributes.get("href").cloned();
                break;
            }
        }
        found?
    };
    if let Some(manifest) = find_child_mut(pkg, "manifest") {
        manifest.children.retain(|n| match n {
            XMLNode::Element(e) if e.name == "item" => {
                e.attributes.get("media-type").map(|s| s.as_str()) != Some(NCX_MIME)
            }
            _ => true,
        });
    }
    Some(join_opf(opf_dir, &href))
}

fn xhtml_manifest_hrefs(pkg: &Element) -> Vec<String> {
    let Some(manifest) = find_child(pkg, "manifest") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for node in &manifest.children {
        let XMLNode::Element(item) = node else {
            continue;
        };
        if item.name != "item" {
            continue;
        }
        let mt = item
            .attributes
            .get("media-type")
            .map(|s| s.as_str())
            .unwrap_or("");
        if mt == XHTML_MIME || mt == "text/html" {
            if let Some(href) = item.attributes.get("href") {
                out.push(href.clone());
            }
        }
    }
    out
}

fn normalize_doctype(s: &str) -> String {
    // ST: replace non-empty public/system doctype with <!DOCTYPE html>.
    let lower = s.to_ascii_lowercase();
    if let Some(start) = lower.find("<!doctype") {
        if let Some(rel_end) = lower[start..].find('>') {
            let end = start + rel_end + 1;
            let decl = &lower[start..end];
            // Already HTML5 short doctype?
            if decl == "<!doctype html>" {
                return s.to_string();
            }
            let mut out = String::with_capacity(s.len());
            out.push_str(&s[..start]);
            out.push_str("<!DOCTYPE html>");
            out.push_str(&s[end..]);
            return out;
        }
    }
    // No doctype — insert before root element if present.
    if let Some(i) = s.find("<html") {
        let mut out = String::with_capacity(s.len() + 16);
        out.push_str(&s[..i]);
        out.push_str("<!DOCTYPE html>\n");
        out.push_str(&s[i..]);
        return out;
    }
    s.to_string()
}

/// ST `collectManifestProperties`: scan XHTML for svg/script/math/epub:switch.
fn collect_manifest_properties(
    pkg: &mut Element,
    entries: &HashMap<String, Vec<u8>>,
    opf_dir: &str,
) {
    let items: Vec<(String, String, String)> = {
        let Some(manifest) = find_child(pkg, "manifest") else {
            return;
        };
        manifest
            .children
            .iter()
            .filter_map(|n| {
                let XMLNode::Element(e) = n else {
                    return None;
                };
                if e.name != "item" {
                    return None;
                }
                let id = e.attributes.get("id")?.clone();
                let href = e.attributes.get("href")?.clone();
                let mt = e.attributes.get("media-type").cloned().unwrap_or_default();
                Some((id, href, mt))
            })
            .collect()
    };

    for (id, href, mt) in items {
        let mt_l = mt.to_ascii_lowercase();
        if mt_l != XHTML_MIME && !mt_l.contains("html") {
            continue;
        }
        let zip_path = join_opf(opf_dir, &href);
        let Some(buf) = entries.get(&zip_path) else {
            continue;
        };
        let Ok(body) = std::str::from_utf8(buf) else {
            continue;
        };
        let body_l = body.to_ascii_lowercase();
        let mut add: Vec<&str> = Vec::new();
        if body_l.contains("<svg") {
            add.push("svg");
        }
        if body_l.contains("<script") {
            add.push("scripted");
        }
        if body_l.contains("<math") {
            add.push("mathml");
        }
        if body_l.contains("epub:switch") {
            add.push("switch");
        }
        if add.is_empty() {
            continue;
        }
        merge_manifest_item_properties(pkg, &id, &add);
    }
}

fn merge_manifest_item_properties(pkg: &mut Element, item_id: &str, extra: &[&str]) {
    let Some(manifest) = find_child_mut(pkg, "manifest") else {
        return;
    };
    for node in &mut manifest.children {
        let XMLNode::Element(item) = node else {
            continue;
        };
        if item.name != "item" {
            continue;
        }
        if item.attributes.get("id").map(|s| s.as_str()) != Some(item_id) {
            continue;
        }
        let mut props: Vec<String> = item
            .attributes
            .get("properties")
            .map(|p| p.split_whitespace().map(|s| s.to_string()).collect())
            .unwrap_or_default();
        let before = props.len();
        for p in extra {
            if !props.iter().any(|x| x == p) {
                props.push((*p).to_string());
            }
        }
        if props.len() == before {
            return;
        }
        props.sort();
        item.attributes.insert("properties".into(), props.join(" "));
        return;
    }
}

fn write_zip(path: &Path, entries: &HashMap<String, Vec<u8>>) -> Result<(), String> {
    let out = File::create(path).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(out);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut names: Vec<_> = entries.keys().cloned().collect();
    names.sort();
    // mimetype first uncompressed if present (EPUB convention).
    if let Some(pos) = names.iter().position(|n| n == "mimetype") {
        names.swap(0, pos);
    }
    for name in names {
        let buf = &entries[&name];
        let file_opts = if name == "mimetype" {
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)
        } else {
            opts
        };
        zip.start_file(&name, file_opts)
            .map_err(|e| e.to_string())?;
        zip.write_all(buf).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_epub2(path: &Path) {
        let file = File::create(path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default();
        zip.start_file("mimetype", opts).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("META-INF/container.xml", opts).unwrap();
        zip.write_all(br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        zip.start_file("OEBPS/content.opf", opts).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:identifier id="uid" opf:scheme="ISBN">9780000000000</dc:identifier>
    <dc:title>Upgrade Fixture</dc:title>
    <dc:language>en</dc:language>
    <dc:creator opf:role="aut" opf:file-as="Author, Test">Test Author</dc:creator>
    <meta name="cover" content="cover-img"/>
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="cover-img" href="cover.jpg" media-type="image/jpeg"/>
  </manifest>
  <spine toc="ncx">
    <itemref idref="c1"/>
  </spine>
  <guide>
    <reference type="cover" title="Cover" href="ch1.xhtml"/>
    <reference type="text" title="Start" href="ch1.xhtml"/>
  </guide>
</package>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/toc.ncx", opts).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <navMap>
    <navPoint id="np1" playOrder="1">
      <navLabel><text>Chapter One</text></navLabel>
      <content src="ch1.xhtml"/>
    </navPoint>
  </navMap>
</ncx>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/ch1.xhtml", opts).unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Ch1</title></head><body><p>Hello.</p></body></html>"#).unwrap();
        zip.start_file("OEBPS/cover.jpg", opts).unwrap();
        zip.write_all(&[0xff, 0xd8, 0xff, 0xd9]).unwrap();
        zip.finish().unwrap();
    }

    fn write_epub3(path: &Path) {
        let file = File::create(path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default();
        zip.start_file("mimetype", opts).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("META-INF/container.xml", opts).unwrap();
        zip.write_all(br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        zip.start_file("OEBPS/content.opf", opts).unwrap();
        zip.write_all(br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="uid">x</dc:identifier><dc:title>T</dc:title><dc:language>en</dc:language></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        zip.start_file("OEBPS/nav.xhtml", opts).unwrap();
        zip.write_all(b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><nav epub:type=\"toc\"><ol><li><a href=\"ch1.xhtml\">One</a></li></ol></nav></body></html>").unwrap();
        zip.start_file("OEBPS/ch1.xhtml", opts).unwrap();
        zip.write_all(b"<!DOCTYPE html><html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>Hi</p></body></html>").unwrap();
        zip.finish().unwrap();
    }

    fn read_opf(path: &Path) -> String {
        let data = std::fs::read(path).unwrap();
        let mut z = ZipArchive::new(Cursor::new(data)).unwrap();
        let mut f = z.by_name("OEBPS/content.opf").unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        s
    }

    fn zip_has(path: &Path, name: &str) -> bool {
        let data = std::fs::read(path).unwrap();
        let mut z = ZipArchive::new(Cursor::new(data)).unwrap();
        let ok = z.by_name(name).is_ok();
        ok
    }

    fn zip_text(path: &Path, name: &str) -> String {
        let data = std::fs::read(path).unwrap();
        let mut z = ZipArchive::new(Cursor::new(data)).unwrap();
        let mut f = z.by_name(name).unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn epub2_upgrades_to_3_with_nav_guide_removed_ncx_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("epub2.epub");
        write_epub2(&path);

        let upgraded = upgrade_epub_in_place(&path, false).unwrap();
        assert!(upgraded);

        let opf = read_opf(&path);
        assert!(opf.contains("version=\"3.0\"") || opf.contains("version='3.0'"));
        assert!(!opf.to_ascii_lowercase().contains("<guide"));
        assert!(opf.contains("properties=\"nav\"") || opf.contains("properties='nav'"));
        assert!(opf.contains("href=\"nav.xhtml\"") || opf.contains("href='nav.xhtml'"));
        // NCX kept when remove_ncx=false
        assert!(opf.contains("application/x-dtbncx+xml"));
        assert!(zip_has(&path, "OEBPS/toc.ncx"));
        assert!(zip_has(&path, "OEBPS/nav.xhtml"));

        let nav = zip_text(&path, "OEBPS/nav.xhtml");
        assert!(nav.contains("epub:type=\"toc\""));
        assert!(nav.contains("Chapter One"));
        assert!(nav.contains("epub:type=\"landmarks\""));

        let ch = zip_text(&path, "OEBPS/ch1.xhtml");
        assert!(ch.contains("<!DOCTYPE html>"));
        assert!(!ch.contains("xhtml11.dtd"));
    }

    #[test]
    fn collect_manifest_properties_adds_svg() {
        let mut package = Element::parse(
            &br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">x</dc:identifier>
    <dc:title>T</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="c1"/></spine>
</package>"#[..],
        )
        .unwrap();
        let mut entries = HashMap::new();
        entries.insert(
            "OEBPS/ch1.xhtml".into(),
            br#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body><svg xmlns="http://www.w3.org/2000/svg"><circle r="1"/></svg></body></html>"#
                .to_vec(),
        );
        collect_manifest_properties(&mut package, &entries, "OEBPS");
        let manifest = find_child(&package, "manifest").unwrap();
        let item = manifest.children.iter().find_map(|n| match n {
            XMLNode::Element(e) if e.name == "item" => Some(e),
            _ => None,
        });
        let props = item
            .and_then(|e| e.attributes.get("properties"))
            .map(|s| s.as_str())
            .unwrap_or("");
        assert!(props.contains("svg"), "properties={props:?}");
    }

    #[test]
    fn epub3_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("epub3.epub");
        write_epub3(&path);
        let before = std::fs::read(&path).unwrap();
        let upgraded = upgrade_epub_in_place(&path, false).unwrap();
        assert!(!upgraded);
        let after = std::fs::read(&path).unwrap();
        assert_eq!(before, after);
        let opf = read_opf(&path);
        assert!(opf.contains("version=\"3.0\""));
    }
}
