//! Spine chapter exclusions — Storyteller `ctc/exclusions` + `exclusionRules` + `titleRules`.
//!
//! Multi-signal `classify_spine_item` + ranked `decide`, spine zones, FILENAME_TEMPLATES,
//! content-type share, mechanical sources, and locale title rules.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExclusionVerdict {
    Exclude,
    Review,
    Include,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExclusionSource {
    Linear,
    Nav,
    MediaType,
    Landmark,
    ContentType,
    Guide,
    Title,
    Filename,
    NoText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpineZone {
    Front,
    Body,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChapterDecision {
    Excluded,
    Included,
    Approved,
}

/// Storyteller `AudioExclusion` — track masks for CTC search excluded_ranges.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioExclusion {
    pub track: String,
    pub start: Option<f64>,
    pub end: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct AlignmentDecisions {
    /// Keyed by normalized href (no fragment).
    pub chapters: HashMap<String, ChapterDecision>,
    /// Audio ranges → frame masks via `audio_exclusion_frame_masks` (blank + search).
    pub audio: Vec<AudioExclusion>,
}

#[derive(Debug, Clone)]
pub struct ExclusionOptions {
    pub decisions: AlignmentDecisions,
    pub auto_exclude: bool,
}

impl Default for ExclusionOptions {
    fn default() -> Self {
        Self {
            decisions: AlignmentDecisions::default(),
            auto_exclude: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExclusionSignal {
    pub source: ExclusionSource,
    pub value: String,
    pub verdict: ExclusionVerdict,
    pub division: bool,
}

#[derive(Debug, Clone)]
pub struct SpineClassification {
    pub index: usize,
    pub id: String,
    pub href: String,
    pub zone: SpineZone,
    pub verdict: ExclusionVerdict,
    pub decided_by: Option<ExclusionSignal>,
    pub signals: Vec<ExclusionSignal>,
}

#[derive(Debug, Clone)]
pub struct SpineItemMeta {
    pub index: usize,
    pub spine_len: usize,
    pub id: String,
    pub href: String,
    pub linear: bool,
    pub media_type: String,
    pub properties: String,
}

#[derive(Debug, Clone, Default)]
pub struct EpubExclusionContext {
    pub language: String,
    /// Normalized href → landmark epub:type tokens.
    pub landmark_types: HashMap<String, Vec<String>>,
    /// Normalized href → guide reference types.
    pub guide_types: HashMap<String, Vec<String>>,
    /// Normalized href → TOC title (first wins).
    pub toc_titles: HashMap<String, String>,
    /// Normalized hrefs of manifest items with `properties` containing `nav`.
    pub nav_property_hrefs: HashSet<String>,
}

const ZONE_FRACTION: f64 = 0.15;
const ZONE_MIN_ITEMS: usize = 6;
const CONTENT_TYPE_SHARE: f64 = 0.5;

const EXCLUSION_SOURCES: &[ExclusionSource] = &[
    ExclusionSource::Linear,
    ExclusionSource::Nav,
    ExclusionSource::MediaType,
    ExclusionSource::Landmark,
    ExclusionSource::ContentType,
    ExclusionSource::Guide,
    ExclusionSource::Title,
    ExclusionSource::Filename,
    ExclusionSource::NoText,
];

fn source_index(source: ExclusionSource) -> usize {
    EXCLUSION_SOURCES
        .iter()
        .position(|&s| s == source)
        .unwrap_or(usize::MAX)
}

fn is_mechanical(source: ExclusionSource) -> bool {
    matches!(
        source,
        ExclusionSource::Linear
            | ExclusionSource::Nav
            | ExclusionSource::MediaType
            | ExclusionSource::NoText
    )
}

/// Storyteller `excludedByRule` — exclude decided by a non-mechanical source.
pub fn excluded_by_rule(classification: &SpineClassification) -> bool {
    classification.decided_by.as_ref().is_some_and(|sig| {
        classification.verdict == ExclusionVerdict::Exclude && !is_mechanical(sig.source)
    })
}

fn verdict_severity(v: ExclusionVerdict) -> u8 {
    match v {
        ExclusionVerdict::Exclude => 0,
        ExclusionVerdict::Review => 1,
        ExclusionVerdict::Include => 2,
    }
}

pub fn spine_zone(index: usize, length: usize) -> SpineZone {
    if length < 2 {
        return SpineZone::Body;
    }
    let fraction = index as f64 / (length - 1) as f64;
    let floor = ZONE_MIN_ITEMS.min(length / 2);
    if fraction <= ZONE_FRACTION || index < floor {
        return SpineZone::Front;
    }
    if fraction >= 1.0 - ZONE_FRACTION || index >= length.saturating_sub(floor) {
        return SpineZone::Back;
    }
    SpineZone::Body
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h1 = (bytes[i + 1] as char).to_digit(16);
            let h2 = (bytes[i + 2] as char).to_digit(16);
            if let (Some(a), Some(b)) = (h1, h2) {
                out.push((a * 16 + b) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn normalize_href(href: &str) -> String {
    let path = href.split('#').next().unwrap_or(href);
    percent_decode(path)
}

fn resolve_relative(base_file: &str, href: &str) -> String {
    let href = normalize_href(href);
    if href.starts_with('/') {
        return href.trim_start_matches('/').to_string();
    }
    let base_dir = Path::new(base_file)
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let joined = if base_dir.is_empty() {
        href
    } else {
        format!("{base_dir}/{href}")
    };
    let mut stack: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            stack.pop();
        } else {
            stack.push(part);
        }
    }
    stack.join("/")
}

fn filename_stem(href: &str) -> String {
    let path = normalize_href(href);
    let name = path.rsplit('/').next().unwrap_or(&path);
    name.rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(name)
        .to_string()
}

fn filename_tokens(href: &str) -> Vec<String> {
    let stem = filename_stem(href);
    let mut spaced = String::new();
    let chars: Vec<char> = stem.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            let prev = chars[i - 1];
            if prev.is_ascii_lowercase() || prev.is_ascii_digit() {
                spaced.push(' ');
            }
        }
        spaced.push(c);
    }
    spaced
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

fn tokens_include(tokens: &[String], phrase: &str) -> bool {
    let haystack = format!("-{}-", tokens.join("-"));
    let needle = format!(
        "-{}-",
        phrase
            .trim()
            .to_ascii_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-")
    );
    haystack.contains(&needle)
}

fn filename_template_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^index[-_]?\d*[-_]split[-_]\d+$").unwrap())
}

fn is_filename_template(href: &str) -> bool {
    filename_template_re().is_match(&filename_stem(href))
}

fn is_xhtml_media(media_type: &str) -> bool {
    media_type == "application/xhtml+xml" || media_type == "text/html"
}

#[derive(Debug, Clone, Copy)]
struct ExclusionRule {
    source: ExclusionSource,
    match_str: &'static str,
    verdict: ExclusionVerdict,
    zones: Option<&'static [SpineZone]>,
    division: bool,
}

const FRONT_BACK: &[SpineZone] = &[SpineZone::Front, SpineZone::Back];
const BACK_ONLY: &[SpineZone] = &[SpineZone::Back];
const FRONT_ONLY: &[SpineZone] = &[SpineZone::Front];

const STRUCTURAL_TYPES: &[&str] = &[
    "cover",
    "titlepage",
    "halftitlepage",
    "toc",
    "toc-brief",
    "landmarks",
    "page-list",
    "loi",
    "lot",
    "loa",
    "lov",
    "index",
    "glossary",
    "bibliography",
    "copyright-page",
    "colophon",
    "contributors",
    "seriespage",
    "imprint",
    "imprimatur",
    "errata",
    "revision-history",
    "other-credits",
    "footnote",
    "footnotes",
    "endnote",
    "endnotes",
    "rearnote",
    "rearnotes",
];

const SOMETIMES_NARRATED: &[&str] = &[
    "dedication",
    "epigraph",
    "acknowledgments",
    "acknowledgements",
    "foreword",
    "preface",
    "introduction",
    "appendix",
    "afterword",
    "preamble",
    "abstract",
];

const NARRATED_TYPES: &[&str] = &[
    "chapter",
    "part",
    "volume",
    "division",
    "prologue",
    "epilogue",
    "conclusion",
];

fn exclusion_rules() -> Vec<ExclusionRule> {
    let mut rules = Vec::new();
    let push_types = |rules: &mut Vec<ExclusionRule>,
                      source: ExclusionSource,
                      values: &'static [&'static str],
                      verdict: ExclusionVerdict| {
        for m in values {
            rules.push(ExclusionRule {
                source,
                match_str: m,
                verdict,
                zones: None,
                division: false,
            });
        }
    };
    for source in [ExclusionSource::Landmark, ExclusionSource::ContentType] {
        push_types(
            &mut rules,
            source,
            STRUCTURAL_TYPES,
            ExclusionVerdict::Exclude,
        );
        push_types(
            &mut rules,
            source,
            SOMETIMES_NARRATED,
            ExclusionVerdict::Review,
        );
        push_types(
            &mut rules,
            source,
            NARRATED_TYPES,
            ExclusionVerdict::Include,
        );
        for (m, v) in [
            ("frontmatter", ExclusionVerdict::Review),
            ("bodymatter", ExclusionVerdict::Include),
            ("backmatter", ExclusionVerdict::Review),
        ] {
            rules.push(ExclusionRule {
                source,
                match_str: m,
                verdict: v,
                zones: None,
                division: true,
            });
        }
    }

    let guide = |m, v| ExclusionRule {
        source: ExclusionSource::Guide,
        match_str: m,
        verdict: v,
        zones: None,
        division: false,
    };
    for (m, v) in [
        ("cover", ExclusionVerdict::Exclude),
        ("title-page", ExclusionVerdict::Exclude),
        ("toc", ExclusionVerdict::Exclude),
        ("index", ExclusionVerdict::Exclude),
        ("glossary", ExclusionVerdict::Exclude),
        ("bibliography", ExclusionVerdict::Exclude),
        ("colophon", ExclusionVerdict::Exclude),
        ("copyright-page", ExclusionVerdict::Exclude),
        ("loi", ExclusionVerdict::Exclude),
        ("lot", ExclusionVerdict::Exclude),
        ("notes", ExclusionVerdict::Exclude),
        ("acknowledgements", ExclusionVerdict::Review),
        ("dedication", ExclusionVerdict::Review),
        ("epigraph", ExclusionVerdict::Review),
        ("foreword", ExclusionVerdict::Review),
        ("preface", ExclusionVerdict::Review),
    ] {
        rules.push(guide(m, v));
    }

    let filename = |m, v, zones: Option<&'static [SpineZone]>| ExclusionRule {
        source: ExclusionSource::Filename,
        match_str: m,
        verdict: v,
        zones,
        division: false,
    };
    let fb = Some(FRONT_BACK);
    let back = Some(BACK_ONLY);
    let front = Some(FRONT_ONLY);
    for (m, v, z) in [
        ("cover", ExclusionVerdict::Exclude, fb),
        ("titlepage", ExclusionVerdict::Exclude, fb),
        ("title page", ExclusionVerdict::Exclude, fb),
        ("halftitle", ExclusionVerdict::Exclude, fb),
        ("half title", ExclusionVerdict::Exclude, fb),
        ("copyright", ExclusionVerdict::Exclude, fb),
        ("toc", ExclusionVerdict::Exclude, fb),
        ("contents", ExclusionVerdict::Exclude, fb),
        ("index", ExclusionVerdict::Exclude, back),
        ("glossary", ExclusionVerdict::Exclude, fb),
        ("bibliography", ExclusionVerdict::Exclude, fb),
        ("colophon", ExclusionVerdict::Exclude, fb),
        ("praise", ExclusionVerdict::Exclude, fb),
        ("map", ExclusionVerdict::Exclude, fb),
        ("maps", ExclusionVerdict::Exclude, fb),
        ("about the author", ExclusionVerdict::Exclude, fb),
        ("about author", ExclusionVerdict::Exclude, fb),
        ("author bio", ExclusionVerdict::Exclude, fb),
        ("about the publisher", ExclusionVerdict::Exclude, fb),
        ("about publisher", ExclusionVerdict::Exclude, fb),
        ("also by", ExclusionVerdict::Exclude, fb),
        ("alsoby", ExclusionVerdict::Exclude, fb),
        ("backad", ExclusionVerdict::Exclude, fb),
        ("torad", ExclusionVerdict::Exclude, fb),
        ("newsletter", ExclusionVerdict::Exclude, fb),
        ("signup", ExclusionVerdict::Exclude, fb),
        ("sign up", ExclusionVerdict::Exclude, fb),
        ("next reads", ExclusionVerdict::Exclude, fb),
        ("discover page", ExclusionVerdict::Exclude, fb),
        ("teaser", ExclusionVerdict::Exclude, back),
        ("excerpt", ExclusionVerdict::Exclude, back),
        ("preview", ExclusionVerdict::Exclude, back),
        ("credits", ExclusionVerdict::Exclude, back),
        ("contributors", ExclusionVerdict::Exclude, fb),
        ("series", ExclusionVerdict::Exclude, back),
        ("imprint", ExclusionVerdict::Exclude, fb),
        ("legal", ExclusionVerdict::Exclude, fb),
        ("notice", ExclusionVerdict::Exclude, front),
        ("endnotes", ExclusionVerdict::Exclude, back),
        ("footnotes", ExclusionVerdict::Exclude, back),
        ("loi", ExclusionVerdict::Exclude, fb),
        ("lot", ExclusionVerdict::Exclude, fb),
        ("lof", ExclusionVerdict::Exclude, fb),
        ("dedication", ExclusionVerdict::Review, fb),
        ("epigraph", ExclusionVerdict::Review, fb),
        ("acknowledgments", ExclusionVerdict::Review, fb),
        ("acknowledgements", ExclusionVerdict::Review, fb),
        ("acknowledgment", ExclusionVerdict::Review, fb),
        ("acknowledgement", ExclusionVerdict::Review, fb),
        ("ack", ExclusionVerdict::Review, fb),
        ("acks", ExclusionVerdict::Review, fb),
        ("appendix", ExclusionVerdict::Review, fb),
        ("foreword", ExclusionVerdict::Review, fb),
        ("preface", ExclusionVerdict::Review, fb),
        ("introduction", ExclusionVerdict::Review, fb),
        ("intro", ExclusionVerdict::Review, fb),
        ("afterword", ExclusionVerdict::Review, fb),
        ("extract", ExclusionVerdict::Review, back),
    ] {
        rules.push(filename(m, v, z));
    }
    rules
}

fn rules_cached() -> &'static [ExclusionRule] {
    static RULES: OnceLock<Vec<ExclusionRule>> = OnceLock::new();
    RULES.get_or_init(exclusion_rules).as_slice()
}

fn rule_signal(source: ExclusionSource, value: &str) -> Option<ExclusionSignal> {
    for rule in rules_cached() {
        if rule.source != source || source == ExclusionSource::Filename {
            continue;
        }
        if rule.match_str != value {
            continue;
        }
        return Some(ExclusionSignal {
            source,
            value: value.to_string(),
            verdict: rule.verdict,
            division: rule.division,
        });
    }
    None
}

fn filename_signals(href: &str, zone: SpineZone) -> Vec<ExclusionSignal> {
    if is_filename_template(href) {
        return Vec::new();
    }
    let tokens = filename_tokens(href);
    let mut signals = Vec::new();
    for rule in rules_cached() {
        if rule.source != ExclusionSource::Filename {
            continue;
        }
        if let Some(zones) = rule.zones {
            if !zones.contains(&zone) {
                continue;
            }
        }
        if !tokens_include(&tokens, rule.match_str) {
            continue;
        }
        signals.push(ExclusionSignal {
            source: ExclusionSource::Filename,
            value: rule.match_str.to_string(),
            verdict: rule.verdict,
            division: false,
        });
    }
    signals
}

fn source_rank(signal: &ExclusionSignal) -> f64 {
    if signal.source == ExclusionSource::ContentType && signal.division {
        return source_index(ExclusionSource::Filename) as f64 + 0.5;
    }
    source_index(signal.source) as f64
}

fn within_source_rank(signal: &ExclusionSignal) -> u8 {
    if signal.source == ExclusionSource::ContentType {
        return match signal.verdict {
            ExclusionVerdict::Include => 0,
            ExclusionVerdict::Exclude => 1,
            ExclusionVerdict::Review => 2,
        };
    }
    verdict_severity(signal.verdict)
}

pub fn decide(signals: &[ExclusionSignal]) -> Option<ExclusionSignal> {
    let mut best: Option<ExclusionSignal> = None;
    for signal in signals {
        match &best {
            None => best = Some(signal.clone()),
            Some(b) => {
                let by_source = source_rank(signal) - source_rank(b);
                if by_source < 0.0
                    || (by_source == 0.0 && within_source_rank(signal) < within_source_rank(b))
                {
                    best = Some(signal.clone());
                }
            }
        }
    }
    best
}

fn attr_value(attrs: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(r#"{name}\s*=\s*"([^"]+)""#)).ok()?;
    re.captures(attrs)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        .or_else(|| {
            let re = Regex::new(&format!(r#"{name}\s*=\s*'([^']+)'"#)).ok()?;
            re.captures(attrs)
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        })
}

fn text_len_no_ws(s: &str) -> usize {
    s.chars().filter(|c| !c.is_whitespace()).count()
}

/// Strip tags and return text content length (whitespace-collapsed away).
fn element_text_len(html: &str) -> usize {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        if c == '<' {
            in_tag = true;
            continue;
        }
        if c == '>' {
            in_tag = false;
            continue;
        }
        if !in_tag {
            out.push(c);
        }
    }
    text_len_no_ws(&out)
}

fn find_body_inner(xhtml: &str) -> Option<&str> {
    let lower = xhtml.to_ascii_lowercase();
    let start = lower.find("<body")?;
    let after = &xhtml[start..];
    let gt = after.find('>')?;
    let inner_start = start + gt + 1;
    let end = lower[inner_start..].rfind("</body>")?;
    Some(&xhtml[inner_start..inner_start + end])
}

fn body_epub_types(xhtml: &str) -> Vec<String> {
    let lower = xhtml.to_ascii_lowercase();
    let Some(start) = lower.find("<body") else {
        return Vec::new();
    };
    let after = &xhtml[start..];
    let Some(gt) = after.find('>') else {
        return Vec::new();
    };
    let tag = &after[..=gt];
    attr_value(tag, "epub:type")
        .unwrap_or_default()
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// Walk children: outermost elements with epub:type contribute their text length.
fn outermost_types(fragment: &str, into: &mut HashMap<String, usize>) {
    let re = Regex::new(r"(?is)<([a-zA-Z][\w:-]*)\b([^>]*)>").unwrap();
    let mut pos = 0usize;
    while pos < fragment.len() {
        let Some(caps) = re.captures(&fragment[pos..]) else {
            break;
        };
        let m = caps.get(0).unwrap();
        // m.start()/end() are relative to fragment[pos..] — do not add m.start() twice.
        let tag_name = caps.get(1).unwrap().as_str().to_ascii_lowercase();
        let attrs = caps.get(2).map(|x| x.as_str()).unwrap_or("");
        let open_end = pos + m.end();

        if tag_name.starts_with('!') || tag_name == "!--" || attrs.trim_end().ends_with('/') {
            pos = open_end;
            continue;
        }
        // void-ish
        if matches!(
            tag_name.as_str(),
            "br" | "hr" | "img" | "meta" | "link" | "input" | "source" | "wbr"
        ) {
            pos = open_end;
            continue;
        }

        let close = format!("</{tag_name}>");
        let close_l = close.to_ascii_lowercase();
        let rest_l = fragment[open_end..].to_ascii_lowercase();
        let Some(close_rel) = find_matching_close(&rest_l, &tag_name, &close_l) else {
            pos = open_end;
            continue;
        };
        let inner = &fragment[open_end..open_end + close_rel];
        let elem_end = open_end + close_rel + close.len();

        if let Some(ty) = attr_value(attrs, "epub:type") {
            let length = element_text_len(inner);
            for value in ty.split_whitespace().filter(|t| !t.is_empty()) {
                let key = value.to_ascii_lowercase();
                *into.entry(key).or_insert(0) += length;
            }
        } else {
            outermost_types(inner, into);
        }
        pos = elem_end;
    }
}

fn find_matching_close(rest_lower: &str, tag_name: &str, close_l: &str) -> Option<usize> {
    let open_pat = format!("<{tag_name}");
    let mut depth = 1i32;
    let mut i = 0usize;
    while i < rest_lower.len() {
        if rest_lower[i..].starts_with(close_l) {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
            i += close_l.len();
            continue;
        }
        if rest_lower[i..].starts_with(&open_pat) {
            let after = &rest_lower[i + open_pat.len()..];
            let boundary = after
                .chars()
                .next()
                .is_some_and(|c| c == '>' || c == '/' || c.is_whitespace());
            if boundary {
                if let Some(gt) = after.find('>') {
                    let tag = &after[..=gt];
                    if !tag.trim_end().ends_with('/') && !tag.contains("/>") {
                        depth += 1;
                    }
                    i += open_pat.len() + gt + 1;
                    continue;
                }
            }
        }
        i += rest_lower[i..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
    }
    None
}

pub fn content_types(xhtml: &str) -> Vec<String> {
    let mut found: HashSet<String> = body_epub_types(xhtml).into_iter().collect();
    let Some(inner) = find_body_inner(xhtml) else {
        return found.into_iter().collect();
    };
    let mut typed: HashMap<String, usize> = HashMap::new();
    outermost_types(inner, &mut typed);
    let total = element_text_len(inner);
    for (ty, length) in typed {
        if total == 0 || (length as f64) / (total as f64) >= CONTENT_TYPE_SHARE {
            found.insert(ty);
        }
    }
    found.into_iter().collect()
}

fn has_text(xhtml: &str) -> bool {
    let Some(inner) = find_body_inner(xhtml) else {
        return false;
    };
    element_text_len(inner) > 0
}

fn first_heading_text(xhtml: &str) -> Option<String> {
    let lower = xhtml.to_ascii_lowercase();
    for tag in ["h1", "h2", "h3"] {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        let mut search = 0usize;
        while let Some(rel) = lower[search..].find(&open) {
            let abs = search + rel;
            let after_lt = &xhtml[abs..];
            let gt = after_lt.find('>')?;
            let inner_start = abs + gt + 1;
            if let Some(end_rel) = lower[inner_start..].find(&close) {
                let raw = &xhtml[inner_start..inner_start + end_rel];
                let text = strip_tags(raw)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if !text.is_empty() {
                    return Some(text);
                }
                search = inner_start + end_rel + close.len();
            } else {
                break;
            }
        }
    }
    None
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        if c == '<' {
            in_tag = true;
            continue;
        }
        if c == '>' {
            in_tag = false;
            continue;
        }
        if !in_tag {
            out.push(c);
        }
    }
    out
}

pub fn normalize_title(title: &str) -> String {
    // ST `normalizeTitle`: NFKC → lower → strip apostrophes (Shelf also collapses
    // other non-alnum to spaces for titleRules matching).
    let mut s = String::new();
    for c in title.nfkc() {
        if matches!(c, '\'' | '\u{2019}' | '\u{02BC}' | '\u{2018}') {
            continue;
        }
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                s.push(lc);
            }
        } else {
            s.push(' ');
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug)]
struct TitleRuleCompiled {
    match_str: &'static str,
    verdict: ExclusionVerdict,
    zones: &'static [SpineZone],
    /// language code → compiled patterns
    patterns: HashMap<&'static str, Vec<Regex>>,
}

const ANYWHERE: &[SpineZone] = &[SpineZone::Front, SpineZone::Body, SpineZone::Back];
const EDGES: &[SpineZone] = &[SpineZone::Front, SpineZone::Back];

fn title_rules_cached() -> &'static [TitleRuleCompiled] {
    static RULES: OnceLock<Vec<TitleRuleCompiled>> = OnceLock::new();
    RULES.get_or_init(|| {
        let mut out = Vec::new();

        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^copyright( page| notice| information)?$").unwrap(),
                Regex::new(r"^legal notice$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^auteursrecht$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^impressum$").unwrap(),
                Regex::new(r"^urheberrecht$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^mentions l[eé]gales$").unwrap(),
                Regex::new(r"^droits dauteur$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^derechos de autor$").unwrap(),
                Regex::new(r"^aviso legal$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^diritti dautore$").unwrap(),
                Regex::new(r"^note legali$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^direitos autorais$").unwrap(),
                Regex::new(r"^ficha t[eé]cnica$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "copyright-page",
                verdict: ExclusionVerdict::Exclude,
                zones: ANYWHERE,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^colophon$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^colofon$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^colof[óo]n$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "colophon",
                verdict: ExclusionVerdict::Exclude,
                zones: ANYWHERE,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^(front )?cover$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^(voor)?omslag$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^umschlag$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^couverture$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^(cubierta|portada)$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^copertina$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^capa$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "cover",
                verdict: ExclusionVerdict::Exclude,
                zones: FRONT_ONLY,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^(half )?title page$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^titelpagina$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^titelseite$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^page de titre$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "titlepage",
                verdict: ExclusionVerdict::Exclude,
                zones: FRONT_ONLY,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^(table of )?contents$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^inhoud(sopgave)?$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^inhalt(sverzeichnis)?$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^table des mati[èe]res$").unwrap(),
                Regex::new(r"^sommaire$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^[íi]ndice$").unwrap(),
                Regex::new(r"^contenidos?$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^indice$").unwrap(),
                Regex::new(r"^sommario$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^[íi]ndice$").unwrap(),
                Regex::new(r"^sum[áa]rio$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "toc",
                verdict: ExclusionVerdict::Exclude,
                zones: EDGES,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^maps?$").unwrap(),
                Regex::new(r"^map of ").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^(land)?kaart(en)?$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^karten?$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^cartes?$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^mapas?$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^(mappa|mappe|cartina|cartine)$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^mapas?$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "maps",
                verdict: ExclusionVerdict::Exclude,
                zones: EDGES,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^(advance )?praise( for .+)?$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^(lof voor|pers over) ").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "praise",
                verdict: ExclusionVerdict::Exclude,
                zones: EDGES,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^(also|other books|more books) by ").unwrap(),
                Regex::new(r"^by the same author$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^(ook|eerder verschenen) (van|bij) ").unwrap(),
                Regex::new(r"^van dezelfde (auteur|schrijver)$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^vom selben autor$").unwrap(),
                Regex::new(r"^von derselben autorin$").unwrap(),
                Regex::new(r"^weitere (b[üu]cher|titel) von ").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^du m[êe]me auteur$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^del mismo autor$").unwrap(),
                Regex::new(r"^otr[oa]s (libros|obras) de ").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^dello stesso autore$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^do mesmo autor$").unwrap(),
                Regex::new(r"^outros livros d[oa] ").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "also-by",
                verdict: ExclusionVerdict::Exclude,
                zones: EDGES,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^about the authors?$").unwrap(),
                Regex::new(r"^meet the author$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^over de (auteurs?|schrijver)$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^[üu]ber (den autor|die autorin)$").unwrap(),
                Regex::new(r"^(zum autor|zur autorin)$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^[àa] propos de lauteur$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^(sobre el|acerca del) autora?$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^nota biografica$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^sobre (o autor|a autora)$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "about-the-author",
                verdict: ExclusionVerdict::Exclude,
                zones: EDGES,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^about the publisher$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^over de uitgever(ij)?$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^[üu]ber den verlag$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "about-the-publisher",
                verdict: ExclusionVerdict::Exclude,
                zones: EDGES,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^(connect|follow) (with|on) ").unwrap(),
                Regex::new(r"^(sign up|subscribe) (for|to) ").unwrap(),
                Regex::new(r"^(newsletter|join our mailing list)$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^nieuwsbrief$").unwrap(),
                Regex::new(r"^volg (ons|de auteur) ").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "newsletter",
                verdict: ExclusionVerdict::Exclude,
                zones: EDGES,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^(an )?(excerpt|extract|preview|sample) (from|of) ").unwrap(),
                Regex::new(r"^(read )?(an excerpt|a preview|a sneak peek)( .+)?$").unwrap(),
                Regex::new(r"^sneak peek$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^(een )?(fragment|voorproefje) (uit|van) ").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^leseprobe( .+)?$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^(un )?extrait (de|du|des) ").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^(un )?(fragmento|avance) de ").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^(un )?(estratto|anteprima) (da|di) ").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^(um )?(trecho|excerto) de ").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "excerpt",
                verdict: ExclusionVerdict::Exclude,
                zones: BACK_ONLY,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^(discussion questions|questions for discussion)$").unwrap(),
                Regex::new(r"^(a )?(reading group|readers|book club) (guide|questions|kit)$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^(leesclubvragen|leesgroepvragen|discussievragen)$").unwrap(),
                Regex::new(r"^vragen voor de leesclub$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^(fragen f[üu]r (den )?lesekreis|lesekreisfragen)$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^(guide de lecture|questions pour les clubs de lecture)$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^gu[íi]a de lectura$").unwrap(),
                Regex::new(r"^preguntas para (el )?club de lectura$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^guida alla lettura$").unwrap(),
                Regex::new(r"^domande per (il )?gruppo di lettura$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^guia de leitura$").unwrap(),
                Regex::new(r"^perguntas para (o )?clube de leitura$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "discussion-questions",
                verdict: ExclusionVerdict::Exclude,
                zones: ANYWHERE,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^(end)?notes$").unwrap(),
                Regex::new(r"^(source )?notes( and)? sources$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^(noten|eindnoten|aantekeningen)$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^(anmerkungen|endnoten)$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^notes$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^notas$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^note$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^notas$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "endnotes",
                verdict: ExclusionVerdict::Exclude,
                zones: BACK_ONLY,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^(general )?index$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^(register|index)$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^(register|index|stichwortverzeichnis|personenregister)$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^index$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^[íi]ndice (alfab[ée]tico|onom[áa]stico|anal[íi]tico)$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^indice (analitico|dei nomi)$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^[íi]ndice remissivo$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "index",
                verdict: ExclusionVerdict::Exclude,
                zones: BACK_ONLY,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^((selected )?bibliography|references|works cited|sources|further reading)$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^(bibliografie|literatuur(lijst)?|bronnen|bronvermelding|verantwoording)$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^(bibliographie|bibliografie|literatur(verzeichnis)?|quellen(verzeichnis)?)$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^(bibliographie|sources|r[ée]f[ée]rences)$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^(bibliograf[íi]a|referencias|fuentes)$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^(bibliografia|fonti)$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^(bibliografia|refer[êe]ncias|fontes)$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "bibliography",
                verdict: ExclusionVerdict::Exclude,
                zones: BACK_ONLY,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^glossary$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^(verklarende )?woordenlijst$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^glossar$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^glossaire$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^glosario$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^glossario$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^gloss[áa]rio$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "glossary",
                verdict: ExclusionVerdict::Exclude,
                zones: BACK_ONLY,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^((photo|image|illustration|text) )?credits$").unwrap(),
                Regex::new(r"^permissions$").unwrap(),
                Regex::new(r"^a note on the type$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^(foto|beeld)verantwoording$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^cr[ée]ditos$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^cr[ée]ditos$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "credits",
                verdict: ExclusionVerdict::Exclude,
                zones: BACK_ONLY,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^dedication$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^opdracht$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^widmung$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^d[ée]dicace$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^dedicatoria$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^dedica$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^dedicat[óo]ria$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "dedication",
                verdict: ExclusionVerdict::Review,
                zones: EDGES,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^epigraphs?$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^motto$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^motto$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^[ée]pigraphe$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^ep[íi]grafe$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^epigrafe$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^ep[íi]grafe$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "epigraph",
                verdict: ExclusionVerdict::Review,
                zones: EDGES,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^acknowledge?ments?$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^(dankwoord|dankbetuiging)$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^dank(sagung)?$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^remerciements?$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^agradecimientos?$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^ringraziamenti$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^agradecimentos?$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "acknowledgments",
                verdict: ExclusionVerdict::Review,
                zones: EDGES,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^foreword$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^voorwoord$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^vorwort$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^avant propos$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "foreword",
                verdict: ExclusionVerdict::Review,
                zones: EDGES,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^preface$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^pr[ée]face$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^prefacio$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^prefazione$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^pref[áa]cio$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "preface",
                verdict: ExclusionVerdict::Review,
                zones: EDGES,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^introduction$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^inleiding$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^einleitung$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^introduction$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^introducci[óo]n$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^introduzione$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^introdu[çc][ãa]o$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "introduction",
                verdict: ExclusionVerdict::Review,
                zones: EDGES,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^(a )?note (from|by) the author$").unwrap(),
                Regex::new(r"^authors? note$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^(noot|woord) van de (auteur|schrijver)$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^anmerkung (des autors|der autorin)$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^note de lauteur$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^nota del autor$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^nota dellautore$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^nota do autor$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "authors-note",
                verdict: ExclusionVerdict::Review,
                zones: EDGES,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^afterword$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^nawoord$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^nachwort$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^postface$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^postfazione$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^posf[áa]cio$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "afterword",
                verdict: ExclusionVerdict::Review,
                zones: BACK_ONLY,
                patterns,
            });
        }
        {
            let mut patterns: HashMap<&'static str, Vec<Regex>> = HashMap::new();
            patterns.insert("en", vec![
                Regex::new(r"^appendix( [a-z0-9]+)?$").unwrap(),
                Regex::new(r"^appendices$").unwrap(),
            ]);
            patterns.insert("nl", vec![
                Regex::new(r"^(bijlage|appendix)( [a-z0-9]+)?$").unwrap(),
            ]);
            patterns.insert("de", vec![
                Regex::new(r"^anhang( [a-z0-9]+)?$").unwrap(),
            ]);
            patterns.insert("fr", vec![
                Regex::new(r"^annexes?( [a-z0-9]+)?$").unwrap(),
            ]);
            patterns.insert("es", vec![
                Regex::new(r"^ap[ée]ndices?( [a-z0-9]+)?$").unwrap(),
            ]);
            patterns.insert("it", vec![
                Regex::new(r"^appendic[ei]( [a-z0-9]+)?$").unwrap(),
            ]);
            patterns.insert("pt", vec![
                Regex::new(r"^ap[êe]ndices?( [a-z0-9]+)?$").unwrap(),
            ]);
            out.push(TitleRuleCompiled {
                match_str: "appendix",
                verdict: ExclusionVerdict::Review,
                zones: BACK_ONLY,
                patterns,
            });
        }

        out
    }).as_slice()
}

pub fn title_signals(title: &str, zone: SpineZone, language: &str) -> Vec<ExclusionSignal> {
    let normalized = normalize_title(title);
    if normalized.is_empty() {
        return Vec::new();
    }
    let languages: Vec<&str> = if language == "en" {
        vec!["en"]
    } else {
        vec![language, "en"]
    };
    let mut signals = Vec::new();
    for rule in title_rules_cached() {
        if !rule.zones.contains(&zone) {
            continue;
        }
        let mut matched = false;
        for code in &languages {
            if let Some(pats) = rule.patterns.get(code) {
                if pats.iter().any(|p| p.is_match(&normalized)) {
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            signals.push(ExclusionSignal {
                source: ExclusionSource::Title,
                value: rule.match_str.to_string(),
                verdict: rule.verdict,
                division: false,
            });
        }
    }
    signals
}

fn map_lookup_types(map: &HashMap<String, Vec<String>>, href: &str) -> Vec<String> {
    let key = normalize_href(href);
    if let Some(v) = map.get(&key) {
        return v.clone();
    }
    for (k, v) in map {
        if key.ends_with(k) || k.ends_with(&key) {
            return v.clone();
        }
    }
    Vec::new()
}

fn map_lookup_title(map: &HashMap<String, String>, href: &str) -> Option<String> {
    let key = normalize_href(href);
    if let Some(v) = map.get(&key) {
        return Some(v.clone());
    }
    for (k, v) in map {
        if key.ends_with(k) || k.ends_with(&key) {
            return Some(v.clone());
        }
    }
    None
}

fn href_in_set(set: &HashSet<String>, href: &str) -> bool {
    let key = normalize_href(href);
    if set.contains(&key) {
        return true;
    }
    set.iter().any(|k| key.ends_with(k) || k.ends_with(&key))
}

/// Classify one spine item (Storyteller `classifySpine` per-item body).
pub fn classify_spine_item(
    item: &SpineItemMeta,
    xhtml: Option<&str>,
    ctx: &EpubExclusionContext,
) -> SpineClassification {
    let zone = spine_zone(item.index, item.spine_len);
    let mut signals: Vec<ExclusionSignal> = Vec::new();

    if !item.linear {
        signals.push(ExclusionSignal {
            source: ExclusionSource::Linear,
            value: "no".into(),
            verdict: ExclusionVerdict::Exclude,
            division: false,
        });
    }
    if item.properties.split_whitespace().any(|p| p == "nav")
        || href_in_set(&ctx.nav_property_hrefs, &item.href)
    {
        signals.push(ExclusionSignal {
            source: ExclusionSource::Nav,
            value: "nav".into(),
            verdict: ExclusionVerdict::Exclude,
            division: false,
        });
    }

    let media_type = if item.media_type.is_empty() {
        "application/xhtml+xml"
    } else {
        item.media_type.as_str()
    };
    let is_document = is_xhtml_media(media_type);
    if !is_document {
        signals.push(ExclusionSignal {
            source: ExclusionSource::MediaType,
            value: media_type.to_string(),
            verdict: ExclusionVerdict::Exclude,
            division: false,
        });
    }

    for ty in map_lookup_types(&ctx.landmark_types, &item.href) {
        if let Some(signal) = rule_signal(ExclusionSource::Landmark, &ty) {
            signals.push(signal);
        }
    }

    if let Some(xml) = xhtml {
        for ty in content_types(xml) {
            if let Some(signal) = rule_signal(ExclusionSource::ContentType, &ty) {
                signals.push(signal);
            }
        }
    }

    for ty in map_lookup_types(&ctx.guide_types, &item.href) {
        if let Some(signal) = rule_signal(ExclusionSource::Guide, &ty) {
            signals.push(signal);
        }
    }

    let title = map_lookup_title(&ctx.toc_titles, &item.href)
        .or_else(|| xhtml.and_then(first_heading_text));
    if let Some(title) = title {
        signals.extend(title_signals(&title, zone, &ctx.language));
    }

    signals.extend(filename_signals(&item.href, zone));

    if let Some(xml) = xhtml {
        if !has_text(xml) {
            signals.push(ExclusionSignal {
                source: ExclusionSource::NoText,
                value: String::new(),
                verdict: ExclusionVerdict::Exclude,
                division: false,
            });
        }
    }

    let decided_by = decide(&signals);
    let verdict = decided_by
        .as_ref()
        .map(|s| s.verdict)
        .unwrap_or(ExclusionVerdict::Include);

    SpineClassification {
        index: item.index,
        id: item.id.clone(),
        href: item.href.clone(),
        zone,
        verdict,
        decided_by,
        signals,
    }
}

/// Storyteller `chapterAligned`.
pub fn chapter_aligned_from(
    classification: &SpineClassification,
    options: &ExclusionOptions,
) -> bool {
    let key = normalize_href(&classification.href);
    if let Some(decision) = options.decisions.chapters.get(&key) {
        return !matches!(decision, ChapterDecision::Excluded);
    }
    // Also try suffix keys used by callers.
    for (k, decision) in &options.decisions.chapters {
        if key.ends_with(k) || k.ends_with(&key) {
            return !matches!(decision, ChapterDecision::Excluded);
        }
    }
    if classification.verdict != ExclusionVerdict::Exclude || classification.decided_by.is_none() {
        return true;
    }
    if options.auto_exclude {
        return false;
    }
    let src = classification.decided_by.as_ref().unwrap().source;
    !is_mechanical(src)
}

pub fn is_xhtml_document(media_type: &str) -> bool {
    is_xhtml_media(media_type)
}

fn insert_types(
    map: &mut HashMap<String, Vec<String>>,
    href: &str,
    types: impl IntoIterator<Item = String>,
) {
    let key = normalize_href(href);
    if key.is_empty() {
        return;
    }
    let list = map.entry(key).or_default();
    for t in types {
        if !t.is_empty() {
            list.push(t);
        }
    }
}

fn dc_language(opf: &str) -> String {
    let re = Regex::new(r"(?is)<dc:language[^>]*>([^<]+)</dc:language>").ok();
    if let Some(re) = re {
        if let Some(c) = re.captures(opf) {
            let raw = c.get(1).map(|m| m.as_str().trim()).unwrap_or("en");
            // BCP47 primary subtag
            return raw
                .split(['-', '_'])
                .next()
                .unwrap_or("en")
                .to_ascii_lowercase();
        }
    }
    "en".into()
}

/// First manifest href whose `properties` include `nav`.
pub fn nav_document_href(opf: &str) -> Option<String> {
    let re = Regex::new(r#"(?is)<item\b([^>]*?)/?>"#).ok()?;
    for caps in re.captures_iter(opf) {
        let attrs = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let props = attr_value(attrs, "properties").unwrap_or_default();
        if !props.split_whitespace().any(|p| p == "nav") {
            continue;
        }
        let href = attr_value(attrs, "href").unwrap_or_default();
        if !href.is_empty() {
            return Some(normalize_href(&href));
        }
    }
    None
}

pub fn nav_property_exclude_hrefs(opf: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let Ok(re) = Regex::new(r#"(?is)<item\b([^>]*?)/?>"#) else {
        return out;
    };
    for caps in re.captures_iter(opf) {
        let attrs = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let props = attr_value(attrs, "properties").unwrap_or_default();
        if !props.split_whitespace().any(|p| p == "nav") {
            continue;
        }
        let href = attr_value(attrs, "href").unwrap_or_default();
        if !href.is_empty() {
            out.insert(normalize_href(&href));
        }
    }
    out
}

fn parse_guide_types(opf: &str) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    let Ok(re) = Regex::new(r#"(?is)<reference\b([^>]*?)/?>"#) else {
        return map;
    };
    for caps in re.captures_iter(opf) {
        let attrs = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let typ = attr_value(attrs, "type").unwrap_or_default();
        let href = attr_value(attrs, "href").unwrap_or_default();
        if !href.is_empty() && !typ.is_empty() {
            insert_types(&mut map, &href, [typ]);
        }
    }
    map
}

fn parse_nav_landmarks_and_toc(
    nav_xhtml: &str,
    nav_href: &str,
) -> (HashMap<String, Vec<String>>, HashMap<String, String>) {
    let mut landmarks = HashMap::new();
    let mut toc = HashMap::new();
    let Ok(nav_re) = Regex::new(r#"(?is)<nav\b([^>]*)>(.*?)</nav>"#) else {
        return (landmarks, toc);
    };
    let Ok(a_re) = Regex::new(r#"(?is)<a\b([^>]*?)>(.*?)</a>"#) else {
        return (landmarks, toc);
    };
    for caps in nav_re.captures_iter(nav_xhtml) {
        let attrs = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let body = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let types: Vec<String> = attr_value(attrs, "epub:type")
            .unwrap_or_default()
            .split_whitespace()
            .map(|t| t.to_ascii_lowercase())
            .collect();
        let is_landmarks = types.iter().any(|t| t == "landmarks");
        let is_toc = types.iter().any(|t| t == "toc");
        for a_caps in a_re.captures_iter(body) {
            let a_attrs = a_caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let a_text = strip_tags(a_caps.get(2).map(|m| m.as_str()).unwrap_or(""))
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let href_raw = attr_value(a_attrs, "href").unwrap_or_default();
            if href_raw.is_empty() {
                continue;
            }
            let resolved = resolve_relative(nav_href, &href_raw);
            if is_landmarks {
                let roles = attr_value(a_attrs, "epub:type")
                    .unwrap_or_default()
                    .split_whitespace()
                    .filter(|t| !t.is_empty())
                    .map(|t| t.to_ascii_lowercase())
                    .collect::<Vec<_>>();
                if !roles.is_empty() {
                    insert_types(&mut landmarks, &resolved, roles);
                }
            }
            if is_toc && !a_text.is_empty() {
                let key = normalize_href(&resolved);
                toc.entry(key).or_insert(a_text);
            }
        }
    }
    (landmarks, toc)
}

/// Build exclusion context from OPF (+ optional nav XHTML).
pub fn exclusion_context_from_opf(opf: &str, nav_xhtml: Option<&str>) -> EpubExclusionContext {
    let language = dc_language(opf);
    let nav_property_hrefs = nav_property_exclude_hrefs(opf);
    let guide_types = parse_guide_types(opf);
    let mut landmark_types = HashMap::new();
    let mut toc_titles = HashMap::new();
    if let Some(nav) = nav_xhtml {
        let nav_href = nav_document_href(opf).unwrap_or_else(|| "nav.xhtml".into());
        let (lm, toc) = parse_nav_landmarks_and_toc(nav, &nav_href);
        landmark_types = lm;
        toc_titles = toc;
    }
    EpubExclusionContext {
        language,
        landmark_types,
        guide_types,
        toc_titles,
        nav_property_hrefs,
    }
}

/// Guide + `properties=nav` + optional landmarks → exclude-href set (compat helper).
pub fn structural_exclude_hrefs(opf: &str, nav_xhtml: Option<&str>) -> HashSet<String> {
    let ctx = exclusion_context_from_opf(opf, nav_xhtml);
    let mut out = ctx.nav_property_hrefs;
    for (href, types) in &ctx.guide_types {
        for t in types {
            if rule_signal(ExclusionSource::Guide, t)
                .is_some_and(|s| s.verdict == ExclusionVerdict::Exclude)
            {
                out.insert(href.clone());
                break;
            }
        }
    }
    for (href, types) in &ctx.landmark_types {
        for t in types {
            if rule_signal(ExclusionSource::Landmark, t)
                .is_some_and(|s| s.verdict == ExclusionVerdict::Exclude)
            {
                out.insert(href.clone());
                break;
            }
        }
    }
    out
}

/// Compat: landmark structural excludes only.
pub fn landmark_exclude_hrefs(nav_xhtml: &str) -> HashSet<String> {
    let (lm, _) = parse_nav_landmarks_and_toc(nav_xhtml, "nav.xhtml");
    let mut out = HashSet::new();
    for (href, types) in lm {
        for t in types {
            if rule_signal(ExclusionSource::Landmark, &t)
                .is_some_and(|s| s.verdict == ExclusionVerdict::Exclude)
            {
                out.insert(href);
                break;
            }
        }
    }
    out
}

pub fn guide_exclude_hrefs(opf: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for (href, types) in parse_guide_types(opf) {
        for t in types {
            if rule_signal(ExclusionSource::Guide, &t)
                .is_some_and(|s| s.verdict == ExclusionVerdict::Exclude)
            {
                out.insert(href);
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        index: usize,
        spine_len: usize,
        id: &str,
        href: &str,
        linear: bool,
        media_type: &str,
        properties: &str,
    ) -> SpineItemMeta {
        SpineItemMeta {
            index,
            spine_len,
            id: id.into(),
            href: href.into(),
            linear,
            media_type: media_type.into(),
            properties: properties.into(),
        }
    }

    fn page(body: &str, body_type: Option<&str>) -> String {
        let bt = body_type
            .map(|t| format!(r#" epub:type="{t}""#))
            .unwrap_or_default();
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Page</title></head>
<body{bt}>{body}</body>
</html>"#
        )
    }

    fn prose(heading: &str) -> String {
        format!(
            "<h1>{heading}</h1><p>The rain had not stopped for three days, and nobody in the village remembered what the river looked like when it was calm.</p><p>She folded the letter twice and put it back in her coat.</p>"
        )
    }

    fn classify(
        index: usize,
        spine_len: usize,
        href: &str,
        xhtml: &str,
        ctx: &EpubExclusionContext,
    ) -> SpineClassification {
        let it = item(
            index,
            spine_len,
            "x",
            href,
            true,
            "application/xhtml+xml",
            "",
        );
        classify_spine_item(&it, Some(xhtml), ctx)
    }

    #[test]
    fn spine_zone_edges() {
        // length 40 → floor=6; indices 0..5 front, 34..39 back
        assert_eq!(spine_zone(0, 40), SpineZone::Front);
        assert_eq!(spine_zone(5, 40), SpineZone::Front);
        assert_eq!(spine_zone(20, 40), SpineZone::Body);
        assert_eq!(spine_zone(34, 40), SpineZone::Back);
        assert_eq!(spine_zone(39, 40), SpineZone::Back);
    }

    #[test]
    fn index_split_in_back_zone_is_include() {
        let ctx = EpubExclusionContext::default();
        let xhtml = page(&prose("Chapter Forty"), None);
        let c = classify(38, 40, "Text/index_split_130.xhtml", &xhtml, &ctx);
        assert_eq!(c.zone, SpineZone::Back);
        assert_eq!(c.verdict, ExclusionVerdict::Include);
        assert!(c.decided_by.is_none());
        assert!(chapter_aligned_from(&c, &ExclusionOptions::default()));
    }

    #[test]
    fn calibre_index_split_spine_never_filename_excludes() {
        // Synthetic 44-item Calibre-style spine: every href is index_split_* so
        // FILENAME_TEMPLATES must keep none of them filename-excluded.
        let ctx = EpubExclusionContext::default();
        let xhtml = page(&prose("Chapter"), None);
        let spine_len = 44;
        for i in 0..spine_len {
            let href = format!("Text/index_split_{i:03}.xhtml");
            let c = classify(i, spine_len, &href, &xhtml, &ctx);
            assert!(
                chapter_aligned_from(&c, &ExclusionOptions::default()),
                "index_split {i} zone={:?} verdict={:?}",
                c.zone,
                c.verdict
            );
            assert!(
                !c.signals
                    .iter()
                    .any(|s| s.source == ExclusionSource::Filename),
                "index_split {i} must not emit filename signals"
            );
            assert!(!excluded_by_rule(&c));
        }
    }

    #[test]
    fn content_types_nbsp_between_siblings_does_not_panic() {
        // RotK SYNC panic: open_end used abs+m.end() (double-counted m.start()),
        // so a following tag after leading text/NBSP sliced mid \u{a0}.
        let xml = page(
            "<span>Lead</span>\u{a0}<p epub:type=\"chapter\">The rain had not stopped for three days in the village.</p>",
            None,
        );
        let types = content_types(&xml);
        assert!(types.iter().any(|t| t == "chapter"));
    }

    #[test]
    fn excluded_by_rule_true_for_filename_exclude() {
        let ctx = EpubExclusionContext::default();
        let xhtml = page("<p>Bell, 12, 88.</p>", None);
        let c = classify(38, 40, "Text/index.xhtml", &xhtml, &ctx);
        assert!(excluded_by_rule(&c));
    }

    #[test]
    fn bare_index_in_back_excludes_filename() {
        let ctx = EpubExclusionContext::default();
        let xhtml = page("<p>Bell, 12, 88. Ferry, 40-44.</p>", None);
        let c = classify(38, 40, "Text/index.xhtml", &xhtml, &ctx);
        assert_eq!(c.zone, SpineZone::Back);
        assert_eq!(c.verdict, ExclusionVerdict::Exclude);
        assert_eq!(
            c.decided_by.as_ref().map(|s| s.source),
            Some(ExclusionSource::Filename)
        );
        assert_eq!(
            c.decided_by.as_ref().map(|s| s.value.as_str()),
            Some("index")
        );
        assert!(!chapter_aligned_from(&c, &ExclusionOptions::default()));
    }

    #[test]
    fn newsletter_mid_body_is_include() {
        let ctx = EpubExclusionContext::default();
        let xhtml = page(&prose("Newsletter"), None);
        let c = classify(20, 40, "Text/newsletter.xhtml", &xhtml, &ctx);
        assert_eq!(c.zone, SpineZone::Body);
        assert_eq!(c.verdict, ExclusionVerdict::Include);
        assert!(chapter_aligned_from(&c, &ExclusionOptions::default()));
    }

    #[test]
    fn chapter_beats_newsletter_filename_in_back() {
        let ctx = EpubExclusionContext::default();
        let body = format!(
            r#"<section epub:type="chapter">{}</section>"#,
            prose("The Newsletter")
        );
        let xhtml = page(&body, None);
        let c = classify(38, 40, "Text/the_newsletter.xhtml", &xhtml, &ctx);
        assert_eq!(c.zone, SpineZone::Back);
        assert_eq!(c.verdict, ExclusionVerdict::Include);
        assert_eq!(
            c.decided_by.as_ref().map(|s| s.source),
            Some(ExclusionSource::ContentType)
        );
        assert_eq!(
            c.decided_by.as_ref().map(|s| s.value.as_str()),
            Some("chapter")
        );
        // filename signal still recorded
        assert!(c
            .signals
            .iter()
            .any(|s| s.source == ExclusionSource::Filename && s.value == "newsletter"));
        assert!(chapter_aligned_from(&c, &ExclusionOptions::default()));
    }

    #[test]
    fn linear_no_excludes() {
        let ctx = EpubExclusionContext::default();
        let it = item(
            10,
            40,
            "popup",
            "Text/map-popup.xhtml",
            false,
            "application/xhtml+xml",
            "",
        );
        let body = format!(
            r#"<section epub:type="chapter">{}</section>"#,
            prose("The Map")
        );
        let xhtml = page(&body, None);
        let c = classify_spine_item(&it, Some(&xhtml), &ctx);
        assert_eq!(c.verdict, ExclusionVerdict::Exclude);
        assert_eq!(
            c.decided_by.as_ref().map(|s| s.source),
            Some(ExclusionSource::Linear)
        );
        assert!(!chapter_aligned_from(&c, &ExclusionOptions::default()));
    }

    #[test]
    fn dedication_filename_is_review_but_aligns() {
        let ctx = EpubExclusionContext::default();
        let xhtml = page("<p>For my mother, who never learned to swim.</p>", None);
        let c = classify(2, 40, "Text/dedication.xhtml", &xhtml, &ctx);
        assert_eq!(c.zone, SpineZone::Front);
        assert_eq!(c.verdict, ExclusionVerdict::Review);
        assert_eq!(
            c.decided_by.as_ref().map(|s| s.source),
            Some(ExclusionSource::Filename)
        );
        assert!(chapter_aligned_from(&c, &ExclusionOptions::default()));
    }

    #[test]
    fn cover_filename_excludes_front() {
        let ctx = EpubExclusionContext::default();
        let xhtml = page(
            r#"<div><img src="../Images/cover.jpg" alt=""/></div>"#,
            None,
        );
        let c = classify(0, 40, "Text/cover.xhtml", &xhtml, &ctx);
        assert_eq!(c.verdict, ExclusionVerdict::Exclude);
        assert!(!chapter_aligned_from(&c, &ExclusionOptions::default()));
    }

    #[test]
    fn nav_property_excludes() {
        let mut ctx = EpubExclusionContext::default();
        ctx.nav_property_hrefs.insert("nav/nav.xhtml".into());
        let it = item(
            1,
            40,
            "nav",
            "nav/nav.xhtml",
            true,
            "application/xhtml+xml",
            "nav",
        );
        let c = classify_spine_item(&it, Some(&page("", None)), &ctx);
        assert_eq!(c.verdict, ExclusionVerdict::Exclude);
        assert_eq!(
            c.decided_by.as_ref().map(|s| s.source),
            Some(ExclusionSource::Nav)
        );
    }

    #[test]
    fn guide_copyright_excludes() {
        let mut ctx = EpubExclusionContext::default();
        ctx.guide_types
            .insert("Text/legal.xhtml".into(), vec!["copyright-page".into()]);
        let xhtml = page("<p>Published by a publisher.</p>", None);
        let c = classify(3, 40, "Text/legal.xhtml", &xhtml, &ctx);
        assert_eq!(c.verdict, ExclusionVerdict::Exclude);
        assert_eq!(
            c.decided_by.as_ref().map(|s| s.source),
            Some(ExclusionSource::Guide)
        );
    }

    #[test]
    fn landmark_cover_excludes() {
        let mut ctx = EpubExclusionContext::default();
        ctx.landmark_types
            .insert("Text/cover.xhtml".into(), vec!["cover".into()]);
        let xhtml = page(r#"<div><img src="c.jpg" alt=""/></div>"#, None);
        let c = classify(0, 40, "Text/cover.xhtml", &xhtml, &ctx);
        assert_eq!(c.verdict, ExclusionVerdict::Exclude);
        assert_eq!(
            c.decided_by.as_ref().map(|s| s.source),
            Some(ExclusionSource::Landmark)
        );
    }

    #[test]
    fn about_author_heading_excludes_at_edge() {
        let ctx = EpubExclusionContext {
            language: "en".into(),
            ..Default::default()
        };
        let xhtml = page(
            "<h1>About the Author</h1><p>The author lives near a river.</p>",
            None,
        );
        let c = classify(38, 40, "Text/bio.xhtml", &xhtml, &ctx);
        assert_eq!(c.verdict, ExclusionVerdict::Exclude);
        assert_eq!(
            c.decided_by.as_ref().map(|s| s.source),
            Some(ExclusionSource::Title)
        );
    }

    #[test]
    fn copyright_filename_excludes() {
        let ctx = EpubExclusionContext::default();
        let xhtml = page("<p>Copyright 2026 by the author.</p>", None);
        let c = classify(4, 40, "Text/crt_copyright.xhtml", &xhtml, &ctx);
        assert_eq!(c.verdict, ExclusionVerdict::Exclude);
        assert_eq!(
            c.decided_by.as_ref().map(|s| s.value.as_str()),
            Some("copyright")
        );
    }

    #[test]
    fn manual_include_overrides_exclude() {
        let ctx = EpubExclusionContext::default();
        let xhtml = page(&prose("Cover"), None);
        let c = classify(0, 40, "Text/cover.xhtml", &xhtml, &ctx);
        assert_eq!(c.verdict, ExclusionVerdict::Exclude);
        let mut opts = ExclusionOptions::default();
        opts.decisions.chapters.insert(
            normalize_href("Text/cover.xhtml"),
            ChapterDecision::Included,
        );
        assert!(chapter_aligned_from(&c, &opts));
    }

    #[test]
    fn normalize_href_percent_decodes() {
        assert_eq!(
            normalize_href("Text/About%20the%20Author.xhtml#frag"),
            "Text/About the Author.xhtml"
        );
    }

    #[test]
    fn about_the_author_percent_encoded_filename() {
        let ctx = EpubExclusionContext::default();
        let xhtml = page("<p>The author lives near a river.</p>", None);
        let c = classify(38, 40, "Text/About%20the%20Author.xhtml", &xhtml, &ctx);
        assert_eq!(c.verdict, ExclusionVerdict::Exclude);
        assert_eq!(
            c.decided_by.as_ref().map(|s| s.source),
            Some(ExclusionSource::Filename)
        );
    }

    #[test]
    fn exclusion_context_from_opf_guide_and_nav() {
        let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:language>en</dc:language>
</metadata>
<manifest>
<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
<item id="c1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
</manifest>
<guide>
<reference type="cover" href="cover.xhtml"/>
<reference type="text" href="chapter1.xhtml"/>
</guide>
</package>"#;
        let nav = r#"<html><body>
<nav epub:type="landmarks">
<ol>
<li><a epub:type="cover" href="cover.xhtml">Cover</a></li>
<li><a epub:type="bodymatter" href="chapter1.xhtml">Start</a></li>
</ol>
</nav>
</body></html>"#;
        let ctx = exclusion_context_from_opf(opf, Some(nav));
        assert!(ctx.nav_property_hrefs.contains("nav.xhtml"));
        assert!(ctx
            .guide_types
            .get("cover.xhtml")
            .is_some_and(|t| t.iter().any(|x| x == "cover")));
        assert!(ctx
            .landmark_types
            .get("cover.xhtml")
            .is_some_and(|t| t.iter().any(|x| x == "cover")));
    }

    #[test]
    fn normalize_title_applies_nfkc() {
        // Fullwidth letters + ﬁ ligature fold under NFKC before titleRules match.
        assert_eq!(
            normalize_title("Ａbout the Author"),
            normalize_title("About the Author")
        );
        assert_eq!(
            normalize_title("ﬁnal notes"),
            normalize_title("final notes")
        );
        assert_eq!(
            title_signals("Ａbout the Author", SpineZone::Back, "en")
                .into_iter()
                .map(|s| s.value)
                .collect::<Vec<_>>(),
            vec!["about-the-author".to_string()]
        );
    }

    #[test]
    fn title_signals_locale_and_zone() {
        let m = |title: &str, zone: SpineZone, lang: &str| {
            title_signals(title, zone, lang)
                .into_iter()
                .map(|s| {
                    format!(
                        "{}:{}",
                        match s.verdict {
                            ExclusionVerdict::Exclude => "exclude",
                            ExclusionVerdict::Review => "review",
                            ExclusionVerdict::Include => "include",
                        },
                        s.value
                    )
                })
                .collect::<Vec<_>>()
        };
        assert!(m("Notes from Underground", SpineZone::Back, "en").is_empty());
        assert_eq!(
            m("Notes", SpineZone::Back, "en"),
            vec!["exclude:endnotes".to_string()]
        );
        assert!(m("Notes", SpineZone::Body, "en").is_empty());
        assert_eq!(
            m("About the Author", SpineZone::Back, "en"),
            vec!["exclude:about-the-author".to_string()]
        );
        assert_eq!(
            m("Over de auteur", SpineZone::Back, "nl"),
            vec!["exclude:about-the-author".to_string()]
        );
        assert!(m("Over de auteur", SpineZone::Back, "en").is_empty());
    }

    #[test]
    fn decide_prefers_linear_over_content_type() {
        let signals = vec![
            ExclusionSignal {
                source: ExclusionSource::Linear,
                value: "no".into(),
                verdict: ExclusionVerdict::Exclude,
                division: false,
            },
            ExclusionSignal {
                source: ExclusionSource::ContentType,
                value: "chapter".into(),
                verdict: ExclusionVerdict::Include,
                division: false,
            },
        ];
        let d = decide(&signals).unwrap();
        assert_eq!(d.source, ExclusionSource::Linear);
    }
}
