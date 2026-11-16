//! Materialize Storyteller `writeAudioChapters` as Shelf MarkedChapters.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::align::holes::{AudioChapter, HolePosition};
use crate::align::readaloud::{MarkedChapter, READALOUD_STYLESHEET};

/// Basename → title for `plan_audio_chapters` (ST `readTrackSource` + ffprobe tags).
pub fn track_titles_from_audio_paths(paths: &[PathBuf]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for path in paths {
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Some(title) = read_source_json_title(path)
            .or_else(|| crate::audio_meta::probe_file(path).and_then(|p| p.title))
        {
            out.insert(name.to_string(), title);
        }
    }
    out
}

fn read_source_json_title(audio: &Path) -> Option<String> {
    let stem = audio.file_stem()?.to_str()?;
    let sibling = audio.with_file_name(format!("{stem}.source.json"));
    let raw = std::fs::read_to_string(sibling).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("title")
        .and_then(|t| t.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// ST `POSITIONAL_TITLES` — language primary subtag → (before, after) formatters.
fn positional_title_locale(lang: &str, position: HolePosition, anchor_title: &str) -> String {
    let lang = lang
        .split(['-', '_'])
        .next()
        .unwrap_or("en")
        .to_ascii_lowercase();
    match (lang.as_str(), position) {
        ("nl", HolePosition::Before) => format!("Voor {anchor_title}"),
        ("nl", HolePosition::After) => format!("Na {anchor_title}"),
        ("de", HolePosition::Before) => format!("Vor {anchor_title}"),
        ("de", HolePosition::After) => format!("Nach {anchor_title}"),
        ("fr", HolePosition::Before) => format!("Avant {anchor_title}"),
        ("fr", HolePosition::After) => format!("Après {anchor_title}"),
        ("es", HolePosition::Before) => format!("Antes de {anchor_title}"),
        ("es", HolePosition::After) => format!("Después de {anchor_title}"),
        ("it", HolePosition::Before) => format!("Prima di {anchor_title}"),
        ("it", HolePosition::After) => format!("Dopo {anchor_title}"),
        ("pt", HolePosition::Before) => format!("Antes de {anchor_title}"),
        ("pt", HolePosition::After) => format!("Depois de {anchor_title}"),
        (_, HolePosition::Before) => format!("Before {anchor_title}"),
        (_, HolePosition::After) => format!("After {anchor_title}"),
    }
}

fn positional_title(chapter: &AudioChapter, locale: &str) -> Option<String> {
    let anchor_title = chapter.anchor_title.as_deref()?;
    Some(positional_title_locale(
        locale,
        chapter.anchor.position,
        anchor_title,
    ))
}

fn resolve_title(chapter: &AudioChapter, n: usize, locale: &str) -> String {
    chapter
        .title
        .clone()
        .or_else(|| chapter.existing.as_ref().and_then(|e| e.title.clone()))
        .or_else(|| positional_title(chapter, locale))
        .unwrap_or_else(|| format!("Audio {n}"))
}

fn relative_href(from_dir: &str, to_path: &str) -> String {
    let from: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = to_path.split('/').filter(|s| !s.is_empty()).collect();
    let mut i = 0;
    while i < from.len() && i < to.len() && from[i] == to[i] {
        i += 1;
    }
    let mut out = "../".repeat(from.len() - i);
    out.push_str(&to[i..].join("/"));
    out
}

fn content_css_href(chapter_href: &str, content_root: &str) -> String {
    let from_dir = Path::new(chapter_href)
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let css_zip = if content_root.is_empty() {
        READALOUD_STYLESHEET.to_string()
    } else {
        format!("{content_root}/{READALOUD_STYLESHEET}")
    };
    relative_href(&from_dir, &css_zip)
}

/// Ensure the first body element has an id (ST `textRefId`).
pub fn ensure_heading_id(xhtml: &str, fallback_id: &str) -> (String, String) {
    // Prefer an existing id on the first element inside <body>.
    if let Some(body_i) = xhtml.find("<body") {
        let after_body = &xhtml[body_i..];
        if let Some(gt) = after_body.find('>') {
            let rest = &after_body[gt + 1..];
            if let Some(open) = rest.find('<') {
                let tag_start = &rest[open..];
                if !tag_start.starts_with("</") {
                    if let Some(id_at) = tag_start.find(" id=\"") {
                        let after = &tag_start[id_at + 5..];
                        if let Some(end) = after.find('"') {
                            return (xhtml.to_string(), after[..end].to_string());
                        }
                    }
                    // Insert id onto first tag.
                    if let Some(space_or_gt) = tag_start.find([' ', '>']) {
                        let insert_at = body_i + gt + 1 + open + space_or_gt;
                        let mut out = String::with_capacity(xhtml.len() + fallback_id.len() + 8);
                        out.push_str(&xhtml[..insert_at]);
                        out.push_str(" id=\"");
                        out.push_str(fallback_id);
                        out.push('"');
                        out.push_str(&xhtml[insert_at..]);
                        return (out, fallback_id.to_string());
                    }
                }
            }
        }
    }
    (xhtml.to_string(), fallback_id.to_string())
}

fn audio_chapter_xhtml(_id: &str, title: &str, heading_id: &str, css_href: &str) -> String {
    let t = xml_escape(title);
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?>"#,
            r#"<!DOCTYPE html>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">"#,
            r#"<head><title>{title}</title>"#,
            r#"<link rel="stylesheet" type="text/css" href="{css}"/>"#,
            r#"</head>"#,
            r#"<body><section epub:type="chapter">"#,
            r#"<h1 id="{heading}">{title}</h1>"#,
            r#"</section></body></html>"#
        ),
        title = t,
        css = xml_escape(css_href),
        heading = xml_escape(heading_id),
    )
}

/// Build listen-only chapters (ST `writeAudioChapters`).
///
/// `spine_xhtml`: id → (href, xhtml) for attach reuse of excluded chapters.
/// `locale`: BCP47 primary subtag (default callers pass `"en"`).
pub fn materialize_audio_chapters(
    planned: &[AudioChapter],
    content_root: &str,
    spine_xhtml: &HashMap<String, (String, String)>,
    locale: &str,
) -> Vec<MarkedChapter> {
    let root = content_root.trim_matches('/');
    let mut written = Vec::new();
    let mut n = 0usize;

    for chapter in planned {
        if chapter.holes.is_empty() {
            continue;
        }
        n += 1;
        let title = resolve_title(chapter, n, locale);

        if let Some(existing) = &chapter.existing {
            if let Some((href, xhtml)) = spine_xhtml.get(&existing.id) {
                let fallback = format!("{}-s0", existing.id);
                let (xhtml, heading_id) = ensure_heading_id(xhtml, &fallback);
                let smil_href = match Path::new(href).parent() {
                    Some(parent) if !parent.as_os_str().is_empty() => {
                        format!(
                            "{}/MediaOverlays/storyteller-audio-{n}.smil",
                            parent.to_string_lossy()
                        )
                    }
                    _ => format!("MediaOverlays/storyteller-audio-{n}.smil"),
                };
                written.push(MarkedChapter {
                    chapter_id: existing.id.clone(),
                    href: href.clone(),
                    xhtml,
                    smil_href,
                    sentences: Vec::new(),
                    holes: chapter.holes.clone(),
                    fragment_prefix: "sentence".into(),
                    text_ref: crate::align::smil::TextRef::IdFragment,
                    sentence_texts: Vec::new(),
                    audio_heading: Some(heading_id),
                    spine_insert: None,
                    title: Some(title),
                });
                continue;
            }
            // Missing item → fall through to create.
        }

        let id = format!("storyteller_audio_{n}");
        let heading_id = format!("{id}-s0");
        let anchor_href = spine_xhtml
            .get(&chapter.anchor.chapter_id)
            .map(|(h, _)| h.as_str())
            .unwrap_or("");
        let href = if anchor_href.is_empty() {
            if root.is_empty() {
                format!("storyteller-audio-{n}.xhtml")
            } else {
                format!("{root}/storyteller-audio-{n}.xhtml")
            }
        } else {
            match Path::new(anchor_href).parent() {
                Some(parent) if !parent.as_os_str().is_empty() => {
                    format!("{}/storyteller-audio-{n}.xhtml", parent.to_string_lossy())
                }
                _ => format!("storyteller-audio-{n}.xhtml"),
            }
        };
        let smil_href = match Path::new(&href).parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                format!(
                    "{}/MediaOverlays/storyteller-audio-{n}.smil",
                    parent.to_string_lossy()
                )
            }
            _ => format!("MediaOverlays/storyteller-audio-{n}.smil"),
        };
        let css = content_css_href(&href, root);
        let xhtml = audio_chapter_xhtml(&id, &title, &heading_id, &css);
        written.push(MarkedChapter {
            chapter_id: id,
            href,
            xhtml,
            smil_href,
            sentences: Vec::new(),
            holes: chapter.holes.clone(),
            fragment_prefix: "sentence".into(),
            text_ref: crate::align::smil::TextRef::IdFragment,
            sentence_texts: Vec::new(),
            audio_heading: Some(heading_id),
            spine_insert: Some((chapter.anchor.chapter_id.clone(), chapter.anchor.position)),
            title: Some(title),
        });
    }
    written
}

/// Insert a TOC `<li>` before/after the anchor entry when a nav document exists.
/// Smallest complete — no-op if nav/toc/anchor missing.
pub fn insert_nav_entry(
    nav_xhtml: &str,
    anchor_href: &str,
    position: HolePosition,
    entry_href: &str,
    entry_title: &str,
) -> Option<String> {
    let anchor_stem = Path::new(anchor_href.split('#').next().unwrap_or(anchor_href))
        .file_stem()
        .and_then(|s| s.to_str())?
        .to_lowercase();
    let href_attr = entry_href.rsplit('/').next().unwrap_or(entry_href);
    let li = format!(
        "<li><a href=\"{}\">{}</a></li>",
        xml_escape(href_attr),
        xml_escape(entry_title)
    );

    // Find <a href=...> whose stem matches the anchor; insert sibling <li>.
    let lower = nav_xhtml.to_lowercase();
    let mut search = 0usize;
    let mut match_li_range: Option<(usize, usize)> = None;
    while let Some(a_rel) = lower[search..].find("<a ") {
        let a_at = search + a_rel;
        let Some(tag_end) = nav_xhtml[a_at..].find('>') else {
            break;
        };
        let tag = &nav_xhtml[a_at..a_at + tag_end];
        let href = tag
            .split("href=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or("");
        let stem = Path::new(href.split('#').next().unwrap_or(href))
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if stem == anchor_stem {
            // Walk back to enclosing <li>.
            let before = &nav_xhtml[..a_at];
            let li_start = before.rfind("<li").unwrap_or(a_at);
            let after = &nav_xhtml[a_at..];
            let li_end_rel = after
                .find("</li>")
                .map(|i| a_at + i + 5)
                .unwrap_or(a_at + tag_end);
            match_li_range = Some((li_start, li_end_rel));
            if position == HolePosition::Before {
                break; // first match
            }
            // After: keep scanning for last match.
        }
        search = a_at + tag_end + 1;
    }
    let (li_start, li_end) = match_li_range?;
    let mut out = String::with_capacity(nav_xhtml.len() + li.len());
    match position {
        HolePosition::Before => {
            out.push_str(&nav_xhtml[..li_start]);
            out.push_str(&li);
            out.push_str(&nav_xhtml[li_start..]);
        }
        HolePosition::After => {
            out.push_str(&nav_xhtml[..li_end]);
            out.push_str(&li);
            out.push_str(&nav_xhtml[li_end..]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::holes::{ExistingChapter, Hole, HoleAnchor};

    fn hole_after(chapter: &str, file: &str, start: f64, end: f64) -> Hole {
        Hole {
            audiofile: file.into(),
            start,
            end,
            anchor: HoleAnchor {
                chapter_id: chapter.into(),
                sentence_id: 0,
                position: HolePosition::After,
            },
            boundary: true,
            between: "|".into(),
        }
    }

    #[test]
    fn materializes_spine_insert_after_anchor_with_track_title() {
        let planned = vec![AudioChapter {
            anchor: HoleAnchor {
                chapter_id: "c1".into(),
                sentence_id: 0,
                position: HolePosition::After,
            },
            holes: vec![hole_after("c1", "narration.mp3", 10.0, 40.0)],
            title: Some("Interview".into()),
            anchor_title: Some("Chapter 1".into()),
            existing: None,
            duration_secs: 30.0,
        }];
        let mut spine = HashMap::new();
        spine.insert(
            "c1".into(),
            (
                "OEBPS/c1.xhtml".into(),
                "<html><body><p>hi</p></body></html>".into(),
            ),
        );
        let got = materialize_audio_chapters(&planned, "OEBPS", &spine, "en");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].chapter_id, "storyteller_audio_1");
        assert!(got[0].href.ends_with("storyteller-audio-1.xhtml"));
        assert_eq!(
            got[0].spine_insert,
            Some(("c1".into(), HolePosition::After))
        );
        assert_eq!(got[0].title.as_deref(), Some("Interview"));
        assert!(got[0].xhtml.contains("id=\"storyteller_audio_1-s0\""));
        assert!(got[0].xhtml.contains("storyteller-readaloud.css"));
        assert_eq!(
            got[0].audio_heading.as_deref(),
            Some("storyteller_audio_1-s0")
        );
    }

    #[test]
    fn attaches_existing_when_set() {
        let planned = vec![AudioChapter {
            anchor: HoleAnchor {
                chapter_id: "c1".into(),
                sentence_id: 0,
                position: HolePosition::After,
            },
            holes: vec![hole_after("c1", "bonus.mp3", 0.0, 120.0)],
            title: None,
            anchor_title: Some("Chapter 1".into()),
            existing: Some(ExistingChapter {
                id: "c2".into(),
                title: Some("Bonus material".into()),
            }),
            duration_secs: 120.0,
        }];
        let mut spine = HashMap::new();
        spine.insert(
            "c1".into(),
            (
                "OEBPS/c1.xhtml".into(),
                "<html><body><p>a</p></body></html>".into(),
            ),
        );
        spine.insert(
            "c2".into(),
            (
                "OEBPS/bonus.xhtml".into(),
                "<html><body><section><p>Bonus</p></section></body></html>".into(),
            ),
        );
        let got = materialize_audio_chapters(&planned, "OEBPS", &spine, "en");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].chapter_id, "c2");
        assert_eq!(got[0].href, "OEBPS/bonus.xhtml");
        assert!(got[0].spine_insert.is_none());
        assert_eq!(got[0].title.as_deref(), Some("Bonus material"));
        assert!(got[0].audio_heading.is_some());
        assert!(got[0].xhtml.contains("id=\""));
    }

    #[test]
    fn positional_titles_nl_de_and_en_fallback() {
        assert_eq!(
            positional_title_locale("nl", HolePosition::Before, "Hoofdstuk 1"),
            "Voor Hoofdstuk 1"
        );
        assert_eq!(
            positional_title_locale("nl", HolePosition::After, "Hoofdstuk 1"),
            "Na Hoofdstuk 1"
        );
        assert_eq!(
            positional_title_locale("de", HolePosition::Before, "Kapitel 1"),
            "Vor Kapitel 1"
        );
        assert_eq!(
            positional_title_locale("de-DE", HolePosition::After, "Kapitel 1"),
            "Nach Kapitel 1"
        );
        assert_eq!(
            positional_title_locale("xx", HolePosition::Before, "Ch 1"),
            "Before Ch 1"
        );
        let planned = vec![AudioChapter {
            anchor: HoleAnchor {
                chapter_id: "c1".into(),
                sentence_id: 0,
                position: HolePosition::Before,
            },
            holes: vec![Hole {
                audiofile: "a.mp3".into(),
                start: 0.0,
                end: 5.0,
                anchor: HoleAnchor {
                    chapter_id: "c1".into(),
                    sentence_id: 0,
                    position: HolePosition::Before,
                },
                boundary: true,
                between: "|".into(),
            }],
            title: None,
            anchor_title: Some("Chapter 1".into()),
            existing: None,
            duration_secs: 5.0,
        }];
        let mut spine = HashMap::new();
        spine.insert(
            "c1".into(),
            (
                "OEBPS/c1.xhtml".into(),
                "<html><body><p>hi</p></body></html>".into(),
            ),
        );
        let got = materialize_audio_chapters(&planned, "OEBPS", &spine, "fr");
        assert_eq!(got[0].title.as_deref(), Some("Avant Chapter 1"));
    }

    #[test]
    fn track_titles_from_source_json_drive_basename_map() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("bonus.mp3");
        std::fs::write(&audio, b"fake").unwrap();
        std::fs::write(
            dir.path().join("bonus.source.json"),
            r#"{"source":"x","sourceSize":1,"start":0,"end":1,"title":"Interview"}"#,
        )
        .unwrap();
        let map = track_titles_from_audio_paths(&[audio]);
        assert_eq!(map.get("bonus.mp3").map(|s| s.as_str()), Some("Interview"));
    }

    #[test]
    fn insert_nav_after_anchor() {
        let nav = r#"<nav epub:type="toc"><ol><li><a href="c1.xhtml">One</a></li><li><a href="c2.xhtml">Two</a></li></ol></nav>"#;
        let out = insert_nav_entry(
            nav,
            "OEBPS/c1.xhtml",
            HolePosition::After,
            "storyteller-audio-1.xhtml",
            "Interview",
        )
        .expect("nav");
        assert!(out.contains("Interview"));
        let one = out.find("c1.xhtml").unwrap();
        let interview = out.find("Interview").unwrap();
        let two = out.find("c2.xhtml").unwrap();
        assert!(one < interview && interview < two);
    }
}
