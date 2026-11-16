//! Readium Guided Navigation Documents from packaged SMIL (ST guidedNavigation.ts).
//!
//! No `@readium/shared` — serde_json shapes matching ST serialize output.

use serde_json::{json, Value};
use xmltree::Element;

use crate::align::readaloud::MarkedChapter;

pub const GND_MIME: &str = "application/guided-navigation+json";
pub const WEBPUB_MIME: &str = "application/webpub+json";
pub const XHTML_MIME: &str = "application/xhtml+xml";
pub const GUIDED_NAV_PROFILE: &str =
    "https://readium.org/webpub-manifest/profiles/guided-navigation";

/// One GND doc + zip path (`GND/{id}.json`).
#[derive(Debug, Clone)]
pub struct GuidedNavDoc {
    pub zip_path: String,
    pub json: Value,
}

/// Parse SMIL clock (`12.400s`, `00:01:02.04`, bare seconds) → seconds.
pub fn parse_clock_seconds(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(stripped) = s.strip_suffix('s').or_else(|| s.strip_suffix('S')) {
        return stripped.trim().parse().ok();
    }
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        1 => s.parse().ok(),
        2 => {
            let m: f64 = parts[0].parse().ok()?;
            let sec: f64 = parts[1].parse().ok()?;
            Some(m * 60.0 + sec)
        }
        3 => {
            let h: f64 = parts[0].parse().ok()?;
            let m: f64 = parts[1].parse().ok()?;
            let sec: f64 = parts[2].parse().ok()?;
            Some(h * 3600.0 + m * 60.0 + sec)
        }
        _ => None,
    }
}

/// Resolve `href` relative to `base_dir` (package-root style, no leading `./`).
pub fn resolve_to_root(href: &str, base_dir: &str) -> String {
    let (path, frag) = match href.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (href, None),
    };
    let joined = if path.starts_with('/') {
        path.trim_start_matches('/').to_string()
    } else if base_dir.is_empty() {
        path.to_string()
    } else {
        format!("{base_dir}/{path}")
    };
    let mut stack: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            s => stack.push(s),
        }
    }
    let mut out = stack.join("/");
    if let Some(f) = frag {
        out.push('#');
        out.push_str(f);
    }
    out
}

fn parse_roles(epub_type: Option<&str>) -> Option<Vec<String>> {
    let roles: Vec<String> = epub_type
        .unwrap_or("")
        .split_whitespace()
        .filter(|r| !r.is_empty())
        .map(|s| s.to_string())
        .collect();
    if roles.is_empty() {
        None
    } else {
        Some(roles)
    }
}

fn child_elems<'a>(el: &'a Element, names: &[&str]) -> Vec<&'a Element> {
    el.children
        .iter()
        .filter_map(|n| match n {
            xmltree::XMLNode::Element(e) if names.contains(&e.name.as_str()) => Some(e),
            _ => None,
        })
        .collect()
}

fn find_child<'a>(el: &'a Element, name: &str) -> Option<&'a Element> {
    el.children.iter().find_map(|n| match n {
        xmltree::XMLNode::Element(e) if e.name == name => Some(e),
        _ => None,
    })
}

fn find_descendant<'a>(el: &'a Element, name: &str) -> Option<&'a Element> {
    if el.name == name {
        return Some(el);
    }
    for n in &el.children {
        if let xmltree::XMLNode::Element(e) = n {
            if let Some(found) = find_descendant(e, name) {
                return Some(found);
            }
        }
    }
    None
}

fn attr(el: &Element, key: &str) -> Option<String> {
    el.attributes.get(key).cloned().or_else(|| {
        el.attributes.iter().find_map(|(k, v)| {
            if k == key || k.ends_with(&format!(":{key}")) {
                Some(v.clone())
            } else {
                None
            }
        })
    })
}

fn parse_guided_objects(
    overlay_dir: &str,
    root: Option<&Element>,
    fallback_textref: &str,
) -> Vec<Value> {
    let Some(root) = root else {
        return vec![json!({
            "textref": fallback_textref,
            "role": ["chapter"]
        })];
    };

    let mut out = Vec::new();
    for element in child_elems(root, &["seq", "par"]) {
        if element.name == "par" {
            let text = find_child(element, "text");
            let audio = find_child(element, "audio");
            let (Some(text), Some(audio)) = (text, audio) else {
                continue;
            };
            let text_src = attr(text, "src");
            let audio_src = attr(audio, "src");
            let clip_begin = attr(audio, "clipBegin");
            let clip_end = attr(audio, "clipEnd");
            let (Some(text_src), Some(audio_src), Some(clip_begin), Some(clip_end)) =
                (text_src, audio_src, clip_begin, clip_end)
            else {
                continue;
            };
            let begin = parse_clock_seconds(&clip_begin).unwrap_or(0.0);
            let end = parse_clock_seconds(&clip_end).unwrap_or(begin);
            let textref = resolve_to_root(&text_src, overlay_dir);
            let audio_file = resolve_to_root(&audio_src, overlay_dir);
            let mut obj = json!({
                "textref": textref,
                "audioFile": audio_file,
                "audioTime": format!("#t={begin},{end}")
            });
            if let Some(roles) = parse_roles(attr(element, "epub:type").as_deref()) {
                obj["role"] = json!(roles);
            }
            out.push(obj);
        } else if element.name == "seq" {
            let Some(textref_attr) = attr(element, "epub:textref") else {
                // Still recurse into children (chapter seq may nest small ranges).
                out.extend(parse_guided_objects(
                    overlay_dir,
                    Some(element),
                    fallback_textref,
                ));
                continue;
            };
            let children = parse_guided_objects(overlay_dir, Some(element), fallback_textref);
            let mut obj = json!({
                "textref": resolve_to_root(&textref_attr, overlay_dir),
                "children": children
            });
            if let Some(roles) = parse_roles(attr(element, "epub:type").as_deref()) {
                obj["role"] = json!(roles);
            }
            out.push(obj);
        }
    }
    if out.is_empty() {
        out.push(json!({
            "textref": fallback_textref,
            "role": ["chapter"]
        }));
    }
    out
}

fn flatten_guided(guided: &[Value]) -> Vec<&Value> {
    let mut pars = Vec::new();
    for obj in guided {
        if let Some(children) = obj.get("children").and_then(|c| c.as_array()) {
            pars.extend(flatten_guided(children));
        } else {
            pars.push(obj);
        }
    }
    pars
}

fn link_obj(rel: &str, href: &str) -> Value {
    json!({
        "rel": [rel],
        "href": href,
        "type": GND_MIME
    })
}

/// Build GND docs from MarkedChapters + their SMIL XML strings.
pub fn generate_guided_navigation_documents(
    chapters: &[(MarkedChapter, String)],
) -> Vec<GuidedNavDoc> {
    let mut docs = Vec::with_capacity(chapters.len());
    for (i, (ch, smil)) in chapters.iter().enumerate() {
        let mut links = Vec::new();
        if i > 0 {
            let prev = &chapters[i - 1].0.chapter_id;
            links.push(link_obj("prev", &format!("GND/{prev}.json")));
        }
        let self_href = format!("GND/{}.json", ch.chapter_id);
        links.push(link_obj("self", &self_href));
        if i + 1 < chapters.len() {
            let next = &chapters[i + 1].0.chapter_id;
            links.push(link_obj("next", &format!("GND/{next}.json")));
        }

        let overlay_dir = std::path::Path::new(&ch.smil_href.replace('\\', "/"))
            .parent()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();

        let root = Element::parse(smil.as_bytes()).ok();
        let body = root.as_ref().and_then(|r| find_descendant(r, "body"));
        let guided = parse_guided_objects(&overlay_dir, body, &ch.href);

        docs.push(GuidedNavDoc {
            zip_path: self_href,
            json: json!({
                "links": links,
                "guided": guided
            }),
        });
    }
    docs
}

/// Guided-navigation profile manifest (`GND/manifest.json`).
pub fn generate_guided_navigation_manifest(title: &str, docs: &[GuidedNavDoc]) -> Value {
    let mut reading_order = Vec::new();
    for doc in docs {
        let href = doc
            .json
            .get("links")
            .and_then(|l| l.as_array())
            .and_then(|arr| {
                arr.iter().find(|l| {
                    l.get("rel")
                        .and_then(|r| r.as_array())
                        .is_some_and(|rels| rels.iter().any(|x| x.as_str() == Some("self")))
                })
            })
            .and_then(|l| l.get("href"))
            .and_then(|h| h.as_str())
            .unwrap_or(&doc.zip_path);

        let guided = doc
            .json
            .get("guided")
            .and_then(|g| g.as_array())
            .cloned()
            .unwrap_or_default();
        let textref = guided
            .first()
            .and_then(|g| g.get("textref"))
            .and_then(|t| t.as_str())
            .unwrap_or("");
        if textref.is_empty() {
            continue;
        }

        let mut referenced: std::collections::HashMap<String, (f64, Option<f64>)> =
            std::collections::HashMap::new();
        for par in flatten_guided(&guided) {
            let Some(audio_file) = par.get("audioFile").and_then(|a| a.as_str()) else {
                continue;
            };
            let Some(range) = par.get("audioTime").and_then(|a| a.as_str()) else {
                continue;
            };
            let body = range.strip_prefix("#t=").unwrap_or(range);
            let mut parts = body.split(',');
            let start: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let end: Option<f64> = parts.next().and_then(|s| s.parse().ok());
            referenced.insert(audio_file.to_string(), (start, end));
        }

        let mut references = vec![json!({
            "href": textref,
            "type": XHTML_MIME
        })];
        for (audio_file, (start, end)) in &referenced {
            let end_s = end
                .map(|e| e.to_string())
                .unwrap_or_else(|| "null".to_string());
            let mime = audio_mime(audio_file);
            references.push(json!({
                "href": format!("{audio_file}#t={start},{end_s}"),
                "type": mime
            }));
        }

        reading_order.push(json!({
            "href": href,
            "type": GND_MIME,
            "properties": { "references": references }
        }));
    }

    json!({
        "metadata": {
            "title": title,
            "conformsTo": [GUIDED_NAV_PROFILE]
        },
        "links": [{
            "href": "GND/manifest.json",
            "type": WEBPUB_MIME,
            "rel": ["self"]
        }],
        "readingOrder": reading_order
    })
}

fn audio_mime(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".mp3") {
        "audio/mpeg"
    } else if lower.ends_with(".m4a") || lower.ends_with(".mp4") {
        "audio/mp4"
    } else if lower.ends_with(".ogg") || lower.ends_with(".opus") {
        "audio/ogg"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::holes::HolePosition;
    use crate::align::ranges::{AudioRange, SentenceRange};
    use crate::align::smil::{chapter_smil_with_text_ref, TextRef};

    fn chapter(id: &str, href: &str, smil_href: &str) -> MarkedChapter {
        MarkedChapter {
            chapter_id: id.into(),
            href: href.into(),
            xhtml: "<html/>".into(),
            smil_href: smil_href.into(),
            sentences: vec![SentenceRange::new(
                0,
                id,
                vec![AudioRange {
                    audiofile: "a.mp3".into(),
                    start: 1.0,
                    end: 2.5,
                }],
            )],
            holes: Vec::new(),
            fragment_prefix: "sentence".into(),
            text_ref: TextRef::IdFragment,
            sentence_texts: Vec::new(),
            audio_heading: None,
            spine_insert: None,
            title: None,
        }
    }

    #[test]
    fn parse_clock_seconds_accepts_smil_forms() {
        assert!((parse_clock_seconds("1.500s").unwrap() - 1.5).abs() < 1e-9);
        assert!((parse_clock_seconds("00:00:03.50").unwrap() - 3.5).abs() < 1e-9);
        assert!((parse_clock_seconds("01:02:03.00").unwrap() - 3723.0).abs() < 1e-9);
    }

    #[test]
    fn gnd_from_one_sentence_par_has_textref_and_audio_time() {
        let ch = chapter("ch1", "Text/ch1.xhtml", "Text/MediaOverlays/ch1.smil");
        let smil = chapter_smil_with_text_ref(
            "ch1",
            "../ch1.xhtml",
            "../Audio/",
            &ch.sentences,
            "sentence",
            &[],
            TextRef::IdFragment,
            &[],
        );
        let docs = generate_guided_navigation_documents(&[(ch, smil)]);
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].zip_path, "GND/ch1.json");
        let guided = docs[0].json["guided"].as_array().unwrap();
        assert!(!guided.is_empty());
        // Flatten to leaf pars
        let leaves = flatten_guided(guided);
        let leaf = leaves
            .iter()
            .find(|v| v.get("audioFile").is_some())
            .unwrap();
        let textref = leaf["textref"].as_str().unwrap();
        assert!(textref.contains("ch1.xhtml"));
        assert!(textref.contains("sentence-0") || textref.contains('#'));
        let audio_time = leaf["audioTime"].as_str().unwrap();
        assert!(audio_time.starts_with("#t="));
        assert!(audio_time.contains('1') || audio_time.contains("1"));
        let audio_file = leaf["audioFile"].as_str().unwrap();
        assert!(audio_file.contains("Audio/") || audio_file.ends_with("a.mp3"));
    }

    #[test]
    fn gnd_prev_next_links_when_multi_chapter() {
        let ch1 = chapter("a", "a.xhtml", "MediaOverlays/a.smil");
        let ch2 = chapter("b", "b.xhtml", "MediaOverlays/b.smil");
        let smil1 = chapter_smil_with_text_ref(
            "a",
            "../a.xhtml",
            "../Audio/",
            &ch1.sentences,
            "sentence",
            &[],
            TextRef::IdFragment,
            &[],
        );
        let smil2 = chapter_smil_with_text_ref(
            "b",
            "../b.xhtml",
            "../Audio/",
            &ch2.sentences,
            "sentence",
            &[],
            TextRef::IdFragment,
            &[],
        );
        let docs = generate_guided_navigation_documents(&[(ch1, smil1), (ch2, smil2)]);
        let links1 = docs[0].json["links"].as_array().unwrap();
        let links2 = docs[1].json["links"].as_array().unwrap();
        assert!(links1.iter().any(|l| l["rel"][0] == "self"));
        assert!(links1.iter().any(|l| l["rel"][0] == "next"));
        assert!(!links1.iter().any(|l| l["rel"][0] == "prev"));
        assert!(links2.iter().any(|l| l["rel"][0] == "prev"));
        assert!(links2.iter().any(|l| l["rel"][0] == "self"));
        assert!(!links2.iter().any(|l| l["rel"][0] == "next"));
    }

    #[test]
    fn chapter_without_mo_gets_chapter_role() {
        let mut ch = chapter("nav", "nav.xhtml", "MediaOverlays/nav.smil");
        ch.sentences.clear();
        let docs = generate_guided_navigation_documents(&[(ch, "not-xml".into())]);
        let guided = docs[0].json["guided"].as_array().unwrap();
        assert_eq!(guided.len(), 1);
        assert_eq!(guided[0]["textref"], "nav.xhtml");
        assert_eq!(guided[0]["role"][0], "chapter");
    }

    #[test]
    fn guided_nav_manifest_conforms_and_orders() {
        let ch = chapter("ch1", "ch1.xhtml", "MediaOverlays/ch1.smil");
        let smil = chapter_smil_with_text_ref(
            "ch1",
            "../ch1.xhtml",
            "../Audio/",
            &ch.sentences,
            "sentence",
            &[],
            TextRef::IdFragment,
            &[],
        );
        let docs = generate_guided_navigation_documents(&[(ch, smil)]);
        let man = generate_guided_navigation_manifest("Test", &docs);
        assert_eq!(man["metadata"]["conformsTo"][0], GUIDED_NAV_PROFILE);
        assert!(!man["readingOrder"].as_array().unwrap().is_empty());
    }

    #[allow(dead_code)]
    fn _hole_pos() -> HolePosition {
        HolePosition::After
    }
}
