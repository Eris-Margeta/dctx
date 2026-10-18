//! Dotted-path assembly, ownership edges and cross-file reference policy.
//! Implements SPEC-MAP-MODE 5.1–5.4 for configuration keys.
//! JSON, TOML and YAML walkers emit into a [`Draft`]; callers consume `LexedFile`.
use crate::domain::{
    body_fingerprint, Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexError,
    LexedFile, Precision, RawReference,
};
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource, SymbolKind,
};
use std::collections::BTreeMap;

/// Byte cursor. `end` is an exclusive limit so one document can be parsed alone.
pub(crate) struct Cur<'a> {
    pub src: &'a str,
    pub i: usize,
    pub end: usize,
}
impl<'a> Cur<'a> {
    pub(crate) fn new(src: &'a str) -> Self { Self { src, i: 0, end: src.len() } }
    pub(crate) fn byte(&self) -> Option<u8> {
        if self.i >= self.end { None } else { self.src.as_bytes().get(self.i).copied() }
    }
    pub(crate) fn eat(&mut self, expected: u8) -> bool {
        if self.byte() == Some(expected) { self.i += 1; true } else { false }
    }
    pub(crate) fn bump(&mut self) -> Option<u8> { let byte = self.byte()?; self.i += 1; Some(byte) }
    pub(crate) fn eof(&self) -> bool { self.i >= self.end || self.i >= self.src.len() }
    pub(crate) fn slice(&self, start: usize, end: usize) -> &str { self.src.get(start..end).unwrap_or("") }
    pub(crate) fn rest(&self) -> &str { self.src.get(self.i..self.end.min(self.src.len())).unwrap_or("") }
    pub(crate) fn starts(&self, pat: &str) -> bool { self.rest().starts_with(pat) }
    pub(crate) fn skip_spaces(&mut self) {
        while matches!(self.byte(), Some(b' ' | b'\t' | b'\r')) { self.i += 1; }
    }
    pub(crate) fn consume_line(&mut self) {
        while let Some(byte) = self.bump() { if byte == b'\n' { break; } }
    }
}

/// First line of a `#` comment group that is not cut off by a blank line.
pub(crate) fn skip_hash_comments(cur: &mut Cur<'_>) -> Option<String> {
    let mut lead = None;
    loop {
        cur.skip_spaces();
        match cur.byte() {
            Some(b'\n') => cur.i += 1,
            Some(b'#') => {
                let text = hash_comment(cur);
                if lead.is_none() {
                    lead = Some(text);
                }
                if cur.eat(b'\n') {
                    let save = cur.i;
                    cur.skip_spaces();
                    if cur.byte() == Some(b'\n') {
                        lead = None;
                        cur.i = save;
                    }
                }
            }
            _ => break,
        }
    }
    lead.filter(|text| !text.is_empty())
}

pub(crate) fn hash_comment(cur: &mut Cur<'_>) -> String {
    if cur.byte() == Some(b'#') {
        cur.i += 1;
    }
    let start = cur.i;
    while let Some(byte) = cur.byte() {
        if byte == b'\n' {
            break;
        }
        cur.i += 1;
    }
    bounded(cur.slice(start, cur.i).trim().trim_end_matches('\r'), 100)
}

/// How a key is introduced. `Implicit` holds a dotted key. `Value`'s flag marks a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Implicit,
    Table,
    Array,
    Value(bool),
}
/// Placement of one path segment: new, reused parent, or abstained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Declared {
    Fresh(usize),
    Reused(usize),
    Skip,
}
impl Declared {
    pub(crate) fn index(self) -> Option<usize> {
        match self {
            Self::Fresh(index) | Self::Reused(index) => Some(index),
            Self::Skip => None,
        }
    }
}

struct DraftNode {
    node: Node, parent: usize, start: u32, end: u32, precision_end: u32,
    container: bool, opened: bool, array: bool, noted: bool,
}
struct DraftRef {
    src: usize,
    name: String,
    scope: String,
    end: u32,
    lines: [u32; 2],
}
struct Lines {
    starts: Vec<u32>,
}
impl Lines {
    fn new(source: &str) -> Self {
        let mut starts = vec![0];
        for (index, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(index as u32 + 1);
            }
        }
        Self { starts }
    }
    fn file_end(&self) -> u32 {
        u32::try_from(self.starts.len()).unwrap_or(u32::MAX).max(1)
    }
    fn line(&self, byte: usize) -> u32 {
        let byte = u32::try_from(byte).unwrap_or(u32::MAX);
        let line = u32::try_from(self.starts.partition_point(|&start| start <= byte)).unwrap_or(1);
        line.max(1).min(self.file_end())
    }
    fn span(&self, start: usize, end: usize) -> [u32; 2] {
        let file_end = self.file_end();
        let start_line = self.line(start).clamp(1, file_end);
        let last = end.saturating_sub(1).max(start);
        [start_line, self.line(last).clamp(start_line, file_end)]
    }
}

/// Owns the file node and every key emitted for one extract.
pub(crate) struct Draft<'a> {
    path: String, source: &'a str, lines: Lines, nodes: Vec<DraftNode>,
    names: BTreeMap<String, usize>, refs: Vec<DraftRef>, diagnostics: Vec<Diagnostic>,
    partial: bool, max_facts: usize, max_depth: u32,
}
impl<'a> Draft<'a> {
    pub(crate) fn open(path: &str, source: &'a str, max_facts: usize, max_depth: u32) -> Result<Self, LexError> {
        if max_facts == 0 {
            return Err(LexError::TooManyFacts { limit: 0 });
        }
        let lines = Lines::new(source);
        let module = Node {
            id: StableId::new(NodeKind::Module, path, None, None)?,
            kind: NodeKind::Module,
            symbol_kind: None,
            path: path.to_string(),
            lines: [1, lines.file_end()],
            name: path.to_string(),
            signature: String::new(),
            fingerprint: String::new(),
            public: false,
            summary: Summary { text: String::new(), source: SummarySource::Doc },
        };
        Ok(Self {
            path: path.to_string(),
            source,
            lines,
            nodes: vec![DraftNode {
                node: module,
                parent: 0,
                start: 0,
                end: u32::try_from(source.len()).unwrap_or(u32::MAX),
                precision_end: u32::MAX,
                container: true,
                opened: true,
                array: false,
                noted: true,
            }],
            names: BTreeMap::new(),
            refs: Vec::new(),
            diagnostics: Vec::new(),
            partial: false,
            max_facts,
            max_depth,
        })
    }
    pub(crate) fn line(&self, byte: usize) -> u32 {
        self.lines.line(byte)
    }
    fn segment(&self, index: usize) -> &str {
        self.nodes.get(index).map(|node| {
            node.node.name.rsplit_once('.').map_or(node.node.name.as_str(), |(_, tail)| tail)
        }).unwrap_or("")
    }
    pub(crate) fn full_name(&self, index: usize) -> String {
        self.nodes.get(index).map(|node| node.node.name.clone()).unwrap_or_default()
    }

    /// Inserts `segment` under `parent`. Depth above the ceiling refuses the file.
    pub(crate) fn put(
        &mut self,
        parent: usize,
        segment: &str,
        kind: Kind,
        start: usize,
        end: usize,
        depth: u32,
    ) -> Result<Declared, LexError> {
        if depth > self.max_depth {
            return Err(LexError::TooDeep { line: self.line(start), limit: self.max_depth });
        }
        if parent >= self.nodes.len() || !valid_segment(segment) {
            self.diagnose(start, DiagnosticReason::UnsupportedName);
            return Ok(Declared::Skip);
        }
        let name = if parent == 0 {
            segment.to_string()
        } else {
            format!("{}.{}", self.nodes[parent].node.name, segment)
        };
        if let Some(&index) = self.names.get(&name) {
            return self.reuse(index, kind, start, end);
        }
        let Ok(id) = StableId::new(NodeKind::Symbol, &self.path, Some(&name), None) else {
            self.diagnose(start, DiagnosticReason::UnsupportedName);
            return Ok(Declared::Skip);
        };
        self.charge()?;
        let (container, opened, array) = match kind {
            Kind::Implicit => (true, false, false),
            Kind::Table => (true, true, false),
            Kind::Array => (true, true, true),
            Kind::Value(flag) => (flag, true, false),
        };
        let end = end.max(start);
        let index = self.nodes.len();
        self.nodes.push(DraftNode {
            node: Node {
                id,
                kind: NodeKind::Symbol,
                symbol_kind: container.then_some(SymbolKind::Struct),
                path: self.path.clone(),
                lines: self.lines.span(start, end),
                name: name.clone(),
                signature: String::new(),
                fingerprint: String::new(),
                public: false,
                summary: Summary { text: String::new(), source: SummarySource::Signature },
            },
            parent,
            start: u32::try_from(start).unwrap_or(u32::MAX),
            end: u32::try_from(end).unwrap_or(u32::MAX),
            precision_end: u32::try_from(end).unwrap_or(u32::MAX),
            container,
            opened,
            array,
            noted: false,
        });
        self.names.insert(name, index);
        if !matches!(kind, Kind::Implicit) {
            self.note_dependency(index, start, end)?;
        }
        Ok(Declared::Fresh(index))
    }

    fn reuse(&mut self, index: usize, kind: Kind, start: usize, end: usize) -> Result<Declared, LexError> {
        let node = &self.nodes[index];
        let conflict = match kind {
            Kind::Implicit => !node.container,
            Kind::Table => node.array || !node.container || node.opened,
            Kind::Array => !node.container || (node.opened && !node.array),
            Kind::Value(_) => true,
        };
        if conflict {
            self.diagnose(start, DiagnosticReason::DuplicateKey);
            return Ok(Declared::Skip);
        }
        if let Kind::Table | Kind::Array = kind {
            let node = &mut self.nodes[index];
            node.container = true;
            node.opened = true;
            node.array = matches!(kind, Kind::Array);
        }
        self.extend(index, end.max(start));
        if !matches!(kind, Kind::Implicit) {
            self.note_dependency(index, start, end)?;
        }
        Ok(Declared::Reused(index))
    }

    pub(crate) fn extend(&mut self, index: usize, end: usize) {
        let mut current = index;
        while current != 0 && current < self.nodes.len() {
            let end_u = u32::try_from(end).unwrap_or(u32::MAX);
            if end_u > self.nodes[current].end {
                self.nodes[current].end = end_u;
                self.nodes[current].precision_end = end_u;
            }
            let start = self.nodes[current].start as usize;
            let lines = self.lines.span(start, end.max(start));
            if lines[1] > self.nodes[current].node.lines[1] {
                self.nodes[current].node.lines[1] = lines[1];
            }
            let parent = self.nodes[current].parent;
            if parent == current {
                break;
            }
            current = parent;
        }
    }

    pub(crate) fn set_signature(&mut self, index: usize, text: &str) {
        let Some(node) = self.nodes.get_mut(index) else { return };
        let text = bounded(text, 160);
        if text.is_empty() {
            return;
        }
        if node.node.signature.is_empty() {
            node.node.signature = text.clone();
        }
        if node.node.summary.text.is_empty() {
            node.node.summary = Summary { text, source: SummarySource::Signature };
        }
    }

    pub(crate) fn set_doc(&mut self, index: usize, text: &str) {
        let text = bounded(text, 100);
        if text.is_empty() || index >= self.nodes.len() {
            return;
        }
        let summary = &self.nodes[index].node.summary;
        if summary.source == SummarySource::Doc && !summary.text.is_empty() {
            return;
        }
        self.nodes[index].node.summary = Summary { text: text.clone(), source: SummarySource::Doc };
        if self.nodes[0].node.summary.text.is_empty() {
            self.nodes[0].node.summary = Summary { text, source: SummarySource::Doc };
        }
    }

    pub(crate) fn string_ref(&mut self, index: usize, value: &str, start: usize, end: usize) -> Result<(), LexError> {
        if index >= self.nodes.len() {
            return Ok(());
        }
        let value = value.trim();
        let key = self.segment(index).to_string();
        let parent = self.nodes[index].parent;
        let scope = if parent == 0 { String::new() } else { self.nodes[parent].node.name.clone() };
        if obvious_member(&scope, value) || obvious_path(&key, value) {
            self.emit(index, value, &scope, start, end)?;
        }
        Ok(())
    }

    fn note_dependency(&mut self, index: usize, start: usize, end: usize) -> Result<(), LexError> {
        if self.nodes.get(index).is_some_and(|node| node.noted) {
            return Ok(());
        }
        let parent = self.nodes[index].parent;
        let scope = if parent == 0 { String::new() } else { self.nodes[parent].node.name.clone() };
        if !dependency_table(&scope) {
            return Ok(());
        }
        let name = self.segment(index).to_string();
        if name.is_empty() {
            return Ok(());
        }
        self.nodes[index].noted = true;
        self.emit(index, &name, &scope, start, end)
    }

    fn emit(&mut self, src: usize, name: &str, scope: &str, start: usize, end: usize) -> Result<(), LexError> {
        if name.is_empty() {
            return Ok(());
        }
        self.charge()?;
        self.refs.push(DraftRef {
            src,
            name: name.to_string(),
            scope: scope.to_string(),
            end: u32::try_from(end).unwrap_or(u32::MAX),
            lines: self.lines.span(start, end.max(start.saturating_add(1))),
        });
        Ok(())
    }

    pub(crate) fn diagnose(&mut self, byte: usize, reason: DiagnosticReason) {
        self.partial = true;
        if self.diagnostics.len() >= LexedFile::MAX_DIAGNOSTICS {
            return;
        }
        let byte_u = u32::try_from(byte.min(self.source.len())).unwrap_or(u32::MAX);
        self.diagnostics.push(Diagnostic { line: self.line(byte), byte: byte_u, reason });
    }

    pub(crate) fn finish(mut self, dialect: Dialect) -> Result<LexedFile, LexError> {
        self.diagnostics.sort_by_key(|item| (item.byte, item.reason as u8));
        let first = self.diagnostics.first().map(|item| item.byte).unwrap_or(u32::MAX);
        let completion = if self.partial { Completion::Partial } else { Completion::Complete };
        let path = self.path.clone();
        let label = |end: u32, base: Precision| {
            if end != u32::MAX && end > first { Precision::Partial } else { base }
        };
        for node in &mut self.nodes {
            let start = node.start as usize;
            let end = (node.end as usize).min(self.source.len()).max(start);
            node.node.fingerprint = if node.precision_end == u32::MAX {
                body_fingerprint(self.source)
            } else {
                body_fingerprint(self.source.get(start..end).unwrap_or(""))
            };
        }
        let edges = self.nodes.iter().skip(1).map(|child| Labeled {
            precision: label(child.precision_end, Precision::Structural),
            fact: Edge {
                src: self.nodes[child.parent].node.id.clone(),
                dst: child.node.id.clone(),
                kind: EdgeKind::Contains,
                conf: Confidence::Exact,
                owner_path: path.clone(),
            },
        }).collect();
        let references = self.refs.iter().map(|reference| Labeled {
            precision: label(reference.end, Precision::Convention),
            fact: RawReference {
                src: self.nodes[reference.src].node.id.clone(),
                kind: EdgeKind::Imports,
                name: reference.name.clone(),
                scope: reference.scope.clone(),
                alias: None,
                lines: reference.lines,
                owner_path: path.clone(),
            },
        }).collect();
        let nodes = self.nodes.into_iter().map(|draft| Labeled {
            precision: label(draft.precision_end, Precision::Structural),
            fact: draft.node,
        }).collect();
        Ok(LexedFile { path, dialect, completion, diagnostics: self.diagnostics, nodes, edges, references })
    }

    fn charge(&self) -> Result<(), LexError> {
        if self.nodes.len() + self.refs.len() >= self.max_facts {
            Err(LexError::TooManyFacts { limit: self.max_facts })
        } else {
            Ok(())
        }
    }
}

fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 512
        && !segment.chars().any(|ch| ch.is_control() || matches!(ch, '#' | '~'))
}

fn bounded(text: &str, max: usize) -> String {
    let mut out = String::new();
    for word in text.split_whitespace() {
        let room = max.saturating_sub(out.chars().count());
        if room == 0 { break; }
        if !out.is_empty() {
            if room == 1 { break; }
            out.push(' ');
        }
        out.extend(word.chars().take(room.saturating_sub(usize::from(!out.is_empty()))));
    }
    out
}

pub(crate) fn dependency_table(name: &str) -> bool {
    matches!(name, "dependencies" | "dev-dependencies" | "build-dependencies" | "devDependencies" | "peerDependencies" | "optionalDependencies" | "workspace.dependencies")
}

pub(crate) fn obvious_member(parent: &str, value: &str) -> bool {
    matches!(parent, "workspace.members" | "workspaces") && plain_token(value)
}

pub(crate) fn obvious_path(key: &str, value: &str) -> bool {
    plain_token(value)
        && (value.starts_with("./") || value.starts_with("../") || (key == "path" && value.contains('/')))
}

fn plain_token(value: &str) -> bool {
    let value = value.trim();
    (1..=240).contains(&value.len())
        && !value.starts_with('/')
        && value != "."
        && value != ".."
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b'+' | b'~'))
}
