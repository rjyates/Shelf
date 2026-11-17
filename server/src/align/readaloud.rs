//! Assemble a readaloud EPUB (marked text + audio + SMIL + OPF wiring).
//!
//! Multi-chapter: one SMIL per aligned spine item under `MediaOverlays/`, matching Storyteller.
//! EPUB3 requires `media-overlay` on the chapter's **manifest** `<item>` (Foliate + Shelf playlist).
//! Package `media:duration` metas follow EPUB3 §9.3.5.2 (Storyteller `formatSmilDuration`).

use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use xmltree::{Element, EmitterConfig, XMLNode};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::align::guided_navigation::{
    generate_guided_navigation_documents, generate_guided_navigation_manifest,
};
use crate::align::holes::{Hole, HolePosition};
use crate::align::ranges::SentenceRange;
use crate::align::report::{build_report_from_chapters, ReportAligner, ALIGN_REPORT_BASENAME};
use crate::align::smil::{
    chapter_smil_with_text_ref, create_audio_chapter_overlay, format_smil_duration,
    overlay_duration_secs, TextRef,
};
use crate::align::webpub::{
    generate_readium_manifest, ReadiumManifestOptions, WEBPUB_MANIFEST_BASENAME,
};

/// Storyteller `READALOUD_STYLESHEET` (content-root relative).
pub const READALOUD_STYLESHEET: &str = "Styles/storyteller-readaloud.css";

/// Storyteller Aligner finalize highlight rule.
pub const READALOUD_CSS: &str = ".-epub-media-overlay-active {\n  background-color: #ffb;\n}\n";

#[derive(Debug, Clone)]
pub struct MarkedChapter {
    pub chapter_id: String,
    pub href: String,
    pub xhtml: String,
    pub smil_href: String,
    pub sentences: Vec<SentenceRange>,
    /// Remaining in-chapter holes → SMIL `storyteller:audio-only` pars.
    pub holes: Vec<Hole>,
    /// SMIL text fragment prefix: `sentence` (default) or `word`.
    pub fragment_prefix: String,
    /// ST `textRef` — default `IdFragment` keeps Foliate `#sentence-N`.
    pub text_ref: TextRef,
    /// Plain sentence texts for `TextRef::TextFragment` maps (empty = id path).
    pub sentence_texts: Vec<String>,
    /// When Some, SMIL is ST `createAudioChapterOverlay` (heading fragment).
    pub audio_heading: Option<String>,
    /// Insert new spine itemref relative to (anchor chapter id, position).
    /// None = already in spine (aligned or attach-reuse).
    pub spine_insert: Option<(String, HolePosition)>,
    pub title: Option<String>,
}

pub struct ReadaloudInputs {
    pub source_epub: PathBuf,
    pub chapters: Vec<MarkedChapter>,
    pub audio_files: Vec<PathBuf>,
    /// ST package `media:duration` = sum of full packaged audio-file durations
    /// (frame_map / `addAudio`), not SMIL clip sum. Per-MO metas stay clip+holes.
    pub package_audio_duration_secs: Option<f64>,
    /// Report v4 `aligner` field (`ctc` | `classic`).
    pub aligner: ReportAligner,
}

pub fn write_readaloud(inputs: &ReadaloudInputs, output: &Path) -> Result<(), String> {
    let mut source = ZipArchive::new(File::open(&inputs.source_epub).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    let out = File::create(output).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(out);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let replace: std::collections::HashSet<String> = inputs
        .chapters
        .iter()
        .map(|c| c.href.replace('\\', "/"))
        .collect();

    for i in 0..source.len() {
        let mut file = source.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().replace('\\', "/");
        if replace.contains(&name) {
            continue;
        }
        if name.contains("Audio/")
            || name.contains("MediaOverlays/")
            || name.contains("GND/")
            || name.ends_with(".smil")
            || name.ends_with(READALOUD_STYLESHEET)
            || name.ends_with(ALIGN_REPORT_BASENAME)
            || name.ends_with(WEBPUB_MANIFEST_BASENAME)
        {
            continue;
        }
        zip.start_file(name, opts).map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        zip.write_all(&buf).map_err(|e| e.to_string())?;
    }

    let content_root = content_root_from_chapters(&inputs.chapters);
    let css_zip_href = if content_root.is_empty() {
        READALOUD_STYLESHEET.to_string()
    } else {
        format!("{content_root}/{READALOUD_STYLESHEET}")
    };

    for ch in &inputs.chapters {
        let href = ch.href.replace('\\', "/");
        let css_href = relative_href(
            Path::new(&href)
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default()
                .as_str(),
            &css_zip_href,
        );
        let xhtml = link_readaloud_stylesheet(&ch.xhtml, &css_href);
        zip.start_file(&href, opts).map_err(|e| e.to_string())?;
        zip.write_all(xhtml.as_bytes()).map_err(|e| e.to_string())?;

        // Storyteller: SMIL lives under MediaOverlays/; text+audio are sibling-relative.
        let chapter_leaf = href.rsplit('/').next().unwrap_or(&href);
        let text_src_base = format!("../{chapter_leaf}");
        let audio_prefix = "../Audio/";
        let smil = if let Some(heading) = &ch.audio_heading {
            create_audio_chapter_overlay(
                &ch.chapter_id,
                &text_src_base,
                heading,
                audio_prefix,
                &ch.holes,
            )
        } else {
            let prefix = if ch.fragment_prefix.is_empty() {
                "sentence"
            } else {
                ch.fragment_prefix.as_str()
            };
            chapter_smil_with_text_ref(
                &ch.chapter_id,
                &text_src_base,
                audio_prefix,
                &ch.sentences,
                prefix,
                &ch.holes,
                ch.text_ref,
                &ch.sentence_texts,
            )
        };
        zip.start_file(&ch.smil_href, opts)
            .map_err(|e| e.to_string())?;
        zip.write_all(smil.as_bytes()).map_err(|e| e.to_string())?;
    }

    zip.start_file(&css_zip_href, opts)
        .map_err(|e| e.to_string())?;
    zip.write_all(READALOUD_CSS.as_bytes())
        .map_err(|e| e.to_string())?;

    let mut audio_zip_hrefs = Vec::new();
    for audio in &inputs.audio_files {
        let fname = audio
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| "bad audio name".to_string())?;
        let dest = if content_root.is_empty() {
            format!("Audio/{fname}")
        } else {
            format!("{content_root}/Audio/{fname}")
        };
        zip.start_file(&dest, opts).map_err(|e| e.to_string())?;
        let mut f = File::open(audio).map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        zip.write_all(&buf).map_err(|e| e.to_string())?;
        audio_zip_hrefs.push(dest);
    }

    // Report v4 + Readium GND/WebPub (wave E) — keep Foliate SMIL path intact.
    let report = build_report_from_chapters(
        inputs.aligner,
        true,
        &inputs.chapters,
        &inputs.audio_files,
        inputs.package_audio_duration_secs,
    );
    let report_bytes =
        serde_json::to_vec_pretty(&report).map_err(|e| format!("align report json: {e}"))?;
    zip.start_file(ALIGN_REPORT_BASENAME, opts)
        .map_err(|e| e.to_string())?;
    zip.write_all(&report_bytes).map_err(|e| e.to_string())?;

    let gnd_pairs: Vec<(MarkedChapter, String)> = inputs
        .chapters
        .iter()
        .map(|ch| {
            let href = ch.href.replace('\\', "/");
            let chapter_leaf = href.rsplit('/').next().unwrap_or(&href);
            let text_src_base = format!("../{chapter_leaf}");
            let audio_prefix = "../Audio/";
            let smil = if let Some(heading) = &ch.audio_heading {
                create_audio_chapter_overlay(
                    &ch.chapter_id,
                    &text_src_base,
                    heading,
                    audio_prefix,
                    &ch.holes,
                )
            } else {
                let prefix = if ch.fragment_prefix.is_empty() {
                    "sentence"
                } else {
                    ch.fragment_prefix.as_str()
                };
                chapter_smil_with_text_ref(
                    &ch.chapter_id,
                    &text_src_base,
                    audio_prefix,
                    &ch.sentences,
                    prefix,
                    &ch.holes,
                    ch.text_ref,
                    &ch.sentence_texts,
                )
            };
            (ch.clone(), smil)
        })
        .collect();
    let gnd_docs = generate_guided_navigation_documents(&gnd_pairs);
    let book_title = inputs
        .chapters
        .iter()
        .find_map(|c| c.title.clone())
        .unwrap_or_else(|| "Untitled".into());
    let gnd_manifest = generate_guided_navigation_manifest(&book_title, &gnd_docs);
    for doc in &gnd_docs {
        let bytes = serde_json::to_vec_pretty(&doc.json).map_err(|e| format!("GND json: {e}"))?;
        zip.start_file(&doc.zip_path, opts)
            .map_err(|e| e.to_string())?;
        zip.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    let gnd_man_bytes =
        serde_json::to_vec_pretty(&gnd_manifest).map_err(|e| format!("GND manifest: {e}"))?;
    zip.start_file("GND/manifest.json", opts)
        .map_err(|e| e.to_string())?;
    zip.write_all(&gnd_man_bytes).map_err(|e| e.to_string())?;

    let opf_xml = read_opf_from_epub(&inputs.source_epub)?;
    let package = Element::parse(opf_xml.as_bytes()).map_err(|e| format!("OPF parse: {e}"))?;
    let gnd_ids: Vec<String> = inputs
        .chapters
        .iter()
        .map(|c| c.chapter_id.clone())
        .collect();
    let webpub = generate_readium_manifest(
        &package,
        &inputs.chapters,
        &audio_zip_hrefs,
        &css_zip_href,
        &gnd_ids,
        ReadiumManifestOptions::default(),
    );
    let webpub_bytes =
        serde_json::to_vec_pretty(&webpub).map_err(|e| format!("webpub json: {e}"))?;
    zip.start_file(WEBPUB_MANIFEST_BASENAME, opts)
        .map_err(|e| e.to_string())?;
    zip.write_all(&webpub_bytes).map_err(|e| e.to_string())?;

    zip.finish().map_err(|e| e.to_string())?;
    wire_opf(
        output,
        &inputs.chapters,
        &audio_zip_hrefs,
        &css_zip_href,
        inputs.package_audio_duration_secs,
    )?;
    Ok(())
}

fn read_opf_from_epub(epub: &Path) -> Result<String, String> {
    let mut archive =
        ZipArchive::new(File::open(epub).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().replace('\\', "/");
        if name.ends_with(".opf") {
            let mut buf = String::new();
            f.read_to_string(&mut buf).map_err(|e| e.to_string())?;
            return Ok(buf);
        }
    }
    Err("OPF missing".into())
}

fn content_root_from_chapters(chapters: &[MarkedChapter]) -> String {
    let Some(href) = chapters.first().map(|c| c.href.replace('\\', "/")) else {
        return String::new();
    };
    match Path::new(&href).parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            parent.to_string_lossy().replace('\\', "/")
        }
        _ => String::new(),
    }
}

fn smil_href_relative_to_opf(smil_href: &str, opf_dir: &str) -> String {
    let smil = smil_href.replace('\\', "/");
    if opf_dir.is_empty() {
        return smil;
    }
    let prefix = format!("{opf_dir}/");
    smil.strip_prefix(&prefix).unwrap_or(&smil).to_string()
}

/// ST `relative(dirname(chapter.href), READALOUD_STYLESHEET)`.
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

fn link_readaloud_stylesheet(xhtml: &str, href: &str) -> String {
    if xhtml.contains("storyteller-readaloud.css") {
        return xhtml.to_string();
    }
    // Build <link ...> without nested quote-escape noise.
    let mut link = String::from("<link rel=");
    link.push('"');
    link.push_str("stylesheet");
    link.push('"');
    link.push_str(" type=");
    link.push('"');
    link.push_str("text/css");
    link.push('"');
    link.push_str(" href=");
    link.push('"');
    link.push_str(href);
    link.push('"');
    link.push_str("/>");
    if let Some(i) = xhtml.find("</head>") {
        let mut out = String::with_capacity(xhtml.len() + link.len());
        out.push_str(&xhtml[..i]);
        out.push_str(&link);
        out.push_str(&xhtml[i..]);
        out
    } else {
        xhtml.to_string()
    }
}

fn wire_opf(
    epub_path: &Path,
    chapters: &[MarkedChapter],
    audio_zip_hrefs: &[String],
    css_zip_href: &str,
    package_audio_duration_secs: Option<f64>,
) -> Result<(), String> {
    let data = std::fs::read(epub_path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(Cursor::new(data)).map_err(|e| e.to_string())?;
    let mut opf_name = None;
    let mut opf_body = None;
    let mut others: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().replace('\\', "/");
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        if name.ends_with(".opf") {
            opf_name = Some(name);
            opf_body = Some(String::from_utf8(buf).map_err(|e| e.to_string())?);
        } else {
            others.push((name, buf));
        }
    }
    let opf_name = opf_name.ok_or_else(|| "OPF missing".to_string())?;
    let opf_dir = opf_name.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut package =
        Element::parse(opf_body.unwrap().as_bytes()).map_err(|e| format!("OPF XML parse: {e}"))?;

    apply_media_overlays(
        &mut package,
        chapters,
        audio_zip_hrefs,
        css_zip_href,
        opf_dir,
        package_audio_duration_secs,
    )?;

    // Nav TOC insert (ST insertNavEntry) — skip gracefully when no nav.
    if let Some(nav_zip) = nav_zip_href_from_package(&package, opf_dir) {
        if let Some((_, buf)) = others.iter_mut().find(|(n, _)| n == &nav_zip) {
            if let Ok(nav_str) = std::str::from_utf8(buf) {
                let mut nav = nav_str.to_string();
                for ch in chapters {
                    let Some((anchor_id, position)) = &ch.spine_insert else {
                        continue;
                    };
                    let Some(title) = ch.title.as_deref() else {
                        continue;
                    };
                    let anchor_href = chapters
                        .iter()
                        .find(|c| c.chapter_id == *anchor_id)
                        .map(|c| c.href.as_str())
                        .unwrap_or(anchor_id.as_str());
                    if let Some(patched) = crate::align::audio_chapters::insert_nav_entry(
                        &nav,
                        anchor_href,
                        *position,
                        &ch.href,
                        title,
                    ) {
                        nav = patched;
                    }
                }
                *buf = nav.into_bytes();
            }
        }
    }

    let mut opf_out = Vec::new();
    let cfg = EmitterConfig::new()
        .perform_indent(true)
        .write_document_declaration(true);
    package
        .write_with_config(&mut opf_out, cfg)
        .map_err(|e| format!("OPF XML write: {e}"))?;

    let out = File::create(epub_path).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(out);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, buf) in others {
        zip.start_file(name, opts).map_err(|e| e.to_string())?;
        zip.write_all(&buf).map_err(|e| e.to_string())?;
    }
    zip.start_file(&opf_name, opts).map_err(|e| e.to_string())?;
    zip.write_all(&opf_out).map_err(|e| e.to_string())?;
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn apply_media_overlays(
    package: &mut Element,
    chapters: &[MarkedChapter],
    audio_zip_hrefs: &[String],
    css_zip_href: &str,
    opf_dir: &str,
    package_audio_duration_secs: Option<f64>,
) -> Result<(), String> {
    let opf_ns = package.namespace.clone();
    let mut clip_secs = 0.0f64;
    let mut chapter_mo: Vec<(String, f64)> = Vec::new();

    {
        let manifest = find_child_mut(package, "manifest")
            .ok_or_else(|| "OPF missing <manifest>".to_string())?;

        let css_href = smil_href_relative_to_opf(css_zip_href, opf_dir);
        ensure_manifest_item(
            manifest,
            "storyteller_readaloud_styles",
            &css_href,
            "text/css",
            opf_ns.as_deref(),
        );

        for (i, ch) in chapters.iter().enumerate() {
            let mo_id = format!("mo{}", i + 1);
            let href_for_opf = smil_href_relative_to_opf(&ch.smil_href, opf_dir);
            ensure_manifest_item(
                manifest,
                &mo_id,
                &href_for_opf,
                "application/smil+xml",
                opf_ns.as_deref(),
            );

            let chapter_href = smil_href_relative_to_opf(&ch.href, opf_dir);
            let chapter_leaf = ch.href.rsplit('/').next().unwrap_or(&ch.href);
            // New audio chapters use ST `storyteller_audio_{n}` id; others keep OPF id.
            let text_id = if ch.spine_insert.is_some() {
                ch.chapter_id.clone()
            } else {
                format!("shelf-text-{}", i + 1)
            };
            // Spine chapters already in the source OPF; audio chapters are new.
            if find_manifest_item_by_href_leaf(manifest, chapter_leaf).is_none() {
                ensure_manifest_item(
                    manifest,
                    &text_id,
                    &chapter_href,
                    "application/xhtml+xml",
                    opf_ns.as_deref(),
                );
            }
            if let Some(item) = find_manifest_item_by_href_leaf(manifest, chapter_leaf) {
                item.attributes
                    .insert("media-overlay".into(), mo_id.clone());
            }
            let secs = overlay_duration_secs(&ch.sentences, &ch.holes);
            clip_secs += secs;
            chapter_mo.push((mo_id, secs));
        }

        for (i, zip_href) in audio_zip_hrefs.iter().enumerate() {
            let href = smil_href_relative_to_opf(zip_href, opf_dir);
            if manifest_has_href(manifest, &href) {
                continue;
            }
            let leaf = href.rsplit('/').next().unwrap_or(href.as_str());
            ensure_manifest_item(
                manifest,
                &format!("audio{i}"),
                &href,
                audio_media_type(leaf),
                opf_ns.as_deref(),
            );
        }
    }

    // Drop mistaken spine-only media-overlay; insert new audio chapters at anchor.
    let inserts: Vec<(String, String, HolePosition)> = chapters
        .iter()
        .filter_map(|ch| {
            let (anchor_id, position) = ch.spine_insert.as_ref()?;
            Some((ch.chapter_id.clone(), anchor_id.clone(), *position))
        })
        .collect();

    if let Some(spine) = find_child_mut(package, "spine") {
        for node in &mut spine.children {
            let XMLNode::Element(itemref) = node else {
                continue;
            };
            if itemref.name != "itemref" {
                continue;
            }
            itemref.attributes.remove("media-overlay");
        }

        let mut last_inserted: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        for (new_id, anchor_id, position) in inserts {
            let already = spine.children.iter().any(|n| {
                matches!(n, XMLNode::Element(e) if e.name == "itemref"
                    && e.attributes.get("idref").map(|s| s.as_str()) == Some(new_id.as_str()))
            });
            if already {
                continue;
            }

            let key = format!("{anchor_id}:{position:?}");
            let neighbour = last_inserted
                .get(&key)
                .cloned()
                .unwrap_or_else(|| anchor_id.clone());
            let neighbour_index = spine.children.iter().position(|n| {
                matches!(n, XMLNode::Element(e) if e.name == "itemref"
                    && e.attributes.get("idref").map(|s| s.as_str()) == Some(neighbour.as_str()))
            });

            let mut itemref = Element::new("itemref");
            itemref.namespace = opf_ns.clone();
            itemref.attributes.insert("idref".into(), new_id.clone());
            let insert_at = match neighbour_index {
                None => spine.children.len(),
                Some(i) => {
                    // ST: after OR already inserted at this key → neighbourIndex+1; else before.
                    if position == HolePosition::After || last_inserted.contains_key(&key) {
                        i + 1
                    } else {
                        i
                    }
                }
            };
            let at = insert_at.min(spine.children.len());
            spine.children.insert(at, XMLNode::Element(itemref));
            last_inserted.insert(key, new_id);
        }
    }

    let metadata =
        find_child_mut(package, "metadata").ok_or_else(|| "OPF missing <metadata>".to_string())?;
    for (mo_id, secs) in &chapter_mo {
        upsert_duration_meta(
            metadata,
            Some(mo_id),
            &format_smil_duration(*secs),
            opf_ns.as_deref(),
        );
    }
    // ST: package media:duration = sum of packaged audio-file lengths, not clips.
    let package_secs = package_audio_duration_secs.unwrap_or(clip_secs);
    upsert_duration_meta(
        metadata,
        None,
        &format_smil_duration(package_secs),
        opf_ns.as_deref(),
    );
    // Foliate applies this class on the active MO fragment; BookOrbit also
    // defaults it, but ST packages emit the meta explicitly.
    ensure_active_class_meta(metadata, opf_ns.as_deref());

    Ok(())
}

fn ensure_active_class_meta(metadata: &mut Element, opf_ns: Option<&str>) {
    const PROP: &str = "media:active-class";
    const CLASS: &str = "-epub-media-overlay-active";
    let already = metadata.children.iter().any(|n| {
        matches!(n,
            XMLNode::Element(e) if e.name == "meta"
                && e.attributes.get("property").map(|p| p.as_str()) == Some(PROP)
        )
    });
    if already {
        return;
    }
    let mut meta = Element::new("meta");
    meta.namespace = opf_ns.map(|s| s.to_string());
    meta.attributes.insert("property".into(), PROP.into());
    meta.children.push(XMLNode::Text(CLASS.into()));
    metadata.children.push(XMLNode::Element(meta));
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

/// Zip path of the EPUB3 nav document, if present.
fn nav_zip_href_from_package(package: &Element, opf_dir: &str) -> Option<String> {
    let manifest = find_child(package, "manifest")?;
    for node in &manifest.children {
        let XMLNode::Element(item) = node else {
            continue;
        };
        if item.name != "item" {
            continue;
        }
        let Some(props) = item.attributes.get("properties") else {
            continue;
        };
        if !props.split_whitespace().any(|p| p == "nav") {
            continue;
        }
        let Some(href) = item.attributes.get("href") else {
            continue;
        };
        let href = href.replace('\\', "/");
        return Some(if opf_dir.is_empty() {
            href
        } else {
            format!("{opf_dir}/{href}").replace("//", "/")
        });
    }
    None
}

fn find_manifest_item_by_href_leaf<'a>(
    manifest: &'a mut Element,
    leaf: &str,
) -> Option<&'a mut Element> {
    manifest.children.iter_mut().find_map(|n| {
        let XMLNode::Element(e) = n else {
            return None;
        };
        if e.name != "item" {
            return None;
        }
        let href = e.attributes.get("href")?;
        let href_leaf = href.rsplit('/').next().unwrap_or(href.as_str());
        (href_leaf == leaf || href.contains(leaf)).then_some(e)
    })
}

fn manifest_has_href(manifest: &Element, href: &str) -> bool {
    manifest.children.iter().any(|n| match n {
        XMLNode::Element(e) if e.name == "item" => {
            e.attributes.get("href").map(|h| h.as_str()) == Some(href)
        }
        _ => false,
    })
}

fn ensure_manifest_item(
    manifest: &mut Element,
    id: &str,
    href: &str,
    media_type: &str,
    opf_ns: Option<&str>,
) {
    if manifest.children.iter().any(|n| match n {
        XMLNode::Element(e) if e.name == "item" => {
            e.attributes.get("id").map(|v| v.as_str()) == Some(id)
        }
        _ => false,
    }) {
        return;
    }
    let mut item = Element::new("item");
    item.namespace = opf_ns.map(|s| s.to_string());
    item.attributes.insert("id".into(), id.into());
    item.attributes.insert("href".into(), href.into());
    item.attributes
        .insert("media-type".into(), media_type.into());
    manifest.children.push(XMLNode::Element(item));
}

fn upsert_duration_meta(
    metadata: &mut Element,
    refines_mo_id: Option<&str>,
    clock: &str,
    opf_ns: Option<&str>,
) {
    let refines_val = refines_mo_id.map(|id| format!("#{id}"));
    // Remove existing matching duration meta so re-wire is idempotent.
    metadata.children.retain(|n| {
        let XMLNode::Element(e) = n else {
            return true;
        };
        if e.name != "meta" {
            return true;
        }
        if e.attributes.get("property").map(|p| p.as_str()) != Some("media:duration") {
            return true;
        }
        match (&refines_val, e.attributes.get("refines")) {
            (None, None) => false,
            (None, Some(_)) => true,
            (Some(want), Some(have)) => have != want,
            (Some(_), None) => true,
        }
    });

    let mut meta = Element::new("meta");
    meta.namespace = opf_ns.map(|s| s.to_string());
    meta.attributes
        .insert("property".into(), "media:duration".into());
    if let Some(r) = refines_val {
        meta.attributes.insert("refines".into(), r);
    }
    meta.children.push(XMLNode::Text(clock.to_string()));
    metadata.children.push(XMLNode::Element(meta));
}

fn audio_media_type(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".m4b") || lower.ends_with(".m4a") || lower.ends_with(".mp4") {
        "audio/mp4"
    } else if lower.ends_with(".ogg") || lower.ends_with(".opus") {
        "audio/ogg"
    } else if lower.ends_with(".flac") {
        "audio/flac"
    } else {
        "audio/mpeg"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::ranges::{AudioRange, SentenceRange};

    fn sample_opf() -> Element {
        Element::parse(
            &br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">t</dc:identifier>
    <dc:title>T</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1" media-overlay="mo1"/>
  </spine>
</package>"#[..],
        )
        .unwrap()
    }

    #[test]
    fn dom_sets_media_overlay_and_duration_without_breaking_self_close() {
        let mut package = sample_opf();
        let chapters = vec![MarkedChapter {
            chapter_id: "ch1".into(),
            href: "OEBPS/chapter1.xhtml".into(),
            xhtml: String::new(),
            smil_href: "OEBPS/MediaOverlays/chapter1.smil".into(),
            sentences: vec![SentenceRange::new(
                0,
                "ch1",
                vec![AudioRange {
                    audiofile: "Audio/n.mp3".into(),
                    start: 1.0,
                    end: 4.5,
                }],
            )],
            holes: Vec::new(),
            fragment_prefix: "sentence".into(),
            text_ref: TextRef::IdFragment,
            sentence_texts: Vec::new(),
            audio_heading: None,
            spine_insert: None,
            title: None,
        }];
        apply_media_overlays(
            &mut package,
            &chapters,
            &["OEBPS/Audio/n.mp3".into()],
            "OEBPS/Styles/storyteller-readaloud.css",
            "OEBPS",
            None,
        )
        .unwrap();

        let mut out = Vec::new();
        package
            .write_with_config(
                &mut out,
                EmitterConfig::new()
                    .perform_indent(true)
                    .write_document_declaration(true),
            )
            .unwrap();
        let opf = String::from_utf8(out).unwrap();
        assert!(!opf.contains("/ media-overlay="), "{opf}");
        assert!(!opf.contains("/> media-overlay="), "{opf}");
        assert!(opf.contains("media-overlay=\"mo1\""), "{opf}");
        assert!(opf.contains("property=\"media:duration\""), "{opf}");
        assert!(opf.contains("refines=\"#mo1\""), "{opf}");
        assert!(opf.contains("00:00:03.50"), "{opf}");
        assert!(
            opf.contains("property=\"media:active-class\"")
                && opf.contains("-epub-media-overlay-active"),
            "{opf}"
        );
        // Spine must not keep media-overlay.
        let spine = opf.split("<spine>").nth(1).unwrap_or("");
        assert!(!spine.contains("media-overlay="), "{opf}");
        assert!(
            opf.contains("storyteller_readaloud_styles")
                && opf.contains("Styles/storyteller-readaloud.css"),
            "{opf}"
        );
        // Round-trip still parses.
        Element::parse(opf.as_bytes()).expect("well-formed OPF");
    }

    #[test]
    fn package_duration_uses_audio_file_sum_not_clips() {
        // Clip overlay is 3.5s; unused audio tail makes packaged file 100s.
        let mut package = sample_opf();
        let chapters = vec![MarkedChapter {
            chapter_id: "ch1".into(),
            href: "OEBPS/chapter1.xhtml".into(),
            xhtml: String::new(),
            smil_href: "OEBPS/MediaOverlays/chapter1.smil".into(),
            sentences: vec![SentenceRange::new(
                0,
                "ch1",
                vec![AudioRange {
                    audiofile: "Audio/n.mp3".into(),
                    start: 1.0,
                    end: 4.5,
                }],
            )],
            holes: Vec::new(),
            fragment_prefix: "sentence".into(),
            text_ref: TextRef::IdFragment,
            sentence_texts: Vec::new(),
            audio_heading: None,
            spine_insert: None,
            title: None,
        }];
        apply_media_overlays(
            &mut package,
            &chapters,
            &["OEBPS/Audio/n.mp3".into()],
            "OEBPS/Styles/storyteller-readaloud.css",
            "OEBPS",
            Some(100.0),
        )
        .unwrap();
        let mut out = Vec::new();
        package
            .write_with_config(
                &mut out,
                EmitterConfig::new()
                    .perform_indent(true)
                    .write_document_declaration(true),
            )
            .unwrap();
        let opf = String::from_utf8(out).unwrap();
        // Per-MO still clip length; package-level uses full file duration.
        assert!(opf.contains("refines=\"#mo1\""), "{opf}");
        assert!(opf.contains("00:00:03.50"), "{opf}");
        assert!(opf.contains("00:01:40.00"), "{opf}");
    }

    #[test]
    fn relative_href_matches_storyteller_dirname_join() {
        assert_eq!(
            relative_href("OEBPS", "OEBPS/Styles/storyteller-readaloud.css"),
            "Styles/storyteller-readaloud.css"
        );
        assert_eq!(
            relative_href("OEBPS/Text", "OEBPS/Styles/storyteller-readaloud.css"),
            "../Styles/storyteller-readaloud.css"
        );
    }

    #[test]
    fn link_readaloud_stylesheet_injects_before_head_close() {
        let xhtml = "<html><head><title>t</title></head><body></body></html>";
        let out = link_readaloud_stylesheet(xhtml, "Styles/storyteller-readaloud.css");
        assert!(out.contains("storyteller-readaloud.css"));
        assert!(out.contains("stylesheet"));
        let head = out.find("</head>").unwrap();
        let link = out.find("storyteller-readaloud.css").unwrap();
        assert!(link < head);
    }
}
