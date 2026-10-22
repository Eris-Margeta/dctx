//! Structural walker: scopes, fact emission and source order.
//! Implements SPEC-MAP-MODE 5.1–5.4 for Kotlin. A declaration owns the spans
//! lexically inside it. Callers use [`extract`].
//! Declaration, type, member and reference scans live in sibling modules.
mod declarations;
mod declarations_types;
mod members;
mod references;

use crate::domain::{
    body_fingerprint, kdoc_summary, Completion, Diagnostic, DiagnosticReason, Dialect, Labeled,
    LexError, LexedFile, Limits, Precision, RawReference,
};
use crate::lexer::{keyword, Kind, Lexed, Token, NONE};
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource, SymbolKind,
};
use std::cmp::Reverse;
use std::collections::BTreeMap;

/// Extracts facts from an already-lexed Kotlin file.
pub(crate) fn extract(
    path: &str,
    source: &str,
    lexed: &Lexed,
    limits: &Limits,
) -> Result<LexedFile, LexError> {
    let draft = Draft::new(path, source, lexed, limits.max_facts)?;
    let mut walker = Walker { src: source, toks: &lexed.tokens, partner: &lexed.partner, lexed, draft, depth: 0 };
    walker.file();
    walker.draft.finish(lexed)
}

/// Lexical owner of nested facts. `name` is `::`-qualified and empty at the root.
#[derive(Clone)]
pub(crate) struct Scope {
    pub owner: usize,
    pub name: String,
    pub simple: String,
}

impl Scope {
    fn root() -> Self {
        Self { owner: 0, name: String::new(), simple: String::new() }
    }
}

/// Modifiers and annotations in front of one declaration.
#[allow(clippy::struct_excessive_bools, reason = "Retain independent lexical modifier facts, including combinations in malformed Kotlin input; this prefix scanner does not validate their legality.")]
pub(crate) struct Prefix {
    pub next: usize,
    pub public: bool,
    pub data: bool,
    pub enum_kind: bool,
    pub annotation: bool,
    pub anns: Vec<Ann>,
}

/// One annotation use, including an optional argument list.
#[derive(Clone)]
pub(crate) struct Ann {
    pub name: String,
    pub at: usize,
    pub to: usize,
    pub args: Option<usize>,
}

/// Fields of one symbol about to be emitted.
pub(crate) struct Symbol {
    pub kind: SymbolKind,
    pub local: String,
    pub first: usize,
    pub last: usize,
    pub sig_end: u32,
    pub body: (u32, u32),
    pub is_public: bool,
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

pub(crate) struct Walker<'a> {
    src: &'a str,
    toks: &'a [Token],
    partner: &'a [u32],
    lexed: &'a Lexed,
    draft: Draft,
    depth: u32,
}

impl Draft {
    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    fn new(path: &str, source: &str, lexed: &Lexed, max_facts: usize) -> Result<Self, LexError> {
        let id = StableId::new(NodeKind::Module, path, None, None)?;
        let lines = source.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
        let summary = match leading_kdoc(source, lexed) {
            Some(text) => Summary { text, source: SummarySource::Doc },
            None => Summary { text: String::new(), source: SummarySource::Signature },
        };
        let module = Node {
            id,
            kind: NodeKind::Module,
            symbol_kind: None,
            path: path.into(),
            lines: [1, lines],
            name: path.into(),
            signature: String::new(),
            fingerprint: body_fingerprint(source),
            public: false,
            summary,
        };
        Ok(Self {
            path: path.into(),
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
            overflow: max_facts == 0,
        })
    }
    fn set_package(&mut self, name: &str) {
        let Some(module) = self.nodes.first_mut() else { return };
        if !module.node.signature.is_empty() {
            return;
        }
        module.node.signature = name.to_string();
        if module.node.summary.source == SummarySource::Signature {
            module.node.summary.text = name.to_string();
        }
    }
    fn accept(&mut self) -> bool {
        if self.overflow || self.nodes.len() + self.refs.len() >= self.max_facts {
            self.overflow = true;
            false
        } else {
            true
        }
    }
    fn finish(mut self, lexed: &Lexed) -> Result<LexedFile, LexError> {
        if self.overflow {
            return Err(LexError::TooManyFacts { limit: self.max_facts });
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
        diagnostics.sort_by_key(|diagnostic| diagnostic.byte);
        let first_problem = diagnostics.first().map_or(u32::MAX, |item| item.byte);
        let completion = if diagnostics.is_empty() { Completion::Complete } else { Completion::Partial };
        diagnostics.truncate(LexedFile::MAX_DIAGNOSTICS);
        let path = self.path.clone();
        let label = |end: u32, base: Precision| {
            if end > first_problem { Precision::Partial } else { base }
        };
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
            .map(|reference| Labeled {
                precision: label(reference.end, reference.precision),
                fact: RawReference {
                    src: nodes[remap.get(reference.src).copied().unwrap_or(0)].node.id.clone(),
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
            .map(|node| Labeled {
                precision: if node.end == u32::MAX { node.precision } else { label(node.end, node.precision) },
                fact: node.node,
            })
            .collect();
        Ok(LexedFile { path, dialect: Dialect::Kotlin, completion, diagnostics, nodes, edges, references })
    }
}

fn assign_ordinals(nodes: &mut [DraftNode]) -> Result<(), LexError> {
    let mut counts = BTreeMap::<(NodeKind, String), u32>::new();
    for draft in nodes.iter().skip(1) {
        if matches!(draft.node.kind, NodeKind::Symbol | NodeKind::Test | NodeKind::Doc) {
            *counts.entry((draft.node.kind, draft.node.name.clone())).or_insert(0) += 1;
        }
    }
    let mut ordinals = BTreeMap::<(NodeKind, String), u32>::new();
    for draft in nodes.iter_mut().skip(1) {
        let node = &mut draft.node;
        if !matches!(node.kind, NodeKind::Symbol | NodeKind::Test | NodeKind::Doc) {
            continue;
        }
        let key = (node.kind, node.name.clone());
        let next = ordinals.entry(key.clone()).or_insert(0);
        *next += 1;
        let ordinal = counts.get(&key).is_some_and(|count| *count > 1).then_some(*next);
        node.id = StableId::new(node.kind, &node.path, Some(&node.name), ordinal)?;
    }
    Ok(())
}

fn leading_kdoc(source: &str, lexed: &Lexed) -> Option<String> {
    let comment = lexed.comments.iter().find(|comment| comment.javadoc)?;
    let before = source.get(..comment.start as usize)?.trim_start_matches('\u{feff}');
    if !before.chars().all(char::is_whitespace) {
        return None;
    }
    kdoc_summary(source.get(comment.start as usize..comment.end as usize)?)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['#', '~']) && !name.chars().any(char::is_control)
}

const MODIFIERS: &[&str] = &[
    "abstract", "actual", "annotation", "companion", "const", "crossinline", "data", "enum",
    "expect", "external", "final", "infix", "inline", "inner", "internal", "lateinit",
    "noinline", "open", "operator", "override", "private", "protected", "public", "sealed",
    "suspend", "tailrec", "value", "vararg",
];

const DECLS: &[&str] = &[
    "class", "constructor", "fun", "import", "interface", "object", "package", "typealias", "val",
    "var",
];

fn is_modifier(word: &str) -> bool {
    MODIFIERS.binary_search(&word).is_ok()
}

fn decl_word(word: &str) -> bool {
    DECLS.binary_search(&word).is_ok()
}

impl<'a> Walker<'a> {
    fn file(&mut self) {
        let end = self.toks.len();
        let scope = Scope::root();
        let mut index = 0usize;
        let mut guard = 0usize;
        while index < end && guard <= end {
            guard += 1;
            let next = self.declaration(index, end, &scope, true);
            index = if next > index { next.min(end) } else { index + 1 };
        }
    }
    pub(crate) fn text(&self, index: usize) -> &'a str {
        self.toks.get(index).map_or("", |token| self.bytes(token.start, token.end))
    }
    pub(crate) fn kind(&self, index: usize) -> Option<Kind> {
        self.toks.get(index).map(|token| token.kind)
    }
    pub(crate) fn is(&self, index: usize, spelling: &str) -> bool {
        self.text(index) == spelling
    }
    pub(crate) fn ident(&self, index: usize) -> bool {
        self.kind(index) == Some(Kind::Ident)
    }
    pub(crate) fn is_decl_name(&self, index: usize) -> bool {
        let text = self.text(index);
        self.ident(index) && (text.starts_with('`') || !keyword(text))
    }
    pub(crate) fn call_start(&self, index: usize) -> bool {
        let text = self.text(index);
        self.ident(index) && (text.starts_with('`') || text == "this" || text == "super" || !keyword(text))
    }
    pub(crate) fn decl_at(&self, index: usize) -> bool {
        self.ident(index) && decl_word(self.text(index))
    }
    pub(crate) fn bare(&self, index: usize) -> String {
        let text = self.text(index);
        text.strip_prefix('`').and_then(|rest| rest.strip_suffix('`')).unwrap_or(text).to_string()
    }
    pub(crate) fn start_byte(&self, index: usize) -> u32 {
        self.toks.get(index).map_or(0, |token| token.start)
    }
    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    pub(crate) fn end_byte(&self, index: usize) -> u32 {
        self.toks.get(index).map_or(self.src.len() as u32, |token| token.end)
    }
    pub(crate) fn line_of(&self, byte: u32) -> u32 {
        self.lexed.line(byte).max(1)
    }
    pub(crate) fn bytes(&self, start: u32, end: u32) -> &'a str {
        let start = start as usize;
        let end = (end as usize).clamp(start, self.src.len());
        self.src.get(start..end).unwrap_or("")
    }
    pub(crate) fn matched(&self, index: usize, end: usize) -> Option<usize> {
        let partner = self.partner.get(index).copied()?;
        (partner != NONE && (partner as usize) > index && (partner as usize) < end).then_some(partner as usize)
    }
    pub(crate) fn after(&self, index: usize) -> usize {
        self.matched(index, usize::MAX).map_or(index + 1, |partner| partner + 1)
    }
    pub(crate) fn is_semi(&self, index: usize) -> bool {
        self.kind(index) == Some(Kind::Semi)
    }
    pub(crate) fn is_inserted_semi(&self, index: usize) -> bool {
        self.toks.get(index).is_some_and(|token| token.kind == Kind::Semi && token.start == token.end)
    }
    pub(crate) fn is_explicit_semi(&self, index: usize) -> bool {
        self.toks.get(index).is_some_and(|token| token.kind == Kind::Semi && token.start != token.end)
    }
    pub(crate) fn skip_semis(&self, mut index: usize, end: usize) -> usize {
        while index < end && self.is_semi(index) {
            index += 1;
        }
        index
    }
    pub(crate) fn finish_stmt(&self, index: usize, end: usize) -> usize {
        if self.is_semi(index) { (index + 1).min(end) } else { index.min(end) }
    }
    fn span_start(&self, index: usize) -> u32 {
        let Some(token) = self.toks.get(index) else { return 0 };
        if token.doc > 0 && let Some(comment) = self.lexed.comments.get((token.doc - 1) as usize) {
            return comment.start;
        }
        token.start
    }
    fn doc_text(&self, index: usize) -> Option<String> {
        let token = self.toks.get(index)?;
        if token.doc == 0 {
            return None;
        }
        let comment = self.lexed.comments.get((token.doc - 1) as usize)?;
        kdoc_summary(self.bytes(comment.start, comment.end))
    }
    pub(crate) fn problem(&mut self, byte: u32, reason: DiagnosticReason) {
        self.draft.problems.push((byte, reason));
    }
    pub(crate) fn emit(&mut self, symbol: Symbol, parent: &Scope) -> Option<Scope> {
        if self.draft.overflow {
            return None;
        }
        let qualified = if parent.name.is_empty() {
            symbol.local.clone()
        } else {
            format!("{}::{}", parent.name, symbol.local)
        };
        if !valid_name(&qualified) {
            self.problem(self.start_byte(symbol.first), DiagnosticReason::UnsupportedName);
            return None;
        }
        let Ok(id) = StableId::new(NodeKind::Symbol, &self.draft.path, Some(&qualified), None) else {
            self.problem(self.start_byte(symbol.first), DiagnosticReason::UnsupportedName);
            return None;
        };
        if !self.draft.accept() {
            return None;
        }
        let signature = crate::domain::normalize(self.bytes(self.start_byte(symbol.first), symbol.sig_end));
        let summary = match self.doc_text(symbol.first) {
            Some(text) => Summary { text, source: SummarySource::Doc },
            None => Summary { text: signature.clone(), source: SummarySource::Signature },
        };
        let start = self.span_start(symbol.first);
        let end = self.end_byte(symbol.last).max(start);
        let line0 = self.line_of(start);
        let line1 = self.line_of(end.saturating_sub(1).max(start)).max(line0);
        let node = Node {
            id,
            kind: NodeKind::Symbol,
            symbol_kind: Some(symbol.kind),
            path: self.draft.path.clone(),
            lines: [line0, line1],
            name: qualified.clone(),
            signature,
            fingerprint: body_fingerprint(self.bytes(symbol.body.0, symbol.body.1)),
            public: symbol.is_public,
            summary,
        };
        self.draft.nodes.push(DraftNode {
            node,
            parent: parent.owner,
            start,
            end,
            precision: Precision::Structural,
        });
        Some(Scope { owner: self.draft.nodes.len() - 1, name: qualified, simple: symbol.local })
    }
    pub(crate) fn refer(
        &mut self,
        scope: &Scope,
        kind: EdgeKind,
        name: String,
        alias: Option<String>,
        from: usize,
        to: usize,
    ) {
        if name.is_empty() || self.draft.overflow || !self.draft.accept() {
            return;
        }
        let to = to.max(from);
        let start = self.start_byte(from);
        let end = self.end_byte(to).max(start);
        let line0 = self.line_of(start);
        let line1 = self.line_of(end.saturating_sub(1).max(start)).max(line0);
        self.draft.refs.push(DraftRef {
            src: scope.owner,
            kind,
            name,
            scope: scope.name.clone(),
            alias,
            lines: [line0, line1],
            start,
            end,
            precision: Precision::Structural,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn word_tables_are_sorted() {
        assert!(MODIFIERS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(DECLS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(is_modifier("public") && !is_modifier("fun") && decl_word("typealias"));
        assert!(valid_name("Box::item") && !valid_name("a~b") && !valid_name("a#b"));
    }
}
