//! Dotted-path assembly, ownership edges and cross-file reference policy.
//! Implements SPEC-MAP-MODE 5.1–5.4 for configuration keys.
//! JSON, TOML and YAML walkers emit into a [`Draft`]; callers consume `LexedFile`.
//! Names are hashed from the parent hash plus the new segment; collisions compare the path.
//! Identities and fingerprints are built only after the walk succeeds, in parallel when it is large.
use crate::domain::{
    body_fingerprint, fingerprint_plain, has_whitespace, Completion, Diagnostic, DiagnosticReason,
    Dialect, Labeled, LexError, LexedFile, Precision, RawReference,
};
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource, SymbolKind,
};
use std::collections::HashMap;

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

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

fn fnv(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Full dotted names keyed by hash. The node stores the only copy of the name.
struct NameIndex {
    first: HashMap<u64, usize>,
    more: Vec<(u64, usize)>,
}

impl NameIndex {
    fn with_capacity(capacity: usize) -> Self {
        Self { first: HashMap::with_capacity(capacity), more: Vec::new() }
    }

    fn find(&self, hash: u64, name: &str, nodes: &[DraftNode], buf: &str) -> Option<usize> {
        let index = *self.first.get(&hash)?;
        if nodes.get(index).is_some_and(|node| name_at(buf, node) == name) {
            return Some(index);
        }
        self.more.iter().find_map(|&(candidate, index)| {
            (candidate == hash && nodes.get(index).is_some_and(|node| name_at(buf, node) == name)).then_some(index)
        })
    }

    fn insert(&mut self, hash: u64, index: usize) {
        if let std::collections::hash_map::Entry::Vacant(e) = self.first.entry(hash) {
            e.insert(index);
        } else {
            self.more.push((hash, index));
        }
    }
}

/// Text kept as a source span until finish, so a refused file does not allocate it.
#[derive(Clone)]
enum Stored {
    Empty,
    Span(u32, u32),
    Owned(String),
}

impl Stored {
    fn is_empty(&self) -> bool {
        match self {
            Self::Empty => true,
            Self::Owned(text) => text.is_empty(),
            Self::Span(start, end) => start == end,
        }
    }

    fn render(&self, source: &str) -> String {
        match self {
            Self::Empty => String::new(),
            Self::Owned(text) => text.clone(),
            Self::Span(start, end) => source
                .get(*start as usize..*end as usize)
                .map(str::to_string)
                .unwrap_or_default(),
        }
    }
}

#[allow(clippy::struct_excessive_bools, reason = "Container shape, array shape, open state, dependency emission and module identity are separate facts retained during staged node assembly.")]
struct DraftNode {
    /// Byte range of the dotted name inside [`Draft::names_buf`].
    name_start: u32,
    name_end: u32,
    name_hash: u64,
    parent: usize,
    start: u32,
    end: u32,
    precision_end: u32,
    lines: [u32; 2],
    container: bool,
    opened: bool,
    array: bool,
    noted: bool,
    module: bool,
    symbol_kind: Option<SymbolKind>,
    signature: Stored,
    summary_text: Stored,
    summary_source: SummarySource,
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
    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
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
    path: String,
    source: &'a str,
    lines: Lines,
    nodes: Vec<DraftNode>,
    names: NameIndex,
    /// Every dotted name, packed. A refused file drops this buffer with the draft.
    names_buf: String,
    refs: Vec<DraftRef>,
    diagnostics: Vec<Diagnostic>,
    partial: bool,
    max_facts: usize,
    max_depth: u32,
    plain: bool,
    one_line: bool,
    /// Proves the path is a model id. Symbol ids are built from the same path at finish.
    module_id: StableId,
}
impl<'a> Draft<'a> {
    pub(crate) fn open(path: &str, source: &'a str, max_facts: usize, max_depth: u32) -> Result<Self, LexError> {
        if max_facts == 0 {
            return Err(LexError::TooManyFacts { limit: 0 });
        }
        let lines = Lines::new(source);
        let one_line = lines.starts.len() == 1;
        let plain = !has_whitespace(source);
        let guess = (source.len() / 16).clamp(1, max_facts);
        let module_id = StableId::new(NodeKind::Module, path, None, None).map_err(LexError::InvalidId)?;
        let mut names_buf = String::with_capacity((guess.saturating_mul(24)).min(8 << 20));
        names_buf.push_str(path);
        let mut nodes = Vec::with_capacity(guess);
        nodes.push(DraftNode {
            name_start: 0,
            name_end: u32::try_from(names_buf.len()).unwrap_or(u32::MAX),
            name_hash: 0,
            parent: 0,
            start: 0,
            end: u32::try_from(source.len()).unwrap_or(u32::MAX),
            precision_end: u32::MAX,
            lines: [1, lines.file_end()],
            container: true,
            opened: true,
            array: false,
            noted: true,
            module: true,
            symbol_kind: None,
            signature: Stored::Empty,
            summary_text: Stored::Empty,
            summary_source: SummarySource::Doc,
        });
        Ok(Self {
            path: path.to_string(),
            source,
            lines,
            nodes,
            names: NameIndex::with_capacity(guess),
            names_buf,
            refs: Vec::new(),
            diagnostics: Vec::new(),
            partial: false,
            max_facts,
            max_depth,
            plain,
            one_line,
            module_id,
        })
    }
    pub(crate) fn line(&self, byte: usize) -> u32 {
        self.lines.line(byte)
    }
    fn name_of(&self, index: usize) -> &str {
        self.nodes.get(index).map_or("", |node| name_at(&self.names_buf, node))
    }
    fn segment(&self, index: usize) -> &str {
        let name = self.name_of(index);
        name.rsplit_once('.').map_or(name, |(_, tail)| tail)
    }
    pub(crate) fn full_name(&self, index: usize) -> String {
        self.name_of(index).to_string()
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
        let name_at = self.names_buf.len();
        let (name_start, name_end, hash) = self.append_name(parent, segment);
        let found = {
            let name = name_at_range(&self.names_buf, name_start, name_end);
            self.names.find(hash, name, &self.nodes, &self.names_buf)
        };
        if let Some(index) = found {
            self.names_buf.truncate(name_at);
            return self.reuse(index, kind, start, end);
        }
        // A segment that passes `valid_segment` builds a symbol id whenever the module id did.
        self.charge()?;
        let (container, opened, array) = match kind {
            Kind::Implicit => (true, false, false),
            Kind::Table => (true, true, false),
            Kind::Array => (true, true, true),
            Kind::Value(flag) => (flag, true, false),
        };
        let end = end.max(start);
        let index = self.nodes.len();
        let lines = if self.one_line { [1, 1] } else { self.lines.span(start, end) };
        self.nodes.push(DraftNode {
            name_start,
            name_end,
            name_hash: hash,
            parent,
            start: u32::try_from(start).unwrap_or(u32::MAX),
            end: u32::try_from(end).unwrap_or(u32::MAX),
            precision_end: u32::try_from(end).unwrap_or(u32::MAX),
            lines,
            container,
            opened,
            array,
            noted: false,
            module: false,
            symbol_kind: container.then_some(SymbolKind::Struct),
            signature: Stored::Empty,
            summary_text: Stored::Empty,
            summary_source: SummarySource::Signature,
        });
        self.names.insert(hash, index);
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
        let end_u = u32::try_from(end).unwrap_or(u32::MAX);
        // One lookup for the shared end byte; ancestors only grow their last line.
        let end_line = if self.one_line { 1 } else { self.lines.line(end.saturating_sub(1)) };
        while current != 0 && current < self.nodes.len() {
            if end_u > self.nodes[current].end {
                self.nodes[current].end = end_u;
                self.nodes[current].precision_end = end_u;
            }
            if !self.one_line {
                let start = self.nodes[current].start as usize;
                let line = if start > end.saturating_sub(1) { self.lines.line(start) } else { end_line };
                if line > self.nodes[current].lines[1] {
                    self.nodes[current].lines[1] = line;
                }
            }
            let parent = self.nodes[current].parent;
            if parent == current {
                break;
            }
            current = parent;
        }
    }

    pub(crate) fn set_signature(&mut self, index: usize, text: &str) {
        if index >= self.nodes.len() || text.is_empty() {
            return;
        }
        let need_sig = self.nodes[index].signature.is_empty();
        let need_sum = self.nodes[index].summary_text.is_empty();
        if !need_sig && !need_sum {
            return;
        }
        let stored = self.store_text(text, 160);
        if stored.is_empty() {
            return;
        }
        if need_sig && need_sum {
            self.nodes[index].signature = stored.clone();
            self.nodes[index].summary_text = stored;
            self.nodes[index].summary_source = SummarySource::Signature;
        } else if need_sig {
            self.nodes[index].signature = stored;
        } else if need_sum {
            self.nodes[index].summary_text = stored;
            self.nodes[index].summary_source = SummarySource::Signature;
        }
    }

    pub(crate) fn set_doc(&mut self, index: usize, text: &str) {
        if index >= self.nodes.len() {
            return;
        }
        let source = self.nodes[index].summary_source;
        let empty = self.nodes[index].summary_text.is_empty();
        if source == SummarySource::Doc && !empty {
            return;
        }
        let stored = self.store_text(text, 100);
        if stored.is_empty() {
            return;
        }
        let module_empty = self.nodes[0].summary_text.is_empty();
        if module_empty && index != 0 {
            self.nodes[index].summary_text = stored.clone();
            self.nodes[index].summary_source = SummarySource::Doc;
            self.nodes[0].summary_text = stored;
            self.nodes[0].summary_source = SummarySource::Doc;
        } else {
            self.nodes[index].summary_text = stored;
            self.nodes[index].summary_source = SummarySource::Doc;
        }
    }

    pub(crate) fn string_ref(&mut self, index: usize, value: &str, start: usize, end: usize) -> Result<(), LexError> {
        if index >= self.nodes.len() {
            return Ok(());
        }
        let value = value.trim();
        let hit = {
            let key = self.segment(index);
            let parent = self.nodes[index].parent;
            let scope = if parent == 0 { "" } else { self.name_of(parent) };
            obvious_member(scope, value) || obvious_path(key, value)
        };
        if !hit {
            return Ok(());
        }
        let parent = self.nodes[index].parent;
        let scope = if parent == 0 { String::new() } else { self.name_of(parent).to_string() };
        self.emit(index, value, &scope, start, end)
    }

    fn note_dependency(&mut self, index: usize, start: usize, end: usize) -> Result<(), LexError> {
        if self.nodes.get(index).is_some_and(|node| node.noted) {
            return Ok(());
        }
        let Some(parent) = self.nodes.get(index).map(|node| node.parent) else {
            return Ok(());
        };
        let hit = {
            let scope = if parent == 0 { "" } else { self.name_of(parent) };
            dependency_table(scope)
        };
        if !hit {
            return Ok(());
        }
        let name = self.segment(index).to_string();
        if name.is_empty() {
            return Ok(());
        }
        let scope = if parent == 0 { String::new() } else { self.name_of(parent).to_string() };
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

    pub(crate) fn finish(self, dialect: Dialect) -> Result<LexedFile, LexError> {
        let workers = worker_count(self.nodes.len());
        self.finish_with(dialect, workers)
    }

    fn finish_with(mut self, dialect: Dialect, workers: usize) -> Result<LexedFile, LexError> {
        self.diagnostics.sort_by_key(|item| (item.byte, item.reason as u8));
        let first = self.diagnostics.first().map_or(u32::MAX, |item| item.byte);
        let completion = if self.partial { Completion::Partial } else { Completion::Complete };
        let prepared = self.prepare_all(workers)?;
        // Small files lose to allocator contention when edges are built on many threads.
        let edge_workers = if self.nodes.len() >= 100_000 { workers } else { 1 };
        let edges = self.edges_from(&prepared, first, edge_workers)?;
        let references = self.references_from(&prepared, first);
        let path = self.path.clone();
        let nodes = self.nodes.into_iter().zip(prepared).map(|(draft, item)| Labeled {
            precision: precision_at(draft.precision_end, first, Precision::Structural),
            fact: Node {
                id: item.id,
                kind: if draft.module { NodeKind::Module } else { NodeKind::Symbol },
                symbol_kind: draft.symbol_kind,
                path: item.path,
                lines: draft.lines,
                name: item.name,
                signature: item.signature,
                fingerprint: item.fingerprint,
                public: false,
                summary: Summary { text: item.summary, source: draft.summary_source },
            },
        }).collect();
        Ok(LexedFile { path, dialect, completion, diagnostics: self.diagnostics, nodes, edges, references })
    }

    /// A slice of `source` stays a span. Anything else is the bounded owned string.
    fn store_text(&self, text: &str, max: usize) -> Stored {
        if text.is_empty() {
            return Stored::Empty;
        }
        let inside = self.byte_span(text);
        let whitespace = match inside {
            Some(_) if self.plain => false,
            _ => has_whitespace(text),
        };
        if text.len() <= max && !whitespace {
            if let Some((start, end)) = inside {
                return Stored::Span(start, end);
            }
            return Stored::Owned(text.to_string());
        }
        let owned = bounded(text, max);
        if owned.is_empty() { Stored::Empty } else { Stored::Owned(owned) }
    }

    fn byte_span(&self, text: &str) -> Option<(u32, u32)> {
        let base = self.source.as_ptr() as usize;
        let start = text.as_ptr() as usize;
        let end = start.checked_add(text.len())?;
        let source_end = base.checked_add(self.source.len())?;
        if start < base || end > source_end {
            return None;
        }
        let offset = start - base;
        Some((u32::try_from(offset).ok()?, u32::try_from(offset + text.len()).ok()?))
    }

    fn prepare_all(&self, workers: usize) -> Result<Vec<Prepared>, LexError> {
        let ranges = byte_ranges(&self.nodes, workers);
        let buf = self.names_buf.as_str();
        dispatch(&ranges, |start, end| {
            prepare_range(&self.nodes[start..end], self.source, &self.path, self.plain, &self.module_id, buf)
        })
    }

    fn edges_from(&self, prepared: &[Prepared], first: u32, workers: usize) -> Result<Vec<Labeled<Edge>>, LexError> {
        if self.nodes.len() <= 1 {
            return Ok(Vec::new());
        }
        let ranges = count_ranges(1, self.nodes.len(), workers);
        let path = self.path.as_str();
        dispatch(&ranges, |start, end| {
            let mut edges = Vec::with_capacity(end.saturating_sub(start));
            for index in start..end {
                let Some(child) = self.nodes.get(index) else {
                    return Err(LexError::InvalidId(dctx_map_model::InvalidId));
                };
                let Some(src) = prepared.get(child.parent) else {
                    return Err(LexError::InvalidId(dctx_map_model::InvalidId));
                };
                let Some(dst) = prepared.get(index) else {
                    return Err(LexError::InvalidId(dctx_map_model::InvalidId));
                };
                edges.push(Labeled {
                    precision: precision_at(child.precision_end, first, Precision::Structural),
                    fact: Edge {
                        src: src.id.clone(),
                        dst: dst.id.clone(),
                        kind: EdgeKind::Contains,
                        conf: Confidence::Exact,
                        owner_path: path.to_string(),
                    },
                });
            }
            Ok(edges)
        })
    }

    fn references_from(&self, prepared: &[Prepared], first: u32) -> Vec<Labeled<RawReference>> {
        let mut references = Vec::with_capacity(self.refs.len());
        for reference in &self.refs {
            let Some(src) = prepared.get(reference.src) else { continue };
            references.push(Labeled {
                precision: precision_at(reference.end, first, Precision::Convention),
                fact: RawReference {
                    src: src.id.clone(),
                    kind: EdgeKind::Imports,
                    name: reference.name.clone(),
                    scope: reference.scope.clone(),
                    alias: None,
                    lines: reference.lines,
                    owner_path: self.path.clone(),
                },
            });
        }
        references
    }

    /// Appends `parent.name + '.' + segment` (or just `segment` at the root) to `names_buf`.
    fn append_name(&mut self, parent: usize, segment: &str) -> (u32, u32, u64) {
        let start = self.names_buf.len();
        let hash = if parent == 0 || parent >= self.nodes.len() {
            self.names_buf.push_str(segment);
            fnv(FNV_OFFSET, segment.as_bytes())
        } else {
            let ps = self.nodes[parent].name_start as usize;
            let pe = self.nodes[parent].name_end as usize;
            let parent_hash = self.nodes[parent].name_hash;
            let parent_len = pe.saturating_sub(ps);
            if parent_len <= 128 {
                let mut tmp = [0u8; 128];
                let bytes = self.names_buf.as_bytes();
                let from = ps.min(bytes.len());
                let to = pe.min(bytes.len()).max(from);
                let len = to - from;
                tmp[..len].copy_from_slice(&bytes[from..to]);
                if let Ok(text) = std::str::from_utf8(&tmp[..len]) {
                    self.names_buf.push_str(text);
                }
            } else if let Some(text) = self.names_buf.get(ps..pe) {
                let owned = text.to_string();
                self.names_buf.push_str(&owned);
            }
            self.names_buf.push('.');
            self.names_buf.push_str(segment);
            fnv(fnv(parent_hash, b"."), segment.as_bytes())
        };
        (u32::try_from(start).unwrap_or(0), u32::try_from(self.names_buf.len()).unwrap_or(u32::MAX), hash)
    }

    fn charge(&self) -> Result<(), LexError> {
        if self.nodes.len() + self.refs.len() >= self.max_facts {
            Err(LexError::TooManyFacts { limit: self.max_facts })
        } else {
            Ok(())
        }
    }
}

struct Prepared {
    id: StableId,
    path: String,
    fingerprint: String,
    signature: String,
    summary: String,
    name: String,
}

fn name_at<'a>(buf: &'a str, node: &DraftNode) -> &'a str {
    name_at_range(buf, node.name_start, node.name_end)
}

fn name_at_range(buf: &str, start: u32, end: u32) -> &str {
    buf.get(start as usize..end as usize).unwrap_or("")
}

fn worker_count(nodes: usize) -> usize {
    if nodes < 1024 {
        return 1;
    }
    let available = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    available.clamp(1, (nodes / 512).max(1))
}

fn precision_at(end: u32, first: u32, base: Precision) -> Precision {
    if end != u32::MAX && end > first { Precision::Partial } else { base }
}

fn span_len(node: &DraftNode) -> u64 {
    u64::from(node.end.saturating_sub(node.start)).max(1)
}

/// Splits nodes into `workers` ranges with similar fingerprint byte counts.
fn byte_ranges(nodes: &[DraftNode], workers: usize) -> Vec<(usize, usize)> {
    if nodes.is_empty() {
        return Vec::new();
    }
    let workers = workers.max(1).min(nodes.len());
    if workers == 1 {
        return vec![(0, nodes.len())];
    }
    let total = nodes.iter().map(span_len).sum::<u64>().max(1);
    let target = (total / workers as u64).max(1);
    let mut ranges = Vec::with_capacity(workers);
    let mut start = 0usize;
    let mut acc = 0u64;
    for (index, node) in nodes.iter().enumerate() {
        acc += span_len(node);
        let last = index + 1 == nodes.len();
        let enough = acc >= target && ranges.len() + 1 < workers;
        if enough || last {
            ranges.push((start, index + 1));
            start = index + 1;
            acc = 0;
        }
    }
    ranges
}

fn count_ranges(start: usize, end: usize, workers: usize) -> Vec<(usize, usize)> {
    let len = end.saturating_sub(start);
    if len == 0 {
        return Vec::new();
    }
    let workers = workers.max(1).min(len);
    let chunk = len.div_ceil(workers);
    let mut ranges = Vec::with_capacity(workers);
    let mut cursor = start;
    while cursor < end {
        let next = (cursor + chunk).min(end);
        ranges.push((cursor, next));
        cursor = next;
    }
    ranges
}

/// Runs `build` over `ranges` and concatenates in range order.
fn dispatch<T, F>(ranges: &[(usize, usize)], build: F) -> Result<Vec<T>, LexError>
where
    T: Send,
    F: Fn(usize, usize) -> Result<Vec<T>, LexError> + Sync,
{
    if ranges.len() <= 1 {
        let (start, end) = ranges.first().copied().unwrap_or((0, 0));
        return build(start, end);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let build = &build;
    std::thread::scope(|scope| {
        for (index, &(start, end)) in ranges.iter().enumerate() {
            let tx = tx.clone();
            scope.spawn(move || {
                let _ = tx.send((index, build(start, end)));
            });
        }
    });
    drop(tx);
    let mut parts = Vec::with_capacity(ranges.len());
    for _ in 0..ranges.len() {
        parts.push(None);
    }
    while let Ok((index, result)) = rx.recv() {
        if index < parts.len() {
            parts[index] = Some(result);
        }
    }
    let mut out = Vec::new();
    for part in parts {
        match part {
            Some(Ok(items)) => out.extend(items),
            Some(Err(error)) => return Err(error),
            None => return Err(LexError::InvalidId(dctx_map_model::InvalidId)),
        }
    }
    Ok(out)
}

fn node_body<'a>(node: &DraftNode, source: &'a str) -> &'a str {
    if node.precision_end == u32::MAX {
        return source;
    }
    let start = node.start as usize;
    let end = (node.end as usize).min(source.len()).max(start);
    source.get(start..end).unwrap_or("")
}

fn prepare_range(
    nodes: &[DraftNode],
    source: &str,
    path: &str,
    plain: bool,
    module_id: &StableId,
    names: &str,
) -> Result<Vec<Prepared>, LexError> {
    let mut prepared = Vec::with_capacity(nodes.len());
    for node in nodes {
        let name = name_at(names, node).to_string();
        let id = if node.module {
            module_id.clone()
        } else {
            StableId::new(NodeKind::Symbol, path, Some(name.as_str()), None).map_err(LexError::InvalidId)?
        };
        let body = node_body(node, source);
        // `plain` is file-wide, so every source span is free of whitespace and hashes raw.
        let fingerprint = if plain { fingerprint_plain(body) } else { body_fingerprint(body) };
        prepared.push(Prepared {
            id,
            path: path.to_string(),
            fingerprint,
            signature: node.signature.render(source),
            summary: node.summary_text.render(source),
            name,
        });
    }
    Ok(prepared)
}

fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 512
        && !segment.chars().any(|ch| ch.is_control() || matches!(ch, '#' | '~'))
}

fn bounded(text: &str, max: usize) -> String {
    if text.len() <= max && !has_whitespace(text) {
        return text.to_string();
    }
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

#[cfg(test)]
mod tests;
