//! Readium WebPub manifest (ST `generateReadiumManifest` shape, no `@readium/shared`).

use std::collections::HashMap;

use serde_json::{json, Map, Value};
use xmltree::Element;

use crate::align::guided_navigation::{GND_MIME, WEBPUB_MIME, XHTML_MIME};
use crate::align::readaloud::MarkedChapter;

pub const WEBPUB_CONTEXT: &str = "https://readium.org/webpub-manifest/context.jsonld";
pub const EPUB_PROFILE: &str = "https://readium.org/webpub-manifest/profiles/epub";
pub const WEBPUB_MANIFEST_BASENAME: &str = "manifest.json";

/// Compressed XHTML bytes per inferred page (Readium architecture #123).
pub const BYTES_PER_PAGE: u64 = 1024;

/// Nav link tree for `toc` / `landmarks` / `pageList` (caller parses nav).
#[derive(Clone, Debug, Default)]
pub struct WebPubNavLink {
    pub title: Option<String>,
    pub href: String,
    pub children: Vec<WebPubNavLink>,
}

/// Optional WebPub extras beyond OPF spine + GND wiring.
#[derive(Clone, Debug, Default)]
pub struct ReadiumManifestOptions<'a> {
    pub toc: Option<&'a [WebPubNavLink]>,
    pub landmarks: Option<&'a [WebPubNavLink]>,
    pub page_list: Option<&'a [WebPubNavLink]>,
    pub infer_page_count: bool,
    /// Compressed sizes keyed by spine `href` and/or manifest `id`.
    pub page_bytes: Option<&'a HashMap<String, u64>>,
}

#[derive(Clone, Debug)]
struct OpfPerson {
    name: String,
    role: Option<String>,
}

fn text_of(el: &Element) -> String {
    el.children
        .iter()
        .filter_map(|n| match n {
            xmltree::XMLNode::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string()
}

fn find_child<'a>(el: &'a Element, name: &str) -> Option<&'a Element> {
    el.children.iter().find_map(|n| match n {
        xmltree::XMLNode::Element(e) if e.name == name || e.name.ends_with(&format!(":{name}")) => {
            Some(e)
        }
        _ => None,
    })
}

fn find_children<'a>(el: &'a Element, name: &str) -> Vec<&'a Element> {
    el.children
        .iter()
        .filter_map(|n| match n {
            xmltree::XMLNode::Element(e)
                if e.name == name || e.name.ends_with(&format!(":{name}")) =>
            {
                Some(e)
            }
            _ => None,
        })
        .collect()
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn attr_local(el: &Element, key: &str) -> Option<String> {
    el.attributes.get(key).cloned().or_else(|| {
        el.attributes.iter().find_map(|(k, v)| {
            if k == key || k.ends_with(&format!(":{key}")) || k.ends_with(key) {
                Some(v.clone())
            } else {
                None
            }
        })
    })
}

fn meta_dc(package: &Element, tag: &str) -> Option<String> {
    let metadata = find_child(package, "metadata")?;
    for n in &metadata.children {
        let xmltree::XMLNode::Element(e) = n else {
            continue;
        };
        if local_name(&e.name) == tag {
            let t = text_of(e);
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

fn meta_role_refines(metadata: &Element) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for n in &metadata.children {
        let xmltree::XMLNode::Element(e) = n else {
            continue;
        };
        if local_name(&e.name) != "meta" {
            continue;
        }
        let prop = attr_local(e, "property").unwrap_or_default();
        if local_name(&prop) != "role" {
            continue;
        }
        let Some(refines) = attr_local(e, "refines") else {
            continue;
        };
        let id = refines.trim_start_matches('#');
        if id.is_empty() {
            continue;
        }
        let role = text_of(e);
        if !role.is_empty() {
            out.insert(id.to_string(), role);
        }
    }
    out
}

fn parse_opf_people(package: &Element, tag: &str) -> Vec<OpfPerson> {
    let Some(metadata) = find_child(package, "metadata") else {
        return Vec::new();
    };
    let refine_roles = meta_role_refines(metadata);
    let mut out = Vec::new();
    for n in &metadata.children {
        let xmltree::XMLNode::Element(e) = n else {
            continue;
        };
        if local_name(&e.name) != tag {
            continue;
        }
        let name = text_of(e);
        if name.is_empty() {
            continue;
        }
        let mut role = attr_local(e, "role");
        if role.is_none() {
            if let Some(id) = e.attributes.get("id") {
                role = refine_roles.get(id).cloned();
            }
        }
        out.push(OpfPerson { name, role });
    }
    out
}

fn person_json(p: &OpfPerson) -> Value {
    match &p.role {
        Some(r) if !r.is_empty() => json!({ "name": p.name, "role": [r] }),
        _ => json!({ "name": p.name }),
    }
}

fn people_with_role(people: &[OpfPerson], role: &str) -> Vec<Value> {
    people
        .iter()
        .filter(|p| p.role.as_deref() == Some(role))
        .map(person_json)
        .collect()
}

/// Authors: `role=aut`, or `dc:creator` with no role (common OPF; preserves lite behavior).
fn authors_from_creators(creators: &[OpfPerson]) -> Vec<Value> {
    creators
        .iter()
        .filter(|p| matches!(p.role.as_deref(), None | Some("") | Some("aut")))
        .map(person_json)
        .collect()
}

fn audio_mime(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".mp3") {
        "audio/mpeg"
    } else if lower.ends_with(".m4a") || lower.ends_with(".mp4") {
        "audio/mp4"
    } else if lower.ends_with(".ogg") || lower.ends_with(".opus") {
        "audio/ogg"
    } else if lower.ends_with(".css") {
        "text/css"
    } else if lower.ends_with(".smil") {
        "application/smil+xml"
    } else {
        "application/octet-stream"
    }
}

fn is_xhtml_media_type(mt: &str) -> bool {
    mt == XHTML_MIME || mt == "application/xhtml+xml"
}

fn infer_number_of_pages(
    package: &Element,
    page_bytes: Option<&HashMap<String, u64>>,
) -> Option<u64> {
    let spine = find_child(package, "spine")?;
    let manifest = find_child(package, "manifest")?;
    let mut total: u64 = 0;
    for itemref in find_children(spine, "itemref") {
        let Some(idref) = itemref.attributes.get("idref") else {
            continue;
        };
        let Some(item) = manifest.children.iter().find_map(|n| match n {
            xmltree::XMLNode::Element(e)
                if local_name(&e.name) == "item"
                    && e.attributes.get("id").map(|s| s.as_str()) == Some(idref.as_str()) =>
            {
                Some(e)
            }
            _ => None,
        }) else {
            continue;
        };
        let mt = item
            .attributes
            .get("media-type")
            .map(|s| s.as_str())
            .unwrap_or("");
        if !is_xhtml_media_type(mt) {
            continue;
        }
        let href = item
            .attributes
            .get("href")
            .map(|s| s.as_str())
            .unwrap_or("");
        let len = page_bytes.and_then(|m| m.get(href).or_else(|| m.get(idref.as_str())).copied());
        match len {
            Some(n) => total = total.saturating_add((n.div_ceil(BYTES_PER_PAGE)).max(1)),
            None => total = total.saturating_add(1),
        }
    }
    if total > 0 {
        Some(total)
    } else {
        None
    }
}

fn nav_links_to_json(links: &[WebPubNavLink]) -> Vec<Value> {
    links
        .iter()
        .map(|l| {
            let href = if l.href.is_empty() {
                "#"
            } else {
                l.href.as_str()
            };
            let mut obj = Map::new();
            obj.insert("href".into(), json!(href));
            if let Some(t) = &l.title {
                obj.insert("title".into(), json!(t));
            }
            if !l.children.is_empty() {
                obj.insert("children".into(), json!(nav_links_to_json(&l.children)));
            }
            Value::Object(obj)
        })
        .collect()
}

fn insert_contributors_field(metadata: &mut Map<String, Value>, key: &str, people: Vec<Value>) {
    if !people.is_empty() {
        metadata.insert(key.into(), Value::Array(people));
    }
}

/// WebPub: context, contributor roles, optional page count / nav, readingOrder, resources, GND.
pub fn generate_readium_manifest(
    package: &Element,
    chapters: &[MarkedChapter],
    audio_zip_hrefs: &[String],
    css_zip_href: &str,
    gnd_chapter_ids: &[String],
    options: ReadiumManifestOptions<'_>,
) -> Value {
    let title = meta_dc(package, "title").unwrap_or_else(|| "Untitled".into());
    let language = meta_dc(package, "language").unwrap_or_else(|| "en".into());

    let creators = parse_opf_people(package, "creator");
    let contributors = parse_opf_people(package, "contributor");

    let authors = authors_from_creators(&creators);
    let artists = people_with_role(&creators, "art");
    let colorists = people_with_role(&creators, "clr");
    let illustrators = people_with_role(&creators, "ill");
    let inkers = people_with_role(&creators, "ink");
    let letterers = people_with_role(&creators, "ltr");
    let narrators = people_with_role(&creators, "nrt");
    let pencilers = people_with_role(&creators, "pnc");
    let translators = people_with_role(&creators, "trl");
    let publishers = people_with_role(&creators, "pbl");
    let contributor_objs: Vec<Value> = contributors.iter().map(person_json).collect();

    let gnd_set: std::collections::HashSet<&str> =
        gnd_chapter_ids.iter().map(|s| s.as_str()).collect();

    let mut reading_order = Vec::new();
    if let Some(spine) = find_child(package, "spine") {
        let manifest = find_child(package, "manifest");
        for itemref in find_children(spine, "itemref") {
            let Some(idref) = itemref.attributes.get("idref") else {
                continue;
            };
            let href = manifest
                .and_then(|m| {
                    m.children.iter().find_map(|n| match n {
                        xmltree::XMLNode::Element(e)
                            if e.name == "item"
                                && e.attributes.get("id").map(|s| s.as_str())
                                    == Some(idref.as_str()) =>
                        {
                            e.attributes.get("href").cloned()
                        }
                        _ => None,
                    })
                })
                .or_else(|| {
                    chapters
                        .iter()
                        .find(|c| c.chapter_id == *idref)
                        .map(|c| c.href.clone())
                });
            let Some(href) = href else {
                continue;
            };
            let mut link = json!({
                "href": href,
                "type": XHTML_MIME
            });
            let gnd_id = if gnd_set.contains(idref.as_str()) {
                Some(idref.as_str())
            } else {
                chapters
                    .iter()
                    .find(|c| c.href == href || c.chapter_id == *idref)
                    .and_then(|c| {
                        if gnd_set.contains(c.chapter_id.as_str()) {
                            Some(c.chapter_id.as_str())
                        } else {
                            None
                        }
                    })
            };
            if let Some(gid) = gnd_id {
                link["alternate"] = json!([{
                    "href": format!("GND/{gid}.json"),
                    "type": GND_MIME
                }]);
            }
            reading_order.push(link);
        }
    }
    if reading_order.is_empty() {
        for ch in chapters {
            let mut link = json!({
                "href": ch.href,
                "type": XHTML_MIME
            });
            if gnd_set.contains(ch.chapter_id.as_str()) {
                link["alternate"] = json!([{
                    "href": format!("GND/{}.json", ch.chapter_id),
                    "type": GND_MIME
                }]);
            }
            reading_order.push(link);
        }
    }

    let mut resources = Vec::new();
    if !css_zip_href.is_empty() {
        resources.push(json!({
            "href": css_zip_href,
            "type": "text/css"
        }));
    }
    for ch in chapters {
        resources.push(json!({
            "href": ch.smil_href,
            "type": "application/smil+xml"
        }));
        if gnd_set.contains(ch.chapter_id.as_str()) {
            resources.push(json!({
                "href": format!("GND/{}.json", ch.chapter_id),
                "type": GND_MIME
            }));
        }
    }
    for audio in audio_zip_hrefs {
        resources.push(json!({
            "href": audio,
            "type": audio_mime(audio)
        }));
    }

    let mut metadata = Map::new();
    metadata.insert("title".into(), json!(title));
    metadata.insert("language".into(), json!([language]));
    metadata.insert("author".into(), Value::Array(authors));
    metadata.insert("conformsTo".into(), json!([EPUB_PROFILE]));
    insert_contributors_field(&mut metadata, "artist", artists);
    insert_contributors_field(&mut metadata, "colorist", colorists);
    insert_contributors_field(&mut metadata, "contributor", contributor_objs);
    insert_contributors_field(&mut metadata, "illustrator", illustrators);
    insert_contributors_field(&mut metadata, "inker", inkers);
    insert_contributors_field(&mut metadata, "letterer", letterers);
    insert_contributors_field(&mut metadata, "narrator", narrators);
    insert_contributors_field(&mut metadata, "penciler", pencilers);
    insert_contributors_field(&mut metadata, "translator", translators);
    insert_contributors_field(&mut metadata, "publisher", publishers);

    if options.infer_page_count {
        if let Some(n) = infer_number_of_pages(package, options.page_bytes) {
            metadata.insert("numberOfPages".into(), json!(n));
        }
    }

    let mut man = Map::new();
    man.insert("@context".into(), json!([WEBPUB_CONTEXT]));
    man.insert("metadata".into(), Value::Object(metadata));
    man.insert(
        "links".into(),
        json!([{
            "href": WEBPUB_MANIFEST_BASENAME,
            "type": WEBPUB_MIME,
            "rel": ["self"]
        }]),
    );
    man.insert("readingOrder".into(), Value::Array(reading_order));
    man.insert("resources".into(), Value::Array(resources));

    if let Some(toc) = options.toc {
        if !toc.is_empty() {
            man.insert("toc".into(), Value::Array(nav_links_to_json(toc)));
        }
    }
    if let Some(lm) = options.landmarks {
        if !lm.is_empty() {
            man.insert("landmarks".into(), Value::Array(nav_links_to_json(lm)));
        }
    }
    if let Some(pl) = options.page_list {
        if !pl.is_empty() {
            man.insert("pageList".into(), Value::Array(nav_links_to_json(pl)));
        }
    }

    Value::Object(man)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::ranges::{AudioRange, SentenceRange};
    use crate::align::smil::TextRef;

    fn sample_package() -> Element {
        let xml = r#"<?xml version="1.0"?>
<package>
  <metadata>
    <dc:title xmlns:dc="http://purl.org/dc/elements/1.1/">Lamp</dc:title>
    <dc:language xmlns:dc="http://purl.org/dc/elements/1.1/">en</dc:language>
    <dc:creator xmlns:dc="http://purl.org/dc/elements/1.1/">Author</dc:creator>
  </metadata>
  <manifest>
    <item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="c1"/>
  </spine>
</package>"#;
        Element::parse(xml.as_bytes()).unwrap()
    }

    fn roles_package() -> Element {
        let xml = r##"<?xml version="1.0"?>
<package xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
  <metadata>
    <dc:title>Roles</dc:title>
    <dc:language>en</dc:language>
    <dc:creator id="a1" opf:role="aut">Ann Author</dc:creator>
    <dc:creator id="n1">Ned Narrator</dc:creator>
    <meta refines="#n1" property="role">nrt</meta>
    <dc:creator id="i1" opf:role="ill">Ivy Ill</dc:creator>
    <dc:creator opf:role="art">Art Person</dc:creator>
    <dc:creator opf:role="clr">Color Person</dc:creator>
    <dc:creator opf:role="ink">Ink Person</dc:creator>
    <dc:creator opf:role="ltr">Letter Person</dc:creator>
    <dc:creator opf:role="pnc">Pencil Person</dc:creator>
    <dc:creator opf:role="trl">Trans Person</dc:creator>
    <dc:creator opf:role="pbl">Pub House</dc:creator>
    <dc:contributor id="c1">Ed Extra</dc:contributor>
  </metadata>
  <manifest>
    <item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="ch2.xhtml" media-type="application/xhtml+xml"/>
    <item id="img" href="cover.jpg" media-type="image/jpeg"/>
  </manifest>
  <spine>
    <itemref idref="c1"/>
    <itemref idref="c2"/>
  </spine>
</package>"##;
        Element::parse(xml.as_bytes()).unwrap()
    }

    fn chapter() -> MarkedChapter {
        MarkedChapter {
            chapter_id: "c1".into(),
            href: "ch1.xhtml".into(),
            xhtml: "<html/>".into(),
            smil_href: "MediaOverlays/ch1.smil".into(),
            sentences: vec![SentenceRange::new(
                0,
                "c1",
                vec![AudioRange {
                    audiofile: "a.mp3".into(),
                    start: 0.0,
                    end: 1.0,
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
    fn webpub_has_context_and_nonempty_reading_order() {
        let pkg = sample_package();
        let ch = chapter();
        let man = generate_readium_manifest(
            &pkg,
            &[ch],
            &["Audio/a.mp3".into()],
            "Styles/storyteller-readaloud.css",
            &["c1".into()],
            ReadiumManifestOptions::default(),
        );
        let ctx = man["@context"].as_array().unwrap();
        assert!(ctx.iter().any(|v| v.as_str() == Some(WEBPUB_CONTEXT)));
        let order = man["readingOrder"].as_array().unwrap();
        assert!(!order.is_empty());
        assert_eq!(man["metadata"]["title"], "Lamp");
        assert!(order[0].get("alternate").is_some());
        assert!(man["metadata"].get("numberOfPages").is_none());
    }

    #[test]
    fn webpub_maps_creator_roles_and_dc_contributor() {
        let pkg = roles_package();
        let man =
            generate_readium_manifest(&pkg, &[], &[], "", &[], ReadiumManifestOptions::default());
        let meta = &man["metadata"];
        assert_eq!(meta["author"][0]["name"], "Ann Author");
        assert_eq!(meta["narrator"][0]["name"], "Ned Narrator");
        assert_eq!(meta["illustrator"][0]["name"], "Ivy Ill");
        assert_eq!(meta["artist"][0]["name"], "Art Person");
        assert_eq!(meta["colorist"][0]["name"], "Color Person");
        assert_eq!(meta["inker"][0]["name"], "Ink Person");
        assert_eq!(meta["letterer"][0]["name"], "Letter Person");
        assert_eq!(meta["penciler"][0]["name"], "Pencil Person");
        assert_eq!(meta["translator"][0]["name"], "Trans Person");
        assert_eq!(meta["publisher"][0]["name"], "Pub House");
        assert_eq!(meta["contributor"][0]["name"], "Ed Extra");
    }

    #[test]
    fn webpub_infer_page_count_from_page_bytes() {
        let pkg = roles_package();
        let mut sizes = HashMap::new();
        sizes.insert("ch1.xhtml".into(), 2048);
        sizes.insert("ch2.xhtml".into(), 100);
        let man = generate_readium_manifest(
            &pkg,
            &[],
            &[],
            "",
            &[],
            ReadiumManifestOptions {
                infer_page_count: true,
                page_bytes: Some(&sizes),
                ..Default::default()
            },
        );
        // ceil(2048/1024)=2 + ceil(100/1024)=1
        assert_eq!(man["metadata"]["numberOfPages"], 3);
    }

    #[test]
    fn webpub_infer_page_count_missing_bytes_counts_one() {
        let pkg = sample_package();
        let man = generate_readium_manifest(
            &pkg,
            &[],
            &[],
            "",
            &[],
            ReadiumManifestOptions {
                infer_page_count: true,
                page_bytes: None,
                ..Default::default()
            },
        );
        assert_eq!(man["metadata"]["numberOfPages"], 1);
    }

    #[test]
    fn webpub_toc_landmarks_page_list() {
        let pkg = sample_package();
        let toc = vec![WebPubNavLink {
            title: Some("Ch1".into()),
            href: "ch1.xhtml".into(),
            children: vec![WebPubNavLink {
                title: Some("Sec".into()),
                href: "ch1.xhtml#s".into(),
                children: vec![],
            }],
        }];
        let landmarks = vec![WebPubNavLink {
            title: Some("Bodymatter".into()),
            href: "ch1.xhtml".into(),
            children: vec![],
        }];
        let page_list = vec![WebPubNavLink {
            title: Some("1".into()),
            href: "ch1.xhtml#p1".into(),
            children: vec![],
        }];
        let man = generate_readium_manifest(
            &pkg,
            &[],
            &[],
            "",
            &[],
            ReadiumManifestOptions {
                toc: Some(&toc),
                landmarks: Some(&landmarks),
                page_list: Some(&page_list),
                ..Default::default()
            },
        );
        assert_eq!(man["toc"][0]["title"], "Ch1");
        assert_eq!(man["toc"][0]["children"][0]["href"], "ch1.xhtml#s");
        assert_eq!(man["landmarks"][0]["href"], "ch1.xhtml");
        assert_eq!(man["pageList"][0]["title"], "1");
    }
}
