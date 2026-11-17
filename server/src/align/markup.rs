//! EPUB XHTML sentence markup (Storyteller `markup` stage).
//!
//! Lifts narratable text from the body (skipping script/style/svg/rt/rp),
//! segments into sentences, then wraps in place. Sentence boundaries follow
//! `@echogarden/text-segmentation` with CLDR suppressions per locale
//! (en/de/es/fr/it/pt/ru) + EN extras (Dr./A.I.). shortcut: no ICU WASM; CJK/EA
//! postprocessing deferred (ST disables it too). Block `\n` + footnotes match
//! ST `liftText` / `inlineFootnotes`. Wave D: Mark/addMark + xmltree body
//! tokenize + ST sentence ids. Locale from EPUB `dc:language` (pipeline).
//! model.ts surface: Root/Node/TextNode/Mark/FootnoteNode + descendants /
//! findIndex / replace / cut / textContent / ResolvedPos (Piece wrap stays
//! production path).

use std::collections::BTreeMap;

use regex::Regex;
use xmltree::{Element, XMLNode};

use crate::align::suppressions::{additional_suppressions, cldr_suppressions};

#[derive(Debug, Clone)]
enum Piece {
    Tag(String),
    Text(String),
}

/// ST `model.ts` Mark — span attrs applied to text ranges (not a PM crate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    pub tag_name: String,
    pub attrs: BTreeMap<String, String>,
}

impl Mark {
    pub fn new(tag_name: impl Into<String>, attrs: &[(&str, &str)]) -> Self {
        Self {
            tag_name: tag_name.into(),
            attrs: attrs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        }
    }

    pub fn eq_mark(&self, other: Option<&Mark>) -> bool {
        other.is_some_and(|o| self == o)
    }
}

/// ST `TextNode` — text carrying stacked marks (outer mark first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkedText {
    pub text: String,
    pub marks: Vec<Mark>,
}

impl MarkedText {
    pub fn new(text: impl Into<String>, marks: Vec<Mark>) -> Self {
        Self {
            text: text.into(),
            marks,
        }
    }

    pub fn is_leaf(&self) -> bool {
        true
    }

    pub fn is_inline(&self) -> bool {
        true
    }

    pub fn is_block(&self) -> bool {
        false
    }

    pub fn border(&self) -> usize {
        0
    }

    pub fn node_size(&self) -> usize {
        self.text.len()
    }

    pub fn content_size(&self) -> usize {
        self.node_size()
    }

    pub fn text_content(&self) -> &str {
        &self.text
    }

    fn split_at(&self, at: usize) -> Vec<MarkedText> {
        if at == 0 || at >= self.text.len() {
            return vec![self.clone()];
        }
        vec![
            MarkedText::new(self.text[..at].to_string(), self.marks.clone()),
            MarkedText::new(self.text[at..].to_string(), self.marks.clone()),
        ]
    }

    pub fn with_marks(&self, marks: Vec<Mark>) -> Self {
        MarkedText::new(self.text.clone(), marks)
    }

    /// ST `TextNode.cut` — slice from `pos` to end.
    pub fn cut(&self, pos: usize) -> Self {
        let at = pos.min(self.text.len());
        MarkedText::new(self.text[at..].to_string(), self.marks.clone())
    }
}

/// ST `Node` — element with attrs/marks/children (block border = 1 when non-leaf).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkNode {
    pub tag_name: String,
    pub attrs: BTreeMap<String, String>,
    pub children: Vec<MarkChild>,
    pub marks: Vec<Mark>,
    /// ST `FootnoteNode` marker (subclass without extra fields).
    pub is_footnote: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkChild {
    Text(MarkedText),
    Node(MarkNode),
}

impl MarkChild {
    pub fn node_size(&self) -> usize {
        match self {
            MarkChild::Text(t) => t.node_size(),
            MarkChild::Node(n) => n.node_size(),
        }
    }

    pub fn is_leaf(&self) -> bool {
        match self {
            MarkChild::Text(t) => t.is_leaf(),
            MarkChild::Node(n) => n.is_leaf(),
        }
    }

    pub fn is_block(&self) -> bool {
        match self {
            MarkChild::Text(t) => t.is_block(),
            MarkChild::Node(n) => n.is_block(),
        }
    }

    pub fn text_content(&self) -> String {
        match self {
            MarkChild::Text(t) => t.text.clone(),
            MarkChild::Node(n) => n.text_content(),
        }
    }

    pub fn cut(&self, pos: usize) -> MarkChild {
        match self {
            MarkChild::Text(t) => MarkChild::Text(t.cut(pos)),
            MarkChild::Node(n) => n.cut(pos).unwrap_or_else(|| self.clone()),
        }
    }
}

impl MarkNode {
    pub fn new(tag_name: impl Into<String>, children: Vec<MarkChild>) -> Self {
        Self {
            tag_name: tag_name.into(),
            attrs: BTreeMap::new(),
            children,
            marks: Vec::new(),
            is_footnote: false,
        }
    }

    pub fn with_attrs(mut self, attrs: &[(&str, &str)]) -> Self {
        self.attrs = attrs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        self
    }

    pub fn footnote(tag_name: impl Into<String>, children: Vec<MarkChild>) -> Self {
        let mut n = Self::new(tag_name, children);
        n.is_footnote = true;
        n
    }

    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    pub fn is_block(&self) -> bool {
        BLOCKS.iter().any(|b| *b == self.tag_name)
    }

    pub fn is_inline(&self) -> bool {
        !self.is_block()
    }

    pub fn border(&self) -> usize {
        if self.is_leaf() {
            0
        } else {
            1
        }
    }

    pub fn content_size(&self) -> usize {
        self.node_size().saturating_sub(self.border() * 2)
    }

    pub fn node_size(&self) -> usize {
        // ST: border + (contentSize || 1) + border
        let inner: usize = self.children.iter().map(MarkChild::node_size).sum();
        let content = if inner == 0 { 1 } else { inner };
        self.border() + content + self.border()
    }

    pub fn text_content(&self) -> String {
        self.children.iter().map(MarkChild::text_content).collect()
    }

    pub fn copy(&self, children: Option<Vec<MarkChild>>) -> Self {
        Self {
            tag_name: self.tag_name.clone(),
            attrs: self.attrs.clone(),
            children: children.unwrap_or_else(|| self.children.clone()),
            marks: self.marks.clone(),
            is_footnote: self.is_footnote,
        }
    }

    /// ST `Node.findIndex` — child index + start offset for a content position.
    pub fn find_index(&self, pos: usize) -> Result<(usize, usize), String> {
        find_index_in_children(&self.children, self.content_size(), pos)
    }

    /// ST `Node.replace` — replace the child at absolute `at` (within this node).
    pub fn replace(&self, at: usize, with_node: MarkChild) -> Result<Self, String> {
        Ok(self.copy(Some(replace_in_children(
            &self.children,
            self.border(),
            at,
            with_node,
        )?)))
    }

    /// ST `Node.cut` — child at `pos`, or cut of that child.
    pub fn cut(&self, pos: usize) -> Option<MarkChild> {
        cut_in_children(&self.children, self.border(), pos)
    }
}

/// ST `Root` — children only (border 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkRoot {
    pub children: Vec<MarkChild>,
}

impl MarkRoot {
    pub fn new(children: Vec<MarkChild>) -> Self {
        Self { children }
    }

    pub fn border(&self) -> usize {
        0
    }

    pub fn is_leaf(&self) -> bool {
        false
    }

    pub fn is_inline(&self) -> bool {
        false
    }

    pub fn is_block(&self) -> bool {
        true
    }

    pub fn node_size(&self) -> usize {
        let size: usize = self.children.iter().map(MarkChild::node_size).sum();
        size.max(1)
    }

    pub fn content_size(&self) -> usize {
        self.node_size()
    }

    pub fn text_content(&self) -> String {
        self.children.iter().map(MarkChild::text_content).collect()
    }

    pub fn copy(&self, children: Option<Vec<MarkChild>>) -> Self {
        Self {
            children: children.unwrap_or_else(|| self.children.clone()),
        }
    }

    pub fn find_index(&self, pos: usize) -> Result<(usize, usize), String> {
        find_index_in_children(&self.children, self.content_size(), pos)
    }

    pub fn replace(&self, at: usize, with_node: MarkChild) -> Result<Self, String> {
        Ok(self.copy(Some(replace_in_children(
            &self.children,
            self.border(),
            at,
            with_node,
        )?)))
    }

    pub fn cut(&self, pos: usize) -> Option<MarkChild> {
        cut_in_children(&self.children, self.border(), pos)
    }

    /// ST `Root.resolve` → `ResolvedPos.resolve`.
    pub fn resolve(&self, pos: usize) -> Result<ResolvedPos<'_>, String> {
        ResolvedPos::resolve(self, pos)
    }
}

/// ST path slot: node | index | start+offset (flat triples).
/// TextNode is never pushed (resolve breaks on text without recording it).
#[derive(Debug, Clone, Copy)]
enum PathSlot<'a> {
    Root(&'a MarkRoot),
    Node(&'a MarkNode),
    Num(usize),
}

/// ST `ResolvedPos.node` — Root | Node (text stops the walk, not stored).
#[derive(Debug, Clone, Copy)]
pub enum ResolveNode<'a> {
    Root(&'a MarkRoot),
    Node(&'a MarkNode),
}

/// ST `ResolvedPos.parent` (Root | Node).
#[derive(Debug, Clone, Copy)]
pub enum ResolveParent<'a> {
    Root(&'a MarkRoot),
    Node(&'a MarkNode),
}

impl ResolveParent<'_> {
    fn children(&self) -> &[MarkChild] {
        match self {
            ResolveParent::Root(r) => &r.children,
            ResolveParent::Node(n) => &n.children,
        }
    }
}

/// ST `ResolvedPos` — walks findIndex; path = [node, index, start+offset] triples.
#[derive(Debug, Clone)]
pub struct ResolvedPos<'a> {
    pub pos: usize,
    path: Vec<PathSlot<'a>>,
    pub parent_offset: usize,
    pub depth: usize,
}

impl<'a> ResolvedPos<'a> {
    /// ST `ResolvedPos.resolve`.
    pub fn resolve(doc: &'a MarkRoot, pos: usize) -> Result<Self, String> {
        let max = doc.node_size().saturating_sub(doc.border() * 2);
        if pos > max {
            return Err(format!("Position {pos} out of range"));
        }

        enum Cursor<'b> {
            Root(&'b MarkRoot),
            Node(&'b MarkNode),
        }

        let mut path: Vec<PathSlot<'a>> = Vec::new();
        let mut start = 0usize;
        let mut parent_offset = pos;
        let mut cursor = Cursor::Root(doc);

        loop {
            let (index, offset) = match cursor {
                Cursor::Root(r) => r.find_index(parent_offset)?,
                Cursor::Node(n) => n.find_index(parent_offset)?,
            };
            let rem = parent_offset - offset;
            match cursor {
                Cursor::Root(r) => path.push(PathSlot::Root(r)),
                Cursor::Node(n) => path.push(PathSlot::Node(n)),
            }
            path.push(PathSlot::Num(index));
            path.push(PathSlot::Num(start + offset));
            if rem == 0 {
                break;
            }
            let child = match cursor {
                Cursor::Root(r) => &r.children[index],
                Cursor::Node(n) => &n.children[index],
            };
            match child {
                MarkChild::Text(_) => break,
                MarkChild::Node(n) => {
                    parent_offset = rem - 1;
                    start += offset + 1;
                    cursor = Cursor::Node(n);
                }
            }
        }

        let depth = path.len() / 3 - 1;
        Ok(Self {
            pos,
            path,
            parent_offset,
            depth,
        })
    }

    fn resolve_depth(&self, val: Option<isize>) -> usize {
        match val {
            None => self.depth,
            Some(v) if v < 0 => (self.depth as isize + v) as usize,
            Some(v) => v as usize,
        }
    }

    pub fn parent(&self) -> ResolveParent<'a> {
        match self.node(None) {
            ResolveNode::Root(r) => ResolveParent::Root(r),
            ResolveNode::Node(n) => ResolveParent::Node(n),
        }
    }

    pub fn node(&self, depth: Option<isize>) -> ResolveNode<'a> {
        match self.path[self.resolve_depth(depth) * 3] {
            PathSlot::Root(r) => ResolveNode::Root(r),
            PathSlot::Node(n) => ResolveNode::Node(n),
            PathSlot::Num(_) => unreachable!("path triple head is a node"),
        }
    }

    pub fn index(&self, depth: Option<isize>) -> usize {
        match self.path[self.resolve_depth(depth) * 3 + 1] {
            PathSlot::Num(i) => i,
            _ => unreachable!("path triple mid is index"),
        }
    }

    /// ST `nodeAfter` — child at depth index, or cut when pos is inside it.
    pub fn node_after(&self) -> Option<MarkChild> {
        let parent = self.parent();
        let index = self.index(None);
        let children = parent.children();
        if index == children.len() {
            return None;
        }
        let path_end = match self.path.last() {
            Some(PathSlot::Num(n)) => *n,
            _ => unreachable!("path ends with start+offset"),
        };
        let d_off = self.pos - path_end;
        let child = &children[index];
        if d_off == 0 {
            Some(child.clone())
        } else {
            Some(child.cut(d_off))
        }
    }

    /// ST `before` — doc position before the node at `depth`.
    pub fn before(&self, depth: Option<isize>) -> Result<usize, String> {
        let depth = self.resolve_depth(depth);
        if depth == 0 {
            return Err("There is no position before the top-level node".into());
        }
        if depth == self.depth + 1 {
            Ok(self.pos)
        } else {
            match self.path[depth * 3 - 1] {
                PathSlot::Num(n) => Ok(n),
                _ => unreachable!("path[depth*3-1] is start+offset"),
            }
        }
    }
}

fn find_index_in_children(
    children: &[MarkChild],
    content_size: usize,
    pos: usize,
) -> Result<(usize, usize), String> {
    if pos == 0 {
        return Ok((0, pos));
    }
    if pos == content_size {
        return Ok((children.len(), pos));
    }
    if pos > content_size {
        return Err(format!("Position {pos} outside of fragment"));
    }
    let mut cur_pos = 0usize;
    for (i, cur) in children.iter().enumerate() {
        let end = cur_pos + cur.node_size();
        if end >= pos {
            if end == pos {
                return Ok((i + 1, end));
            }
            return Ok((i, cur_pos));
        }
        cur_pos = end;
    }
    Err(format!("Position {pos} outside of fragment"))
}

fn replace_in_children(
    children: &[MarkChild],
    border: usize,
    at: usize,
    with_node: MarkChild,
) -> Result<Vec<MarkChild>, String> {
    let mut out = Vec::new();
    let mut pos = border;
    for child in children {
        if at == pos {
            out.push(with_node.clone());
            pos += out.last().map(MarkChild::node_size).unwrap_or(0);
            continue;
        }
        if at > pos && at < pos + child.node_size() {
            match child {
                MarkChild::Text(_) => {
                    return Err("Tried to replace at a position within a text node".into());
                }
                MarkChild::Node(n) => {
                    out.push(MarkChild::Node(n.replace(at - pos, with_node.clone())?));
                }
            }
        } else {
            out.push(child.clone());
        }
        pos += out.last().map(MarkChild::node_size).unwrap_or(0);
    }
    Ok(out)
}

fn cut_in_children(children: &[MarkChild], border: usize, pos: usize) -> Option<MarkChild> {
    let mut current = border;
    for child in children {
        if pos == current {
            return Some(child.clone());
        }
        if pos > current && pos < current + child.node_size() {
            return Some(child.cut(pos - current));
        }
        current += child.node_size();
    }
    None
}

/// ST `descendants` — walk nodes; `cb` return true to descend into non-leaves.
pub fn descendants<F>(root: &MarkRoot, mut cb: F)
where
    F: FnMut(&MarkChild, usize, bool /*parent_is_root*/, usize /*index*/) -> bool,
{
    descendants_children(&root.children, 0, true, &mut cb);
}

fn descendants_children<F>(children: &[MarkChild], mut pos: usize, parent_is_root: bool, cb: &mut F)
where
    F: FnMut(&MarkChild, usize, bool, usize) -> bool,
{
    for (i, child) in children.iter().enumerate() {
        let descend = cb(child, pos, parent_is_root, i);
        if descend {
            if let MarkChild::Node(n) = child {
                if !n.is_leaf() {
                    descendants_children(&n.children, pos + n.border(), false, cb);
                }
            }
        }
        pos += child.node_size();
    }
}

/// ST `addMark` — split at from/to, prepend mark on covered text leaves.
pub fn add_mark(root: MarkRoot, from: usize, to: usize, mark: Mark) -> MarkRoot {
    let split = split_root(split_root(root, from), to);
    let mut pos = 0usize;
    let mut children = Vec::with_capacity(split.children.len());
    for child in split.children {
        let size = child.node_size();
        children.push(add_mark_to_child(child, pos, from, to, &mark));
        pos += size;
    }
    MarkRoot::new(children)
}

fn split_root(root: MarkRoot, at: usize) -> MarkRoot {
    let mut children = Vec::new();
    let mut pos = 0usize;
    for child in root.children {
        let size = child.node_size();
        if at > pos && at < pos + size {
            match child {
                MarkChild::Text(t) => {
                    children.extend(t.split_at(at - pos).into_iter().map(MarkChild::Text))
                }
                MarkChild::Node(n) => children.push(MarkChild::Node(split_node(n, at - pos))),
            }
        } else {
            children.push(child);
        }
        pos += size;
    }
    MarkRoot::new(children)
}

fn split_node(node: MarkNode, at: usize) -> MarkNode {
    let border = node.border();
    if at == border || at == node.node_size() - border {
        return node;
    }
    let mut children = Vec::new();
    let mut pos = border;
    let old_children = node.children.clone();
    for child in old_children {
        let size = child.node_size();
        if at > pos && at < pos + size {
            match child {
                MarkChild::Text(t) => {
                    children.extend(t.split_at(at - pos).into_iter().map(MarkChild::Text))
                }
                MarkChild::Node(n) => children.push(MarkChild::Node(split_node(n, at - pos))),
            }
        } else {
            children.push(child);
        }
        pos += size;
    }
    node.copy(Some(children))
}

fn add_mark_to_child(
    child: MarkChild,
    pos: usize,
    from: usize,
    to: usize,
    mark: &Mark,
) -> MarkChild {
    if from >= pos + child.node_size() || to <= pos {
        return child;
    }
    match child {
        MarkChild::Text(t) => {
            let mut marks = vec![mark.clone()];
            marks.extend(t.marks.iter().cloned());
            MarkChild::Text(t.with_marks(marks))
        }
        MarkChild::Node(n) => {
            let border = n.border();
            let mut child_pos = border;
            let mut children = Vec::new();
            let old_children = n.children.clone();
            for c in old_children {
                let size = c.node_size();
                children.push(add_mark_to_child(c, pos + child_pos, from, to, mark));
                child_pos += size;
            }
            MarkChild::Node(n.copy(Some(children)))
        }
    }
}

/// ST `semantics.ts` BLOCKS = TEXT_CONTENT + CONTENT_SECTIONING + TABLE_PARTS.
const BLOCKS: &[&str] = &[
    "blockquote",
    "dd",
    "div",
    "dl",
    "dt",
    "figcaption",
    "figure",
    "hr",
    "li",
    "menu",
    "ol",
    "p",
    "pre",
    "ul",
    "address",
    "article",
    "aside",
    "footer",
    "header",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hgroup",
    "main",
    "nav",
    "section",
    "search",
    "table",
    "thead",
    "th",
    "tbody",
    "tr",
    "td",
    "colgroup",
    "caption",
    "tfoot",
];

const FOOTNOTE_ROLES: &[&str] = &["footnote", "rearnote", "endnote", "note"];

/// Lowercase EN suppressions from Echogarden CLDR + `additionalSuppressions`
/// (each includes the trailing `.`).
const EN_ABBREVS: &[&str] = &[
    "a.d.", "a.i.", "a.m.", "a.s.", "aa.", "ab.", "abs.", "adj.", "adv.", "alt.", "approx.",
    "aug.", "b.v.", "c.f.", "c.o.d.", "capt.", "col.", "comm.", "conn.", "cont.", "d.a.", "d.c.",
    "dc.", "dec.", "def.", "dept.", "diff.", "dr.", "e.g.", "ed.", "est.", "etc.", "ex.", "exec.",
    "feb.", "fn.", "fri.", "hon.b.a.", "i.", "i.d.", "i.e.", "i.t.", "j.b.", "j.d.", "j.k.",
    "jan.", "jun.", "k.r.", "l.a.", "l.p.", "lev.", "lib.", "lt.", "lt.cdr.", "m.", "m.i.t.",
    "m.r.", "m.t.", "maj.", "mar.", "mart.", "mb.", "md.", "mgr.", "min.", "misc.", "mr.", "mrs.",
    "ms.", "mt.", "n.v.", "n.y.", "nov.", "nr.", "num.", "op.", "org.", "p.m.", "p.o.", "p.v.",
    "ph.d.", "phys.", "pp.", "prof.", "pvt.", "r.l.", "r.t.", "rep.", "rev.", "s.a.", "s.a.r.",
    "s.e.", "s.p.a.", "sep.", "sept.", "sgt.", "sq.", "u.s.", "u.s.a.", "u.s.c.", "vs.", "yr.",
];

const MIN_SENTENCE_LETTERS: usize = 2;

/// Sentinel piece index for synthetic block-boundary newlines (not in the offset map).
const SYNTH_PI: usize = usize::MAX;

fn is_sentence_ender(c: char) -> bool {
    matches!(c, '.' | '?' | '!' | '。' | '？' | '！' | '\n')
}

fn is_trailing_after_ender(c: char) -> bool {
    matches!(
        c,
        '"' | '\''
            | ')'
            | ']'
            | '}'
            | '»'
            | '」'
            | '\u{201C}'
            | '\u{201D}'
            | '\u{2018}'
            | '\u{2019}'
            | '.'
            | '?'
            | '!'
            | '。'
            | '？'
            | '！'
            | '\n'
            | ','
            | '、'
            | '，'
            | '،'
            | ';'
            | '；'
            | ':'
            | '：'
            | '—'
    ) || c.is_whitespace()
}

fn primary_lang(locale: &str) -> String {
    locale
        .split(['-', '_'])
        .next()
        .unwrap_or("en")
        .to_ascii_lowercase()
}

fn ends_with_locale_abbrev(through_dot: &str, locale: &str) -> bool {
    let lang = primary_lang(locale);
    let lower = through_dot.to_lowercase();
    let mut abbrevs: Vec<&str> = Vec::new();
    if lang == "en" {
        abbrevs.extend_from_slice(EN_ABBREVS);
    }
    for a in cldr_suppressions(&lang) {
        abbrevs.push(a);
    }
    for a in additional_suppressions(&lang) {
        abbrevs.push(a);
    }
    for abbr in abbrevs {
        let abbr_l = abbr.to_lowercase();
        if let Some(prefix) = lower.strip_suffix(abbr_l.as_str()) {
            if prefix.is_empty() {
                return true;
            }
            if let Some(c) = prefix.chars().last() {
                if !c.is_alphanumeric() {
                    return true;
                }
            }
        }
    }
    false
}

/// Byte span of a sentence inside the lifted plain text (ST segmentation pos).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentenceSpan {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

fn trimmed_span(slice: &str, base: usize) -> Option<SentenceSpan> {
    let trimmed = slice.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lead = slice.len() - slice.trim_start().len();
    let start = base + lead;
    let end = start + trimmed.len();
    Some(SentenceSpan {
        start,
        end,
        text: trimmed.to_string(),
    })
}

/// Split plain text into sentence spans (Echogarden-shaped rules, default EN).
/// Spans tile the input in order — ST `markupChapter` walks `pos` this way.
pub fn split_sentence_spans(text: &str) -> Vec<SentenceSpan> {
    split_sentence_spans_locale(text, "en")
}

/// Locale-aware sentence spans (CLDR suppressions for `language`).
pub fn split_sentence_spans_locale(text: &str, locale: &str) -> Vec<SentenceSpan> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut idx = 0usize;
    while idx < chars.len() {
        let (byte_i, c) = chars[idx];
        if !is_sentence_ender(c) {
            idx += 1;
            continue;
        }
        // '.' / '。' glued to alnum → decimal or dotted word, not a boundary.
        if matches!(c, '.' | '。') {
            if let Some(&(_, next)) = chars.get(idx + 1) {
                if next.is_alphanumeric() {
                    idx += 1;
                    continue;
                }
            }
        }
        let letter_count = text[start..byte_i]
            .chars()
            .filter(|ch| ch.is_alphabetic())
            .count();
        if letter_count < MIN_SENTENCE_LETTERS {
            idx += 1;
            continue;
        }
        if c == '.' {
            let dot_end = byte_i + c.len_utf8();
            if ends_with_locale_abbrev(&text[start..dot_end], locale) {
                idx += 1;
                continue;
            }
        }
        let mut j = idx + 1;
        while j < chars.len() && is_trailing_after_ender(chars[j].1) {
            j += 1;
        }
        let sent_end = if j < chars.len() {
            chars[j].0
        } else {
            text.len()
        };
        if let Some(span) = trimmed_span(&text[start..sent_end], start) {
            out.push(span);
        }
        start = sent_end;
        idx = j;
    }
    if start < text.len() {
        if let Some(span) = trimmed_span(&text[start..], start) {
            out.push(span);
        }
    }
    if out.is_empty() {
        if let Some(span) = trimmed_span(text, 0) {
            out.push(span);
        }
    }
    out
}

/// Split plain text into sentences (default EN).
pub fn split_sentences(text: &str) -> Vec<String> {
    split_sentences_locale(text, "en")
}

pub fn split_sentences_locale(text: &str, locale: &str) -> Vec<String> {
    split_sentence_spans_locale(text, locale)
        .into_iter()
        .map(|s| s.text)
        .collect()
}

/// Wrap each sentence with ST `id="{chapterId}-s{N}"` (or Foliate `sentence-N`
/// when `chapter_id` is None) + `epub:type=storyteller:sentence-span` **in place**.
/// Returns marked XHTML and the sentence strings (align stage input).
pub fn markup_xhtml_body(
    xhtml: &str,
    chapter_id: Option<&str>,
) -> Result<(String, Vec<String>), String> {
    markup_xhtml_body_locale(xhtml, chapter_id, "en")
}

/// Like [`markup_xhtml_body`] with Echogarden CLDR locale (EPUB `dc:language`).
pub fn markup_xhtml_body_locale(
    xhtml: &str,
    chapter_id: Option<&str>,
    locale: &str,
) -> Result<(String, Vec<String>), String> {
    let body_re = Regex::new(r"(?is)<body([^>]*)>(.*)</body>").map_err(|e| e.to_string())?;
    let caps = body_re
        .captures(xhtml)
        .ok_or_else(|| "XHTML has no body".to_string())?;
    let attrs = caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
    let inner = caps.get(2).map(|m| m.as_str()).unwrap_or("");

    let pieces = tokenize_body(inner);
    let (pieces, restores) = inline_footnotes(pieces);
    let (plain, map) = lift_text(&pieces);
    let spans = split_sentence_spans_locale(&plain, locale);
    if spans.is_empty() {
        return Err("No sentences found to mark up".into());
    }
    let sentences: Vec<String> = spans.iter().map(|s| s.text.clone()).collect();

    // ST markupChapter: sequential pos spans, not plain.find.
    let piece_out = wrap_sentences(&pieces, &plain, &map, &spans, chapter_id);
    let marked_inner = restore_footnotes(piece_out, &restores);
    let out = body_re
        .replace(xhtml, format!("<body{attrs}>{marked_inner}</body>"))
        .into_owned();
    Ok((ensure_storyteller_epub_prefix(&out), sentences))
}

const EPUB_NS: &str = "http://www.idpf.org/2007/ops";
const STORYTELLER_VOCAB: &str =
    "storyteller: https://storyteller-platform.gitlab.io/storyteller/docs/vocabulary";

/// ST `addStorytellerPrefix` — xmlns:epub + epub:prefix on `<html>`.
fn ensure_storyteller_epub_prefix(xhtml: &str) -> String {
    let html_re = Regex::new(r"(?is)<html([^>]*)>").expect("regex");
    let Some(caps) = html_re.captures(xhtml) else {
        return xhtml.to_string();
    };
    let attrs = caps.get(1).map(|m| m.as_str()).unwrap_or("");
    let mut new_attrs = attrs.to_string();
    let lower = new_attrs.to_ascii_lowercase();
    if !lower.contains("xmlns:epub") {
        new_attrs.push_str(&format!(r#" xmlns:epub="{EPUB_NS}""#));
    }
    let lower = new_attrs.to_ascii_lowercase();
    if !lower.contains("epub:prefix") {
        new_attrs.push_str(&format!(r#" epub:prefix="{STORYTELLER_VOCAB}""#));
    } else if !lower.contains("storyteller:") {
        // Append into existing double-quoted epub:prefix value.
        if let Some(i) = lower.find("epub:prefix=") {
            let after_eq = i + "epub:prefix=".len();
            let bytes = new_attrs.as_bytes();
            if bytes.get(after_eq) == Some(&b'"') {
                let start = after_eq + 1;
                if let Some(rel) = new_attrs[start..].find('"') {
                    let end = start + rel;
                    let existing = new_attrs[start..end].to_string();
                    let merged = format!("{existing} {STORYTELLER_VOCAB}");
                    new_attrs.replace_range(start..end, &merged);
                }
            }
        }
    }
    html_re
        .replace(xhtml, format!("<html{new_attrs}>"))
        .into_owned()
}

fn is_skip_open(lower: &str) -> bool {
    lower.starts_with("<script")
        || lower.starts_with("<style")
        || lower.starts_with("<svg")
        || lower.starts_with("<rt")
        || lower.starts_with("<rp")
}

fn tokenize_body(inner: &str) -> Vec<Piece> {
    tokenize_body_xml(inner).unwrap_or_else(|| tokenize_body_regex(inner))
}

fn tokenize_body_regex(inner: &str) -> Vec<Piece> {
    let re = Regex::new(r"(?is)<[^>]+>|[^<]+").expect("regex");
    let mut pieces = Vec::new();
    let mut skip_depth = 0i32;
    for m in re.find_iter(inner) {
        let s = m.as_str();
        if s.starts_with('<') {
            let lower = s.to_ascii_lowercase();
            let self_close = is_self_closing(&lower);
            if lower.starts_with("</") {
                if skip_depth > 0 {
                    skip_depth = (skip_depth - 1).max(0);
                }
            } else if is_skip_open(&lower) {
                if !self_close {
                    skip_depth += 1;
                }
            } else if skip_depth > 0 && !self_close {
                skip_depth += 1;
            }
            pieces.push(Piece::Tag(s.to_string()));
        } else if skip_depth > 0 {
            pieces.push(Piece::Tag(s.to_string())); // keep but not narratable
        } else {
            pieces.push(Piece::Text(s.to_string()));
        }
    }
    pieces
}

/// xmltree body walk → Piece list (nested inline/block fidelity). Falls back on
/// parse failure (HTML-ish fragments).
fn tokenize_body_xml(inner: &str) -> Option<Vec<Piece>> {
    // No xmlns:epub on the wrapper — declaring it makes xmltree fold
    // `epub:type` into a namespaced local name and we lose the prefix on emit.
    let wrapped = format!("<_shelf>{inner}</_shelf>");
    let root = Element::parse(wrapped.as_bytes()).ok()?;
    let mut pieces = Vec::new();
    flatten_xml_nodes(&root.children, &mut pieces, false);
    Some(pieces)
}

fn is_skip_tag_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "script" | "style" | "svg" | "rt" | "rp"
    )
}

fn attr_key_for_emit(key: &str) -> String {
    if let Some(local) = key.strip_prefix("{http://www.idpf.org/2007/ops}") {
        return format!("epub:{local}");
    }
    if let Some((_, local)) = key.rsplit_once('}') {
        if key.starts_with('{') {
            return local.to_string();
        }
    }
    key.to_string()
}

fn emit_open_tag(el: &Element) -> String {
    let mut s = format!("<{}", el.name);
    // Prefer stable attr order for tests.
    let mut keys: Vec<_> = el.attributes.keys().cloned().collect();
    keys.sort();
    for k in keys {
        let v = &el.attributes[&k];
        let ak = attr_key_for_emit(&k);
        s.push_str(&format!(r#" {ak}="{}""#, xml_escape_attr(v)));
    }
    if el.children.is_empty()
        && matches!(
            el.name.to_ascii_lowercase().as_str(),
            "br" | "img" | "hr" | "wbr" | "meta" | "link" | "input"
        )
    {
        s.push_str("/>");
    } else {
        s.push('>');
    }
    s
}

fn xml_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn flatten_xml_nodes(nodes: &[XMLNode], out: &mut Vec<Piece>, skip: bool) {
    for node in nodes {
        match node {
            XMLNode::Element(el) => {
                let name = el.name.as_str();
                let skip_here = skip || is_skip_tag_name(name);
                let open = emit_open_tag(el);
                let self_c = open.ends_with("/>");
                out.push(Piece::Tag(open));
                if self_c {
                    continue;
                }
                flatten_xml_nodes(&el.children, out, skip_here);
                out.push(Piece::Tag(format!("</{name}>")));
            }
            XMLNode::Text(t) => {
                if skip {
                    out.push(Piece::Tag(t.clone()));
                } else {
                    out.push(Piece::Text(t.clone()));
                }
            }
            XMLNode::Comment(_) | XMLNode::CData(_) | XMLNode::ProcessingInstruction(_, _) => {}
        }
    }
}

fn is_self_closing(lower: &str) -> bool {
    lower.contains("/>") || {
        let name = tag_name(lower).unwrap_or("");
        matches!(
            name,
            "br" | "img" | "hr" | "wbr" | "meta" | "link" | "input"
        )
    }
}

fn tag_name(tag: &str) -> Option<&str> {
    let s = tag.trim_start_matches('<').trim_start_matches('/');
    let end = s
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(s.len());
    let name = &s[..end];
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn is_close_tag(tag: &str) -> bool {
    tag.as_bytes().get(1) == Some(&b'/')
}

fn is_block_name(name: &str) -> bool {
    BLOCKS.iter().any(|b| b.eq_ignore_ascii_case(name))
}

fn attr_value(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let name_l = name.to_ascii_lowercase();
    for quote in ['"', '\''] {
        let pattern = format!("{name_l}={quote}");
        if let Some(i) = lower.find(&pattern) {
            let start = i + pattern.len();
            let rest = &tag[start..];
            let end = rest.find(quote).unwrap_or(rest.len());
            return Some(rest[..end].to_string());
        }
    }
    None
}

fn epub_type_tokens(tag: &str) -> Vec<String> {
    attr_value(tag, "epub:type")
        .or_else(|| attr_value(tag, "type"))
        .map(|v| {
            v.split_whitespace()
                .map(|t| t.to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

fn has_epub_type(tag: &str, role: &str) -> bool {
    epub_type_tokens(tag).iter().any(|t| t == role)
}

fn is_footnote_open(tag: &str, noteref_ids: &std::collections::HashSet<String>) -> bool {
    if is_close_tag(tag) || !tag.starts_with('<') {
        return false;
    }
    let tokens = epub_type_tokens(tag);
    if tokens.iter().any(|t| FOOTNOTE_ROLES.contains(&t.as_str())) {
        return true;
    }
    if let Some(id) = attr_value(tag, "id") {
        if noteref_ids.contains(&id) {
            return true;
        }
    }
    false
}

fn is_noteref_open(tag: &str) -> bool {
    tag_name(tag).is_some_and(|n| n.eq_ignore_ascii_case("a")) && has_epub_type(tag, "noteref")
}

fn href_fragment(tag: &str) -> Option<String> {
    let href = attr_value(tag, "href")?;
    href.split('#')
        .nth(1)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Inclusive range of an element starting at `open_idx`.
fn element_end(pieces: &[Piece], open_idx: usize) -> Option<usize> {
    let Piece::Tag(open) = &pieces[open_idx] else {
        return None;
    };
    if is_self_closing(&open.to_ascii_lowercase()) {
        return Some(open_idx);
    }
    let name = tag_name(open)?;
    let mut depth = 0i32;
    for (i, piece) in pieces.iter().enumerate().skip(open_idx) {
        let Piece::Tag(t) = piece else {
            continue;
        };
        let Some(n) = tag_name(t) else {
            continue;
        };
        if !n.eq_ignore_ascii_case(name) {
            continue;
        }
        if is_close_tag(t) {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        } else if !is_self_closing(&t.to_ascii_lowercase()) {
            depth += 1;
        }
    }
    None
}

#[derive(Debug)]
struct FootnoteRestore {
    /// Inclusive range in the *inlined* piece list where footnote body sits.
    inline_range: (usize, usize),
    /// Inclusive range of the empty shell at the original footnote site.
    shell_range: (usize, usize),
    noteref_html: Vec<String>,
}

fn collect_noteref_ids(pieces: &[Piece]) -> std::collections::HashSet<String> {
    let mut ids = std::collections::HashSet::new();
    for piece in pieces {
        if let Piece::Tag(t) = piece {
            if is_noteref_open(t) {
                if let Some(frag) = href_fragment(t) {
                    ids.insert(frag);
                }
            }
        }
    }
    ids
}

/// ST `inlineFootnotes`: splice footnote bodies at noteref sites; leave empty shells.
fn inline_footnotes(pieces: Vec<Piece>) -> (Vec<Piece>, Vec<FootnoteRestore>) {
    let noteref_ids = collect_noteref_ids(&pieces);

    let mut noteref_by_id: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    let mut footnotes: Vec<(String, usize, usize, String)> = Vec::new();

    let mut i = 0usize;
    while i < pieces.len() {
        let Piece::Tag(t) = &pieces[i] else {
            i += 1;
            continue;
        };
        if is_noteref_open(t) {
            if let (Some(frag), Some(end)) = (href_fragment(t), element_end(&pieces, i)) {
                noteref_by_id.entry(frag).or_insert((i, end));
                i = end + 1;
                continue;
            }
        }
        if is_footnote_open(t, &noteref_ids) {
            if let Some(end) = element_end(&pieces, i) {
                let id = attr_value(t, "id").unwrap_or_default();
                let tag = tag_name(t).unwrap_or("aside").to_string();
                footnotes.push((id, i, end, tag));
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }

    let mut plans: Vec<(usize, usize, usize, usize, String)> = Vec::new();
    for (id, fn_start, fn_end, tag) in &footnotes {
        if id.is_empty() {
            continue;
        }
        if let Some(&(nr_start, nr_end)) = noteref_by_id.get(id) {
            plans.push((nr_start, nr_end, *fn_start, *fn_end, tag.clone()));
        }
    }
    if plans.is_empty() {
        return (pieces, Vec::new());
    }

    let mut consumed = vec![false; pieces.len()];
    for &(nr_s, nr_e, fn_s, fn_e, _) in &plans {
        for slot in &mut consumed[nr_s..=nr_e] {
            *slot = true;
        }
        for slot in &mut consumed[fn_s..=fn_e] {
            *slot = true;
        }
    }

    let mut out: Vec<Piece> = Vec::new();
    let mut restores: Vec<FootnoteRestore> = Vec::new();
    // Track pending restore slots filled when we emit both sites.
    struct Partial {
        inline_range: Option<(usize, usize)>,
        shell_range: Option<(usize, usize)>,
        noteref_html: Vec<String>,
    }
    let mut partials: Vec<Partial> = plans
        .iter()
        .map(|&(nr_s, nr_e, _, _, _)| Partial {
            inline_range: None,
            shell_range: None,
            noteref_html: pieces[nr_s..=nr_e]
                .iter()
                .map(|p| match p {
                    Piece::Tag(t) | Piece::Text(t) => t.clone(),
                })
                .collect(),
        })
        .collect();

    let plan_at_noteref = |idx: usize| plans.iter().position(|&(nr_s, _, _, _, _)| nr_s == idx);
    let plan_at_footnote = |idx: usize| plans.iter().position(|&(_, _, fn_s, _, _)| fn_s == idx);

    i = 0;
    while i < pieces.len() {
        if let Some(pi) = plan_at_noteref(i) {
            let (_, _, fn_s, fn_e, _) = plans[pi];
            let start = out.len();
            out.extend(pieces[fn_s..=fn_e].iter().cloned());
            let end = out.len() - 1;
            partials[pi].inline_range = Some((start, end));
            i = plans[pi].1 + 1;
            continue;
        }
        if let Some(pi) = plan_at_footnote(i) {
            let tag = &plans[pi].4;
            let start = out.len();
            out.push(Piece::Tag(format!("<{tag}>")));
            out.push(Piece::Tag(format!("</{tag}>")));
            partials[pi].shell_range = Some((start, start + 1));
            i = plans[pi].3 + 1;
            continue;
        }
        if !consumed[i] {
            out.push(pieces[i].clone());
        }
        i += 1;
    }

    for p in partials {
        if let (Some(inline_range), Some(shell_range)) = (p.inline_range, p.shell_range) {
            restores.push(FootnoteRestore {
                inline_range,
                shell_range,
                noteref_html: p.noteref_html,
            });
        }
    }

    (out, restores)
}

fn restore_footnotes(mut piece_out: Vec<String>, restores: &[FootnoteRestore]) -> String {
    // Apply from the end so earlier indices stay valid.
    let mut order: Vec<usize> = (0..restores.len()).collect();
    order.sort_by_key(|&i| {
        std::cmp::Reverse(restores[i].inline_range.0.max(restores[i].shell_range.0))
    });

    for &ri in &order {
        let r = &restores[ri];
        let (is, ie) = r.inline_range;
        let (ss, se) = r.shell_range;
        if ie >= piece_out.len() || se >= piece_out.len() {
            continue;
        }
        let marked_fn: Vec<String> = piece_out[is..=ie].to_vec();
        // Replace higher index range first.
        if is > ss {
            piece_out.splice(is..=ie, r.noteref_html.iter().cloned());
            let shell_end = se.min(piece_out.len().saturating_sub(1));
            if ss <= shell_end && shell_end < piece_out.len() {
                // Shell may have shifted if somehow overlapping — re-find by using
                // precomputed ranges only when inline was after shell (typical).
                piece_out.splice(ss..=se, marked_fn);
            }
        } else {
            piece_out.splice(ss..=se, marked_fn.iter().cloned());
            // inline range shifts by marked_fn.len() - shell_len
            let shell_len = se - ss + 1;
            let delta = marked_fn.len() as isize - shell_len as isize;
            let nis = (is as isize + delta) as usize;
            let nie = (ie as isize + delta) as usize;
            if nie < piece_out.len() {
                piece_out.splice(nis..=nie, r.noteref_html.iter().cloned());
            }
        }
    }
    piece_out.concat()
}

fn normalize_text_newlines(t: &str) -> String {
    t.chars().map(|c| if c == '\n' { ' ' } else { c }).collect()
}

fn push_synth_newline(plain: &mut String, map: &mut Vec<(usize, usize)>) {
    if plain.ends_with('\n') {
        return;
    }
    plain.push('\n');
    map.push((SYNTH_PI, 0));
}

fn parent_has_block_child(pieces: &[Piece], parent_open: usize, parent_name: &str) -> bool {
    let mut depth = 0i32;
    for piece in pieces.iter().skip(parent_open + 1) {
        let Piece::Tag(t) = piece else {
            continue;
        };
        let Some(n) = tag_name(t) else {
            continue;
        };
        if is_close_tag(t) {
            if depth == 0 && n.eq_ignore_ascii_case(parent_name) {
                return false;
            }
            depth -= 1;
            continue;
        }
        if depth == 0 && is_block_name(n) {
            return true;
        }
        if !is_self_closing(&t.to_ascii_lowercase()) {
            depth += 1;
        }
    }
    false
}

fn body_has_block_child(pieces: &[Piece]) -> bool {
    let mut depth = 0i32;
    for piece in pieces {
        let Piece::Tag(t) = piece else {
            continue;
        };
        let Some(n) = tag_name(t) else {
            continue;
        };
        if is_close_tag(t) {
            depth -= 1;
            continue;
        }
        if depth == 0 && is_block_name(n) {
            return true;
        }
        if !is_self_closing(&t.to_ascii_lowercase()) {
            depth += 1;
        }
    }
    false
}

fn is_last_child_before_parent_close(pieces: &[Piece], idx: usize, parent_name: &str) -> bool {
    let mut depth = 0i32;
    for piece in pieces.iter().skip(idx + 1) {
        match piece {
            Piece::Text(t) => {
                if depth == 0 && t.chars().any(|c| !c.is_whitespace()) {
                    return false;
                }
            }
            Piece::Tag(t) => {
                let Some(n) = tag_name(t) else {
                    continue;
                };
                if is_close_tag(t) {
                    if depth == 0 && n.eq_ignore_ascii_case(parent_name) {
                        return true;
                    }
                    depth -= 1;
                    continue;
                }
                // Any remaining open/self-close sibling means we are not last.
                if depth == 0 {
                    return false;
                }
                if !is_self_closing(&t.to_ascii_lowercase()) {
                    depth += 1;
                }
            }
        }
    }
    false
}

/// Plain narratable text + map from plain byte offset → (piece_idx, offset_in_text).
/// Synthetic block `\n` uses `SYNTH_PI` and is not tied to a piece (ST liftText).
fn lift_text(pieces: &[Piece]) -> (String, Vec<(usize, usize)>) {
    let mut plain = String::new();
    let mut map = Vec::new();
    let body_blocks = body_has_block_child(pieces);

    // Stack of open elements: (tag_name, open_idx, is_block, is_footnote)
    let mut stack: Vec<(String, usize, bool, bool)> = Vec::new();
    let noteref_ids = collect_noteref_ids(pieces);

    for (pi, piece) in pieces.iter().enumerate() {
        match piece {
            Piece::Tag(t) => {
                let lower = t.to_ascii_lowercase();
                let Some(name) = tag_name(&lower).map(str::to_string) else {
                    continue;
                };
                if is_close_tag(t) {
                    while let Some((n, _, _, _)) = stack.last() {
                        if n.eq_ignore_ascii_case(&name) {
                            stack.pop();
                            break;
                        }
                        // mismatched close — pop until match or empty
                        stack.pop();
                    }
                    continue;
                }
                let is_block = is_block_name(&name);
                let is_fn = is_footnote_open(t, &noteref_ids);
                if is_fn && !plain.is_empty() {
                    // ST FootnoteNode: leading \n before footnote content.
                    push_synth_newline(&mut plain, &mut map);
                }
                let self_c = is_self_closing(&lower);
                // Trailing leaf atoms (br/img) force \n — ST trailing-atoms test.
                if self_c && matches!(name.as_str(), "br" | "img") {
                    if let Some((pname, _, true, _)) = stack.last() {
                        if is_last_child_before_parent_close(pieces, pi, pname) {
                            push_synth_newline(&mut plain, &mut map);
                        }
                    }
                }
                if !self_c {
                    stack.push((name, pi, is_block, is_fn));
                }
            }
            Piece::Text(t) => {
                let normalized = normalize_text_newlines(t);
                let has_block_siblings = if let Some((pname, pidx, _, _)) = stack.last() {
                    parent_has_block_child(pieces, *pidx, pname)
                } else {
                    body_blocks
                };
                if has_block_siblings && !normalized.chars().any(|c| !c.is_whitespace()) {
                    continue;
                }

                let mut start = 0usize;
                if plain.is_empty() || plain.ends_with('\n') {
                    let content_start = normalized
                        .char_indices()
                        .find(|(_, c)| !c.is_whitespace())
                        .map(|(i, _)| i)
                        .unwrap_or(normalized.len());
                    start = content_start;
                }
                let slice = &normalized[start..];
                for off in start..normalized.len() {
                    map.push((pi, off));
                }
                plain.push_str(slice);

                // Last text child of a block → forced sentence break (not in map).
                if let Some((pname, _, true, _)) = stack.last() {
                    if is_last_child_before_parent_close(pieces, pi, pname)
                        && !(plain.ends_with('\n'))
                    {
                        // Only if no trailing atom will add the newline later.
                        if !trailing_leaf_atom_follows(pieces, pi, pname) {
                            push_synth_newline(&mut plain, &mut map);
                        }
                    }
                }
            }
        }
    }
    (plain, map)
}

fn trailing_leaf_atom_follows(pieces: &[Piece], idx: usize, parent_name: &str) -> bool {
    let mut depth = 0i32;
    for piece in pieces.iter().skip(idx + 1) {
        match piece {
            Piece::Text(t) => {
                if depth == 0 && t.chars().any(|c| !c.is_whitespace()) {
                    return false;
                }
            }
            Piece::Tag(t) => {
                let lower = t.to_ascii_lowercase();
                let Some(n) = tag_name(&lower) else {
                    continue;
                };
                if is_close_tag(t) {
                    if depth == 0 && n.eq_ignore_ascii_case(parent_name) {
                        return false;
                    }
                    depth -= 1;
                    continue;
                }
                if depth == 0 && matches!(n, "br" | "img") && is_self_closing(&lower) {
                    return true;
                }
                if depth == 0 {
                    return false;
                }
                if !is_self_closing(&lower) {
                    depth += 1;
                }
            }
        }
    }
    false
}

fn wrap_sentences(
    pieces: &[Piece],
    plain: &str,
    map: &[(usize, usize)],
    spans: &[SentenceSpan],
    chapter_id: Option<&str>,
) -> Vec<String> {
    let mut sentence_at = vec![None; plain.len()];
    for (sid, span) in spans.iter().enumerate() {
        let end = span.end.min(sentence_at.len());
        for slot in &mut sentence_at[span.start..end] {
            *slot = Some(sid);
        }
    }

    let mut piece_out: Vec<String> = pieces
        .iter()
        .map(|p| match p {
            Piece::Tag(t) => t.clone(),
            Piece::Text(t) => normalize_text_newlines(t),
        })
        .collect();

    #[derive(Clone, Copy)]
    enum Ev {
        Open(usize),
        Close,
    }
    let mut events: Vec<Vec<(usize, Ev)>> = vec![Vec::new(); pieces.len()];
    let mut open: Option<(usize, usize)> = None;

    let mut prev: Option<usize> = None;
    let mut prev_pi: Option<usize> = None;
    for (byte_i, &sid_opt) in sentence_at.iter().enumerate() {
        let Some(&(pi, off)) = map.get(byte_i) else {
            continue;
        };
        if pi == SYNTH_PI {
            continue;
        }
        let piece_changed = prev_pi.is_some_and(|p| p != pi);
        if sid_opt != prev || (sid_opt.is_some() && piece_changed) {
            if let Some((_sid, opi)) = open.take() {
                if opi == pi && !piece_changed {
                    events[pi].push((off, Ev::Close));
                } else {
                    let end = piece_out[opi].len();
                    events[opi].push((end, Ev::Close));
                }
            }
            if let Some(sid) = sid_opt {
                events[pi].push((off, Ev::Open(sid)));
                open = Some((sid, pi));
            }
            prev = sid_opt;
        }
        prev_pi = Some(pi);
    }
    if let Some((_sid, pi)) = open.take() {
        let end = piece_out[pi].len();
        events[pi].push((end, Ev::Close));
    }

    for (pi, evs) in events.iter_mut().enumerate() {
        if evs.is_empty() {
            continue;
        }
        evs.sort_by(|a, b| {
            a.0.cmp(&b.0).then_with(|| match (&a.1, &b.1) {
                (Ev::Close, Ev::Open(_)) => std::cmp::Ordering::Less,
                (Ev::Open(_), Ev::Close) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            })
        });
        let src = piece_out[pi].clone();
        let mut out = String::new();
        let mut cursor = 0usize;
        let mut first_open_done: std::collections::HashSet<usize> =
            std::collections::HashSet::new();
        for &(off, ref ev) in evs.iter() {
            let off = off.min(src.len());
            if off > cursor {
                out.push_str(&src[cursor..off]);
                cursor = off;
            }
            match *ev {
                Ev::Open(sid) => {
                    if first_open_done.insert(sid) {
                        let id = match chapter_id {
                            Some(cid) => format!("{cid}-s{sid}"),
                            None => format!("sentence-{sid}"),
                        };
                        out.push_str(&format!(
                            r#"<span id="{id}" class="shelf-mo" epub:type="storyteller:sentence-span">"#
                        ));
                    } else {
                        // Reopen across pieces (ST serializeDom id-once).
                        out.push_str(
                            r#"<span class="shelf-mo" epub:type="storyteller:sentence-span">"#,
                        );
                    }
                }
                Ev::Close => out.push_str("</span>"),
            }
        }
        if cursor < src.len() {
            out.push_str(&src[cursor..]);
        }
        piece_out[pi] = out;
    }

    piece_out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_assigns_duplicate_sentence_text_in_order() {
        // plain.find would latch both copies onto the first occurrence.
        let xhtml = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body>
<p>Hello world. Other words here. Hello world.</p>
</body></html>"#;
        let (out, sentences) = markup_xhtml_body(xhtml, None).unwrap();
        assert_eq!(sentences.len(), 3, "{sentences:?}");
        assert_eq!(sentences[0], sentences[2]);
        assert!(out.contains(r#"id="sentence-0""#), "{out}");
        assert!(out.contains(r#"id="sentence-2""#), "{out}");
        let first = out.find(r#"id="sentence-0""#).expect("s0");
        let last = out.rfind(r#"id="sentence-2""#).expect("s2");
        assert!(
            first < last,
            "duplicate sentences must wrap in document order"
        );
    }

    #[test]
    fn wraps_h1_and_paragraph_sentences() {
        let xhtml = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body>
<h1>Lamp and Rain</h1>
<p>The lamp on the desk throws a warm circle across the open pages. Outside the window a soft rain ticks against the glass.</p>
<p>She turns one leaf and finds the sentence she had marked yesterday. There is time enough for another chapter before the night ends.</p>
</body></html>"#;
        let (out, sentences) = markup_xhtml_body(xhtml, None).unwrap();
        // Block-boundary \n keeps title as its own sentence (ST liftText).
        assert_eq!(sentences.len(), 5, "sentences={sentences:?}");
        assert_eq!(sentences[0], "Lamp and Rain");
        assert!(
            sentences[1].contains("The lamp on the desk"),
            "{sentences:?}"
        );
        assert!(out.contains(r#"id="sentence-0""#), "{out}");
        assert!(
            out.contains(r#"id="sentence-0""#)
                && out.contains(r#"epub:type="storyteller:sentence-span""#),
            "title must be wrapped with ST sentence-span: {out}"
        );
        assert!(
            out.contains(r#"<p><span"#) && out.contains("The lamp"),
            "first prose must be inside a MO span: {out}"
        );
        assert!(out.contains(r#"id="sentence-1""#), "{out}");
    }

    #[test]
    fn lift_text_nested_paragraphs_insert_newlines() {
        let pieces = tokenize_body(
            "<div><p>This is a sentence.</p><p>This is a second sentence.</p><p>This is a third sentence.</p></div>",
        );
        let (plain, _) = lift_text(&pieces);
        assert_eq!(
            plain,
            "This is a sentence.\nThis is a second sentence.\nThis is a third sentence.\n"
        );
    }

    #[test]
    fn lift_text_trailing_atoms_force_newline() {
        let pieces = tokenize_body(r#"<h1>Chapter 1.<br/><img src="../images/cn.png"/></h1>"#);
        let (plain, _) = lift_text(&pieces);
        assert_eq!(plain, "Chapter 1.\n");
    }

    #[test]
    fn lift_text_strips_inter_block_leading_whitespace() {
        let pieces = tokenize_body(
            "<p>\n    This is a sentence.\n</p>\n<p>\n    This is a second sentence.\n</p>",
        );
        let (plain, _) = lift_text(&pieces);
        assert_eq!(plain, "This is a sentence. \nThis is a second sentence. \n");
    }

    #[test]
    fn wraps_two_sentences() {
        let xhtml = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Hello world. Goodbye moon!</p></body></html>"#;
        let (out, sentences) = markup_xhtml_body(xhtml, None).unwrap();
        assert_eq!(sentences.len(), 2);
        assert!(out.contains(r#"id="sentence-0""#));
        assert!(out.contains(r#"id="sentence-1""#));
        assert!(out.contains("Hello world."));
        assert!(out.contains("<p>")); // structure preserved
    }

    #[test]
    fn preserves_emphasis_across_sentence() {
        let xhtml = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body><p>He said <em>hello</em> there. Next.</p></body></html>"#;
        let (out, sentences) = markup_xhtml_body(xhtml, None).unwrap();
        assert!(sentences.len() >= 2);
        assert!(out.contains("<em>"));
        assert!(out.contains(r#"id="sentence-0""#));
        assert!(out.contains("Next."));
    }

    #[test]
    fn keeps_mr_abbreviation_in_sentence() {
        let sents = split_sentences("Mr. Smith went home. Then he slept.");
        assert_eq!(sents.len(), 2);
        assert!(sents[0].contains("Mr. Smith"));
    }

    #[test]
    fn keeps_mrs_and_dr_abbreviations() {
        let sents = split_sentences("Mrs. Jones met Dr. Lee yesterday. They talked.");
        assert_eq!(sents.len(), 2);
        assert!(sents[0].contains("Mrs. Jones"));
        assert!(sents[0].contains("Dr. Lee"));
        assert_eq!(sents[1], "They talked.");
    }

    #[test]
    fn ellipsis_ends_sentence_like_echogarden() {
        let sents = split_sentences("Wait... what happened next?");
        assert_eq!(sents.len(), 2);
        assert_eq!(sents[0], "Wait...");
        assert_eq!(sents[1], "what happened next?");
    }

    #[test]
    fn quote_after_period_stays_with_sentence() {
        let sents = split_sentences("He said \"Stop.\" She ignored him.");
        assert_eq!(sents.len(), 2);
        assert!(sents[0].ends_with("\"Stop.\""));
        assert_eq!(sents[1], "She ignored him.");
    }

    #[test]
    fn multi_sentence_paragraph() {
        let sents = split_sentences("Hello world. Goodbye moon! Is this the end? Yes it is.");
        assert_eq!(sents.len(), 4);
        assert_eq!(sents[0], "Hello world.");
        assert_eq!(sents[1], "Goodbye moon!");
        assert_eq!(sents[2], "Is this the end?");
        assert_eq!(sents[3], "Yes it is.");
    }

    #[test]
    fn decimal_and_usa_not_false_splits() {
        let sents = split_sentences("Version 3.14 shipped in the U.S.A. today. Upgrade now.");
        assert_eq!(sents.len(), 2);
        assert!(sents[0].contains("3.14"));
        assert!(sents[0].contains("U.S.A."));
    }

    #[test]
    fn fr_cldr_keeps_monsieur_abbrev() {
        let sents = split_sentences_locale("M. Dupont est parti. Ensuite il dormit.", "fr");
        assert_eq!(sents.len(), 2, "{sents:?}");
        assert!(sents[0].contains("M. Dupont"), "{sents:?}");
    }

    #[test]
    fn de_cldr_keeps_dr_abbrev() {
        let sents = split_sentences_locale("Dr. Müller ging nach Hause. Dann schlief er.", "de");
        assert_eq!(sents.len(), 2, "{sents:?}");
        assert!(sents[0].contains("Dr. Müller"), "{sents:?}");
    }

    #[test]
    fn ru_cldr_keeps_prof_abbrev() {
        let sents = split_sentences_locale("Проф. Иванов ушёл. Потом он спал.", "ru");
        assert_eq!(sents.len(), 2, "{sents:?}");
        assert!(sents[0].contains("Проф."), "{sents:?}");
    }

    #[test]
    fn markup_locale_fr_wires_through() {
        let xhtml = concat!(
            r#"<?xml version="1.0"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml">"#,
            r#"<body><p>M. Dupont est parti. Ensuite il dormit.</p></body></html>"#,
        );
        let (_, sents) = markup_xhtml_body_locale(xhtml, Some("ch"), "fr").unwrap();
        assert_eq!(sents.len(), 2, "{sents:?}");
    }

    #[test]
    fn xml_escape_used_when_needed() {
        assert_eq!(xml_escape("a&b"), "a&amp;b");
    }

    #[test]
    fn unpaired_footnote_body_is_segmented() {
        let xhtml = concat!(
            r#"<?xml version="1.0"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">"#,
            r#"<body><p>Main sentence one. Main sentence two.</p>"#,
            r#"<aside epub:type="footnote"><p>Secret footnote noise.</p></aside>"#,
            r#"</body></html>"#,
        );
        let (out, sentences) = markup_xhtml_body(xhtml, None).unwrap();
        let joined = sentences.join(" ");
        assert!(joined.contains("Main sentence"));
        assert!(
            sentences.iter().any(|s| s.contains("Secret footnote")),
            "unpaired footnote must not be dropped: {sentences:?}"
        );
        assert!(
            out.contains("Secret footnote") && out.contains("shelf-mo"),
            "footnote body should be marked in place: {out}"
        );
    }

    #[test]
    fn paired_footnote_lifts_then_restores() {
        let xhtml = concat!(
            r##"<?xml version="1.0"?>"##,
            r##"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">"##,
            r##"<body>"##,
            r##"<h1>Chapter 1.</h1>"##,
            r##"<p>Hello, world!<a epub:type="noteref" href="#footnote">1</a></p>"##,
            r##"<p>Goodbye, world!</p>"##,
            r##"<aside epub:type="footnote" id="footnote"><p>Footnote content.</p></aside>"##,
            r##"</body></html>"##,
        );
        let (out, sentences) = markup_xhtml_body(xhtml, None).unwrap();
        assert!(
            sentences.iter().any(|s| s.contains("Footnote content")),
            "footnote text must appear in sentence stream: {sentences:?}"
        );
        // After restore: noteref link back at original site; marked footnote in aside.
        assert!(
            out.contains("epub:type=\"noteref\"") && out.contains("href=\"#footnote\""),
            "noteref must be restored: {out}"
        );
        assert!(
            out.contains(r#"id="footnote""#)
                && out.contains("Footnote content")
                && out.contains("shelf-mo"),
            "marked footnote body must be restored to aside: {out}"
        );
        // Footnote sentence should sit between Hello and Goodbye in stream order.
        let fn_idx = sentences
            .iter()
            .position(|s| s.contains("Footnote content"))
            .unwrap();
        let hello_idx = sentences
            .iter()
            .position(|s| s.contains("Hello, world"))
            .unwrap();
        let bye_idx = sentences
            .iter()
            .position(|s| s.contains("Goodbye, world"))
            .unwrap();
        assert!(hello_idx < fn_idx && fn_idx < bye_idx, "{sentences:?}");
    }

    #[test]
    fn sentence_wrap_emits_storyteller_epub_type() {
        let xhtml = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Hello world. Goodbye moon!</p></body></html>"#;
        let (out, _) = markup_xhtml_body(xhtml, None).unwrap();
        assert!(out.contains(r#"id="sentence-0""#), "{out}");
        assert!(out.contains(r#"id="sentence-1""#), "{out}");
        assert!(
            out.contains(r#"epub:type="storyteller:sentence-span""#),
            "missing ST sentence-span: {out}"
        );
        assert!(
            out.contains("xmlns:epub"),
            "html should declare epub ns: {out}"
        );
        assert!(
            out.contains("storyteller:"),
            "html should declare storyteller prefix: {out}"
        );
    }

    #[test]
    fn wrap_with_chapter_id_emits_storyteller_chapter_s_ids() {
        // Production CTC path passes OPF chapter id → ST `{chapterId}-s{N}`.
        let xhtml = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Hello world. Goodbye moon!</p></body></html>"#;
        let (out, _) = markup_xhtml_body(xhtml, Some("ch1")).unwrap();
        assert!(out.contains(r#"id="ch1-s0""#), "{out}");
        assert!(out.contains(r#"id="ch1-s1""#), "{out}");
        assert!(!out.contains(r#"id="sentence-0""#), "{out}");
        assert!(
            out.contains(r#"epub:type="storyteller:sentence-span""#),
            "{out}"
        );
    }

    #[test]
    fn add_mark_covers_full_text_node() {
        let root = MarkRoot::new(vec![MarkChild::Node(MarkNode::new(
            "p",
            vec![MarkChild::Text(MarkedText::new("Hello, world!", vec![]))],
        ))]);
        let mark = Mark::new("span", &[("id", "test")]);
        let result = add_mark(root, 1, 14, mark.clone());
        match &result.children[0] {
            MarkChild::Node(n) => {
                assert_eq!(n.children.len(), 1);
                match &n.children[0] {
                    MarkChild::Text(t) => {
                        assert_eq!(t.text, "Hello, world!");
                        assert_eq!(t.marks, vec![mark]);
                    }
                    _ => panic!("expected text"),
                }
            }
            _ => panic!("expected p"),
        }
    }

    #[test]
    fn add_mark_at_start_of_text_node() {
        let root = MarkRoot::new(vec![MarkChild::Node(MarkNode::new(
            "p",
            vec![MarkChild::Text(MarkedText::new("Hello, world!", vec![]))],
        ))]);
        let mark = Mark::new("span", &[("id", "test")]);
        let result = add_mark(root, 1, 7, mark.clone());
        match &result.children[0] {
            MarkChild::Node(n) => {
                assert_eq!(n.children.len(), 2);
                match (&n.children[0], &n.children[1]) {
                    (MarkChild::Text(a), MarkChild::Text(b)) => {
                        assert_eq!(a.text, "Hello,");
                        assert_eq!(a.marks, vec![mark]);
                        assert_eq!(b.text, " world!");
                        assert!(b.marks.is_empty());
                    }
                    _ => panic!("expected two texts"),
                }
            }
            _ => panic!("expected p"),
        }
    }

    #[test]
    fn add_mark_within_text_preserves_existing_marks() {
        let existing = Mark::new("span", &[("class", "red")]);
        let root = MarkRoot::new(vec![MarkChild::Node(MarkNode::new(
            "p",
            vec![MarkChild::Text(MarkedText::new(
                "Hello, world!",
                vec![existing.clone()],
            ))],
        ))]);
        let mark = Mark::new("span", &[("id", "test")]);
        let result = add_mark(root, 4, 10, mark.clone());
        match &result.children[0] {
            MarkChild::Node(n) => {
                assert_eq!(n.children.len(), 3);
                match (&n.children[0], &n.children[1], &n.children[2]) {
                    (MarkChild::Text(a), MarkChild::Text(b), MarkChild::Text(c)) => {
                        assert_eq!(a.text, "Hel");
                        assert_eq!(a.marks, vec![existing.clone()]);
                        assert_eq!(b.text, "lo, wo");
                        assert_eq!(b.marks, vec![mark, existing.clone()]);
                        assert_eq!(c.text, "rld!");
                        assert_eq!(c.marks, vec![existing]);
                    }
                    _ => panic!("expected three texts"),
                }
            }
            _ => panic!("expected p"),
        }
    }

    #[test]
    fn xmltree_tokenize_preserves_nested_emphasis() {
        let pieces = tokenize_body("<p>He said <em>hello</em> there.</p>");
        let tags: Vec<_> = pieces
            .iter()
            .filter_map(|p| match p {
                Piece::Tag(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert!(tags.iter().any(|t| t.starts_with("<em")), "{tags:?}");
        assert!(tags.contains(&"</em>"), "{tags:?}");
        let (plain, _) = lift_text(&pieces);
        assert!(plain.contains("He said hello there"), "{plain:?}");
    }

    #[test]
    fn model_text_content_and_flags() {
        let p = MarkNode::new("p", vec![MarkChild::Text(MarkedText::new("Hi", vec![]))])
            .with_attrs(&[("class", "lead")]);
        assert!(p.is_block());
        assert!(!p.is_inline());
        assert!(!p.is_leaf());
        assert_eq!(p.border(), 1);
        assert_eq!(p.text_content(), "Hi");
        assert_eq!(p.attrs.get("class").map(String::as_str), Some("lead"));
        let fn_node = MarkNode::footnote("aside", vec![]);
        assert!(fn_node.is_footnote);
        assert!(fn_node.is_leaf());
    }

    #[test]
    fn model_find_index_replace_cut_descendants() {
        let root = MarkRoot::new(vec![MarkChild::Node(MarkNode::new(
            "p",
            vec![MarkChild::Text(MarkedText::new("Hello", vec![]))],
        ))]);
        assert_eq!(root.text_content(), "Hello");
        let (idx, off) = root.find_index(0).unwrap();
        assert_eq!((idx, off), (0, 0));

        let replaced = root
            .replace(
                0,
                MarkChild::Node(MarkNode::new(
                    "p",
                    vec![MarkChild::Text(MarkedText::new("Bye", vec![]))],
                )),
            )
            .unwrap();
        assert_eq!(replaced.text_content(), "Bye");

        let cut = root.cut(0).unwrap();
        assert_eq!(cut.text_content(), "Hello");

        let mut saw_text = false;
        descendants(&root, |child, _pos, _root, _i| {
            if matches!(child, MarkChild::Text(_)) {
                saw_text = true;
            }
            true
        });
        assert!(saw_text);
    }

    #[test]
    fn resolved_pos_path_depth_parent_node_after_before() {
        // <p>Hello</p> — p node_size 7, root content 7
        let root = MarkRoot::new(vec![MarkChild::Node(MarkNode::new(
            "p",
            vec![MarkChild::Text(MarkedText::new("Hello", vec![]))],
        ))]);
        assert_eq!(root.node_size(), 7);

        let r0 = root.resolve(0).unwrap();
        assert_eq!(r0.depth, 0);
        assert_eq!(r0.index(None), 0);
        assert!(matches!(r0.parent(), ResolveParent::Root(_)));
        assert_eq!(r0.node_after().unwrap().text_content(), "Hello");
        assert!(r0.before(None).is_err());

        let r3 = root.resolve(3).unwrap();
        assert_eq!(r3.depth, 1);
        assert_eq!(r3.parent_offset, 2);
        assert!(matches!(r3.node(Some(0)), ResolveNode::Root(_)));
        assert!(matches!(r3.parent(), ResolveParent::Node(n) if n.tag_name == "p"));
        assert_eq!(r3.index(None), 0);
        assert_eq!(r3.node_after().unwrap().text_content(), "llo");
        assert_eq!(r3.before(Some(1)).unwrap(), 0);
        assert_eq!(r3.before(Some(2)).unwrap(), 3);
        assert!(matches!(r3.node(Some(-1)), ResolveNode::Root(_)));
        assert!(r3.before(Some(-1)).is_err());

        let r7 = root.resolve(7).unwrap();
        assert_eq!(r7.depth, 0);
        assert_eq!(r7.index(None), 1);
        assert!(r7.node_after().is_none());

        assert!(root.resolve(8).is_err());
        assert!(MarkRoot::resolve(&root, 1).is_ok());
    }
}
