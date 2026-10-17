//! Structural walker: scopes, fact assembly and top-level declaration dispatch.
//! Implements SPEC-MAP-MODE 5.1–5.4 for Go. A declaration owns the spans lexically
//! inside it. Uses lexer tokens; declarations and reference scans are sibling modules.
mod declarations;
mod references;

use crate::domain::{
    body_fingerprint, Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexError,
    LexedFile, Limits, Precision, RawReference,
};
use crate::lexer::{keyword, Kind, Lexed, Token, NONE};
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource, SymbolKind,
};
use std::cmp::Reverse;
use std::collections::BTreeMap;

/// Extracts facts from an already-lexed Go file.
pub(crate) fn extract(
    path: &str,
    source: &str,
    lexed: &Lexed,
    limits: &Limits,
) -> Result<LexedFile, LexError> {
    let draft = Draft::new(path, source, limits.max_facts)?;
    let mut walker = Walker {
        src: source,
        toks: &lexed.tokens,
        partner: &lexed.partner,
        lexed,
        draft,
        saw_package: false,
    };
    walker.file();
    walker.draft.finish(lexed, Dialect::Go, limits.max_facts)
}

/// Lexical owner of nested facts. `name` is `::`-qualified and empty at the root.
#[derive(Debug, Clone, Default)]
pub(crate) struct Scope {
    /// Index of the owning draft node.
    pub owner: usize,
    /// Qualified name, empty for the file root.
    pub name: String,
}

/// A recognised declaration, in token and byte coordinates.
pub(crate) struct Item {
    /// Graph category.
    pub node_kind: NodeKind,
    /// Parsed symbol category. Vars have none; the model has no var kind.
    pub symbol: Option<SymbolKind>,
    /// Name qualified by a receiver when this item is a method (`Box::Serve`).
    pub local: String,
    /// Bare identifier used for export and doc lookup.
    pub bare: String,
    /// First token of the declaration.
    pub first: usize,
    /// Last token of the declaration.
    pub last: usize,
    /// Byte where the signature ends, exclusive.
    pub sig_end: u32,
    /// Body bytes, or the whole declaration when there is no brace body.
    pub body: (u32, u32),
    /// Token whose leading comment documents the declaration.
    pub doc_at: Option<usize>,
}

pub(crate) struct Walker<'a> {
    src: &'a str,
    toks: &'a [Token],
    partner: &'a [u32],
    lexed: &'a Lexed,
    draft: Draft,
    saw_package: bool,
}

impl<'a> Walker<'a> {
    fn file(&mut self) {
        let end = self.toks.len();
        let mut index = 0;
        while index < end {
            let before = index;
            index = self.skip_semis(index, end);
            if index >= end {
                break;
            }
            index = match self.text(index) {
                "package" => self.package_clause(index, end),
                "import" => self.import_decl(index, end),
                "func" => self.func_decl(index, end, &Scope::default()),
                "type" => self.type_decl(index, end),
                "const" => self.value_decl(index, end, true),
                "var" => self.value_decl(index, end, false),
                _ => self.skip_foreign(index, end),
            };
            if index <= before {
                index = before + 1;
            }
        }
    }

    fn skip_foreign(&mut self, index: usize, end: usize) -> usize {
        self.problem(self.start_byte(index), DiagnosticReason::Unrecognized);
        if matches!(self.text(index), "(" | "[" | "{") {
            self.after(index).min(end)
        } else {
            index + 1
        }
    }

    pub(crate) fn text(&self, index: usize) -> &'a str {
        self.toks.get(index).map_or("", |token| {
            self.src.get(token.start as usize..token.end as usize).unwrap_or("")
        })
    }

    pub(crate) fn kind(&self, index: usize) -> Option<Kind> {
        self.toks.get(index).map(|token| token.kind)
    }

    pub(crate) fn is(&self, index: usize, spelling: &str) -> bool {
        self.text(index) == spelling
    }

    pub(crate) fn is_semi(&self, index: usize) -> bool {
        self.kind(index) == Some(Kind::Semi)
    }

    pub(crate) fn ident(&self, index: usize) -> bool {
        self.kind(index) == Some(Kind::Ident)
    }

    pub(crate) fn is_keyword(&self, index: usize) -> bool {
        self.ident(index) && keyword(self.text(index))
    }

    /// An identifier that can bind a name. Keywords and type starters cannot.
    pub(crate) fn can_be_name(&self, index: usize) -> bool {
        self.ident(index) && !keyword(self.text(index))
    }

    pub(crate) fn type_starts(&self, index: usize) -> bool {
        self.ident(index) || matches!(self.text(index), "*" | "[" | "<-")
    }

    pub(crate) fn start_byte(&self, index: usize) -> u32 {
        self.toks.get(index).map_or(self.src.len() as u32, |token| token.start)
    }

    pub(crate) fn end_byte(&self, index: usize) -> u32 {
        self.toks.get(index).map_or(self.src.len() as u32, |token| token.end)
    }

    pub(crate) fn line(&self, byte: u32) -> u32 {
        self.lexed.line(byte)
    }

    /// Partner of an opener, or the last in-range token when it is unmatched.
    pub(crate) fn close_of(&self, index: usize, end: usize) -> usize {
        match self.partner.get(index).copied() {
            Some(partner) if partner != NONE && (partner as usize) > index && (partner as usize) < end => {
                partner as usize
            }
            _ => end.saturating_sub(1).max(index),
        }
    }

    pub(crate) fn after(&self, index: usize) -> usize {
        match self.partner.get(index).copied() {
            Some(partner) if partner != NONE && (partner as usize) > index => partner as usize + 1,
            _ => index + 1,
        }
    }

    pub(crate) fn skip_semis(&self, mut index: usize, end: usize) -> usize {
        while index < end && self.is_semi(index) {
            index += 1;
        }
        index
    }

    pub(crate) fn consume_semi(&self, index: usize, end: usize) -> usize {
        let next = index + 1;
        if next < end && self.is_semi(next) {
            next + 1
        } else {
            next.min(end)
        }
    }

    pub(crate) fn problem(&mut self, byte: u32, reason: DiagnosticReason) {
        self.draft.problem(byte, reason);
    }

    pub(crate) fn doc_before(&self, token: usize) -> Option<String> {
        let start = self.toks.get(token)?.start;
        let prev_end = token.checked_sub(1).map(|index| self.toks[index].end).unwrap_or(0);
        let mut limit = start;
        let mut group = Vec::new();
        for &(from, to) in self.lexed.comments.iter().rev() {
            if to > start {
                continue;
            }
            if from < prev_end || !gap_ok(self.src, to, limit) {
                break;
            }
            group.push((from, to));
            limit = from;
        }
        if group.is_empty() {
            return None;
        }
        group.reverse();
        let text = group
            .iter()
            .map(|&(from, to)| comment_body(self.src, from, to))
            .collect::<Vec<_>>()
            .join(" ");
        let text = sentence(&text);
        (!text.is_empty()).then_some(text)
    }

    /// Emits a declaration and returns the scope its contents belong to.
    pub(crate) fn item(&mut self, item: Item, scope: &Scope) -> Scope {
        if self.draft.full() {
            return scope.clone();
        }
        let name = if scope.name.is_empty() {
            item.local.clone()
        } else {
            format!("{}::{}", scope.name, item.local)
        };
        if name.is_empty() || name.contains('~') {
            self.problem(self.start_byte(item.first), DiagnosticReason::UnsupportedName);
            return scope.clone();
        }
        let path = self.draft.path.clone();
        let id = if item.node_kind == NodeKind::Package {
            StableId::new(NodeKind::Package, &path, None, None)
        } else {
            StableId::new(item.node_kind, &path, Some(&name), None)
        };
        let Ok(id) = id else {
            self.problem(self.start_byte(item.first), DiagnosticReason::UnsupportedName);
            return scope.clone();
        };
        let start = self.start_byte(item.first);
        let end = self.end_byte(item.last).max(start);
        let sig_end = item.sig_end.clamp(start, end) as usize;
        let signature = normalize(self.src.get(start as usize..sig_end).unwrap_or(""));
        let summary = match item.doc_at.and_then(|at| self.doc_before(at)) {
            Some(text) => Summary {
                text,
                source: SummarySource::Doc,
            },
            None => Summary {
                text: signature.clone(),
                source: SummarySource::Signature,
            },
        };
        if item.node_kind == NodeKind::Package
            && self.draft.nodes[0].node.summary.text.is_empty()
            && summary.source == SummarySource::Doc
        {
            self.draft.nodes[0].node.summary = summary.clone();
        }
        let body = self.src.get(item.body.0 as usize..item.body.1 as usize).unwrap_or("");
        let lines = [self.line(start), self.line(end.saturating_sub(1).max(start))];
        let node = Node {
            id,
            kind: item.node_kind,
            symbol_kind: item.symbol,
            path,
            lines,
            name: name.clone(),
            signature,
            fingerprint: body_fingerprint(body),
            public: exported(&item.bare),
            summary,
        };
        match self.draft.node(node, scope.owner, (start, end), Precision::Structural) {
            Some(owner) => Scope { owner, name },
            None => scope.clone(),
        }
    }

    /// Emits one reference. `span` is an inclusive token range.
    pub(crate) fn refer(
        &mut self,
        src: usize,
        scope: &str,
        kind: EdgeKind,
        name: String,
        alias: Option<String>,
        span: (usize, usize),
    ) {
        if name.is_empty() || self.draft.full() {
            return;
        }
        let (first, last) = span;
        let start = self.start_byte(first);
        let end = self.end_byte(last).max(start);
        let line0 = self.line(start);
        let line1 = self.line(end.saturating_sub(1).max(start)).max(line0);
        self.draft.reference(DraftRef {
            src,
            kind,
            name,
            scope: scope.to_string(),
            alias,
            lines: [line0, line1],
            start,
            end,
            precision: Precision::Structural,
        });
    }
}

fn exported(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

pub(crate) fn compact(value: &str) -> String {
    value.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn normalize(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn sentence(value: &str) -> String {
    let value = normalize(value);
    match value.find(". ") {
        Some(end) => value[..=end].to_string(),
        None => value,
    }
}

fn comment_body(src: &str, start: u32, end: u32) -> String {
    let Some(text) = src.get(start as usize..end as usize) else {
        return String::new();
    };
    if let Some(rest) = text.strip_prefix("//") {
        return rest.trim().to_string();
    }
    if let Some(inner) = text.strip_prefix("/*").and_then(|body| body.strip_suffix("*/")) {
        return inner
            .lines()
            .map(|line| line.trim().trim_start_matches('*').trim())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
    }
    text.trim().to_string()
}

/// At most one newline, and no other non-space, between a doc comment and its declaration.
fn gap_ok(src: &str, from: u32, to: u32) -> bool {
    if from > to {
        return false;
    }
    let Some(gap) = src.get(from as usize..to as usize) else {
        return false;
    };
    gap.bytes().filter(|&byte| byte == b'\n').count() <= 1
        && gap.bytes().all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

struct DraftNode {
    node: Node,
    parent: usize,
    start: u32,
    end: u32,
    precision: Precision,
}

struct DraftRef {
    src: usize,
    kind: EdgeKind,
    name: String,
    scope: String,
    alias: Option<String>,
    lines: [u32; 2],
    start: u32,
    end: u32,
    precision: Precision,
}

struct Draft {
    path: String,
    nodes: Vec<DraftNode>,
    refs: Vec<DraftRef>,
    problems: Vec<(u32, DiagnosticReason)>,
    max_facts: usize,
    overflow: bool,
}

impl Draft {
    fn new(path: &str, source: &str, max_facts: usize) -> Result<Self, LexError> {
        let id = StableId::new(NodeKind::Module, path, None, None)?;
        let module = Node {
            id,
            kind: NodeKind::Module,
            symbol_kind: None,
            path: path.to_string(),
            lines: [1, source.lines().count().max(1) as u32],
            name: path.to_string(),
            signature: String::new(),
            fingerprint: body_fingerprint(source),
            public: false,
            summary: Summary {
                text: String::new(),
                source: SummarySource::Doc,
            },
        };
        Ok(Self {
            path: path.to_string(),
            nodes: vec![DraftNode {
                node: module,
                parent: 0,
                start: 0,
                end: u32::MAX,
                precision: Precision::Structural,
            }],
            refs: Vec::new(),
            problems: Vec::new(),
            max_facts,
            overflow: false,
        })
    }

    fn full(&mut self) -> bool {
        if self.nodes.len() + self.refs.len() >= self.max_facts {
            self.overflow = true;
            true
        } else {
            false
        }
    }

    fn node(
        &mut self,
        node: Node,
        parent: usize,
        (start, end): (u32, u32),
        precision: Precision,
    ) -> Option<usize> {
        if self.full() {
            return None;
        }
        self.nodes.push(DraftNode {
            node,
            parent,
            start,
            end,
            precision,
        });
        Some(self.nodes.len() - 1)
    }

    fn reference(&mut self, reference: DraftRef) {
        if !self.full() {
            self.refs.push(reference);
        }
    }

    fn problem(&mut self, byte: u32, reason: DiagnosticReason) {
        self.problems.push((byte, reason));
    }

    fn finish(mut self, lexed: &Lexed, dialect: Dialect, max_facts: usize) -> Result<LexedFile, LexError> {
        if self.overflow {
            return Err(LexError::TooManyFacts { limit: max_facts });
        }
        let mut order: Vec<usize> = (1..self.nodes.len()).collect();
        order.sort_by_key(|&index| (self.nodes[index].start, Reverse(self.nodes[index].end)));
        let mut remap = vec![0; self.nodes.len()];
        for (position, &old) in order.iter().enumerate() {
            remap[old] = position + 1;
        }
        let mut slots: Vec<Option<DraftNode>> = self.nodes.drain(..).map(Some).collect();
        let mut nodes = Vec::with_capacity(slots.len());
        nodes.extend(slots.first_mut().and_then(Option::take));
        for &old in &order {
            nodes.extend(slots.get_mut(old).and_then(Option::take));
        }
        for node in nodes.iter_mut().skip(1) {
            node.parent = remap.get(node.parent).copied().unwrap_or(0);
        }
        assign_ordinals(&mut nodes)?;
        self.refs.sort_by_key(|reference| (reference.start, Reverse(reference.end)));
        let mut diagnostics = lexed.diagnostics.clone();
        diagnostics.extend(self.problems.iter().map(|&(byte, reason)| Diagnostic {
            line: lexed.line(byte),
            byte,
            reason,
        }));
        diagnostics.sort_by_key(|diagnostic| (diagnostic.byte, diagnostic.reason as u8));
        let first = diagnostics.first().map(|diagnostic| diagnostic.byte).unwrap_or(u32::MAX);
        let completion = if diagnostics.is_empty() {
            Completion::Complete
        } else {
            Completion::Partial
        };
        diagnostics.truncate(LexedFile::MAX_DIAGNOSTICS);
        let label = |end: u32, base: Precision| {
            if end != u32::MAX && end > first {
                Precision::Partial
            } else {
                base
            }
        };
        let path = self.path.clone();
        let edges = nodes
            .iter()
            .skip(1)
            .map(|child| Labeled {
                fact: Edge {
                    src: nodes[child.parent].node.id.clone(),
                    dst: child.node.id.clone(),
                    kind: EdgeKind::Contains,
                    conf: Confidence::Exact,
                    owner_path: path.clone(),
                },
                precision: label(child.end, child.precision),
            })
            .collect();
        let references = self
            .refs
            .into_iter()
            .filter(|reference| reference.src < remap.len())
            .map(|reference| Labeled {
                precision: label(reference.end, reference.precision),
                fact: RawReference {
                    src: nodes[remap[reference.src]].node.id.clone(),
                    kind: reference.kind,
                    name: reference.name,
                    scope: reference.scope,
                    alias: reference.alias,
                    lines: reference.lines,
                    owner_path: path.clone(),
                },
            })
            .collect();
        let nodes = nodes
            .into_iter()
            .map(|draft| Labeled {
                precision: label(draft.end, draft.precision),
                fact: draft.node,
            })
            .collect();
        Ok(LexedFile {
            path,
            dialect,
            completion,
            diagnostics,
            nodes,
            edges,
            references,
        })
    }
}

fn assign_ordinals(nodes: &mut [DraftNode]) -> Result<(), LexError> {
    let mut counts = BTreeMap::new();
    for draft in nodes.iter().skip(1).filter(|draft| named(draft.node.kind)) {
        *counts.entry((draft.node.kind, draft.node.name.clone())).or_insert(0_u32) += 1;
    }
    let mut ordinals = BTreeMap::new();
    for draft in nodes.iter_mut().skip(1).filter(|draft| named(draft.node.kind)) {
        let key = (draft.node.kind, draft.node.name.clone());
        let next = ordinals.entry(key.clone()).or_insert(0_u32);
        *next += 1;
        let ordinal = (counts[&key] > 1).then_some(*next);
        draft.node.id = StableId::new(draft.node.kind, &draft.node.path, Some(&draft.node.name), ordinal)?;
    }
    Ok(())
}

fn named(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Symbol | NodeKind::Test | NodeKind::Doc)
}

#[cfg(test)]
mod tests {
    use crate::GoLexer;

    #[test]
    fn duplicate_functions_receive_ordinals() {
        let facts = GoLexer::default()
            .extract("p.go", b"package p\nfunc F() {}\nfunc F() {}\n")
            .unwrap();
        let ids: Vec<_> = facts.nodes.iter().map(|node| node.fact.id.as_str()).collect();
        assert!(ids.contains(&"s:p.go#F~1"), "{ids:?}");
        assert!(ids.contains(&"s:p.go#F~2"), "{ids:?}");
        assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    }
}
