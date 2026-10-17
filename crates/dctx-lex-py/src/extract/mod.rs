//! Structural walker: scopes, fact assembly and the per-file entry.
//! Implements SPEC-MAP-MODE 5.1–5.4 for Python. A declaration owns the spans lexically
//! inside it. Declarations, imports, references and docstrings are sibling modules.
mod declarations;
mod docstrings;
mod imports;
mod references;

use references::is_keyword;

use crate::domain::{
    Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexError, LexedFile, Limits,
    Precision, RawReference, body_fingerprint, comment_summary,
};
use crate::lexer::{Kind, Lexed, Token};
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource,
};
use std::cmp::Reverse;
use std::collections::BTreeMap;

#[derive(Clone)]
struct Scope {
    owner: usize,
    name: String,
    class_body: bool,
    module_top: bool,
    in_match: bool,
}

impl Scope {
    fn blocked(&self) -> Self {
        Self {
            module_top: false,
            in_match: false,
            ..self.clone()
        }
    }
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
    nodes: Vec<DraftNode>,
    refs: Vec<DraftRef>,
}

struct Walker<'a> {
    src: &'a str,
    toks: &'a [Token],
    lines: &'a [u32],
    i: usize,
    draft: Draft,
    depth: u32,
    max_depth: u32,
    max_facts: usize,
    refused: Option<LexError>,
    problems: Vec<(u32, DiagnosticReason)>,
    path: String,
}

pub(crate) fn extract(
    path: &str,
    source: &str,
    dialect: Dialect,
    lexed: &Lexed,
    limits: &Limits,
) -> Result<LexedFile, LexError> {
    let _ = StableId::new(NodeKind::Module, path, None, None)?;
    let mut walker = Walker::new(path, source, lexed, limits)?;
    let module_doc = walker.lookahead_module_doc();
    let root = Scope {
        owner: 0,
        name: String::new(),
        class_body: false,
        module_top: true,
        in_match: false,
    };
    while walker.i < walker.toks.len() && walker.refused.is_none() {
        if walker.at(Kind::Newline) {
            walker.bump();
            continue;
        }
        if walker.at(Kind::Dedent) {
            walker.problem_here(DiagnosticReason::InconsistentIndent);
            walker.bump();
            continue;
        }
        if walker.at(Kind::Indent) {
            walker.unexpected_indent(&root);
            continue;
        }
        let mark = walker.i;
        walker.statement(&root);
        if walker.i == mark {
            walker.bump();
        }
    }
    if let Some(text) = module_doc {
        walker.set_module_summary(text);
    }
    walker.finish(dialect, lexed)
}

impl<'a> Walker<'a> {
    fn new(
        path: &str,
        source: &'a str,
        lexed: &'a Lexed,
        limits: &Limits,
    ) -> Result<Self, LexError> {
        if limits.max_facts == 0 {
            return Err(LexError::TooManyFacts { limit: 0 });
        }
        let id = StableId::new(NodeKind::Module, path, None, None)?;
        let summary = comment_summary(source, &lexed.comments);
        let end_byte = source.len() as u32;
        let last = if source.is_empty() {
            1
        } else {
            lexed.line(end_byte.saturating_sub(1))
        };
        let module = Node {
            id,
            kind: NodeKind::Module,
            symbol_kind: None,
            path: path.to_owned(),
            lines: [1, last],
            name: path.to_owned(),
            signature: String::new(),
            fingerprint: body_fingerprint(source),
            public: false,
            summary: Summary {
                text: summary,
                source: SummarySource::Doc,
            },
        };
        Ok(Self {
            src: source,
            toks: &lexed.tokens,
            lines: &lexed.line_starts,
            i: 0,
            draft: Draft {
                nodes: vec![DraftNode {
                    node: module,
                    parent: 0,
                    start: 0,
                    end: u32::MAX,
                    precision: Precision::Structural,
                }],
                refs: Vec::new(),
            },
            depth: 0,
            max_depth: limits.max_depth,
            max_facts: limits.max_facts,
            refused: None,
            problems: Vec::new(),
            path: path.to_owned(),
        })
    }
}

impl Walker<'_> {
    fn finish(mut self, dialect: Dialect, lexed: &Lexed) -> Result<LexedFile, LexError> {
        if let Some(error) = self.refused.take() {
            return Err(error);
        }
        let mut order: Vec<usize> = (1..self.draft.nodes.len()).collect();
        order.sort_by_key(|&index| {
            (
                self.draft.nodes[index].start,
                Reverse(self.draft.nodes[index].end),
            )
        });
        let mut remap = vec![0; self.draft.nodes.len()];
        for (position, &old) in order.iter().enumerate() {
            remap[old] = position + 1;
        }
        let mut slots: Vec<Option<DraftNode>> = self.draft.nodes.drain(..).map(Some).collect();
        let mut nodes = Vec::with_capacity(slots.len());
        if let Some(module) = slots.first_mut() {
            nodes.extend(module.take());
        }
        for &old in &order {
            if let Some(slot) = slots.get_mut(old) {
                nodes.extend(slot.take());
            }
        }
        for node in nodes.iter_mut().skip(1) {
            node.parent = remap.get(node.parent).copied().unwrap_or(0);
        }
        assign_ordinals(&mut nodes, &self.path)?;
        self.draft.refs.sort_by_key(|reference| (reference.start, Reverse(reference.end)));
        let mut diagnostics = lexed.diagnostics.clone();
        diagnostics.extend(self.problems.iter().map(|&(byte, reason)| Diagnostic {
            line: lexed.line(byte),
            byte,
            reason,
        }));
        diagnostics.sort_by_key(|diagnostic| diagnostic.byte);
        let first = diagnostics.first().map_or(u32::MAX, |diagnostic| diagnostic.byte);
        let path = self.path.clone();
        let edges = nodes
            .iter()
            .skip(1)
            .map(|child| Labeled {
                fact: Edge {
                    src: nodes
                        .get(child.parent)
                        .map(|parent| parent.node.id.clone())
                        .unwrap_or_else(|| child.node.id.clone()),
                    dst: child.node.id.clone(),
                    kind: EdgeKind::Contains,
                    conf: Confidence::Exact,
                    owner_path: path.clone(),
                },
                precision: label(child.end, child.precision, first),
            })
            .collect();
        let references = self
            .draft
            .refs
            .into_iter()
            .filter_map(|reference| {
                let src = nodes.get(*remap.get(reference.src)?)?.node.id.clone();
                Some(Labeled {
                    precision: label(reference.end, reference.precision, first),
                    fact: RawReference {
                        src,
                        kind: reference.kind,
                        name: reference.name,
                        scope: reference.scope,
                        alias: reference.alias,
                        lines: reference.lines,
                        owner_path: path.clone(),
                    },
                })
            })
            .collect();
        let completion = if diagnostics.is_empty() {
            Completion::Complete
        } else {
            Completion::Partial
        };
        diagnostics.truncate(LexedFile::MAX_DIAGNOSTICS);
        let nodes = nodes
            .into_iter()
            .map(|node| Labeled {
                precision: if node.end == u32::MAX {
                    node.precision
                } else {
                    label(node.end, node.precision, first)
                },
                fact: node.node,
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

    fn sync_line(&mut self) {
        let mut depth = 0i32;
        while self.i < self.toks.len() {
            if depth <= 0 && (self.at(Kind::Newline) || self.at(Kind::Dedent)) {
                if self.at(Kind::Newline) {
                    self.bump();
                }
                return;
            }
            depth += self.delta(self.i);
            if !self.bump() {
                return;
            }
        }
    }

    fn refuse_depth(&mut self) {
        let line = self.line_of(self.tok_start(self.i));
        self.refused = Some(LexError::TooDeep {
            line,
            limit: self.max_depth,
        });
        self.i = self.toks.len();
    }

    fn problem_here(&mut self, reason: DiagnosticReason) {
        if self.problems.len() >= 128 {
            return;
        }
        let at = if self.toks.is_empty() {
            0
        } else {
            self.tok_start(self.i.min(self.toks.len() - 1))
        };
        self.problems.push((at, reason));
    }

    fn span_bytes(&self, lo: usize, hi: usize) -> (u32, u32) {
        if lo >= hi || lo >= self.toks.len() {
            let at = self.tok_start(lo.min(self.toks.len().saturating_sub(1)));
            return (at, at);
        }
        let end_i = (hi - 1).min(self.toks.len() - 1);
        (self.tok_start(lo), self.tok_end(end_i))
    }

    fn delta(&self, index: usize) -> i32 {
        match self.op_at(index) {
            Some("(" | "[" | "{") => 1,
            Some(")" | "]" | "}") => -1,
            _ => 0,
        }
    }

    fn line_of(&self, byte: u32) -> u32 {
        self.lines.partition_point(|&start| start <= byte).max(1) as u32
    }

    fn slice(&self, start: u32, end: u32) -> &str {
        let (start, end) = (start as usize, end as usize);
        if start <= end {
            self.src.get(start..end).unwrap_or("")
        } else {
            ""
        }
    }

    fn text(&self) -> &str {
        self.toks
            .get(self.i)
            .map(|token| self.token_text(token))
            .unwrap_or("")
    }

    fn token_text(&self, token: &Token) -> &str {
        self.src
            .get(token.start as usize..token.end as usize)
            .unwrap_or("")
    }

    fn text_at(&self, index: usize) -> &str {
        self.toks
            .get(index)
            .map(|token| self.token_text(token))
            .unwrap_or("")
    }

    fn kind_at(&self, index: usize) -> Option<Kind> {
        self.toks.get(index).map(|token| token.kind)
    }

    fn op_at(&self, index: usize) -> Option<&str> {
        let token = self.toks.get(index)?;
        if token.kind == Kind::Punct {
            Some(self.token_text(token))
        } else {
            None
        }
    }

    fn at(&self, kind: Kind) -> bool {
        self.kind_at(self.i) == Some(kind)
    }

    fn at_text(&self, text: &str) -> bool {
        self.at(Kind::Ident) && self.text() == text
    }

    fn at_name(&self) -> bool {
        self.at(Kind::Ident) && !self.text().is_empty() && !is_keyword(self.text())
    }

    fn at_op(&self, op: &str) -> bool {
        self.op_at(self.i) == Some(op)
    }

    fn peek_text(&self, text: &str) -> bool {
        self.kind_at(self.i + 1) == Some(Kind::Ident) && self.text_at(self.i + 1) == text
    }

    fn peek_name(&self) -> bool {
        self.kind_at(self.i + 1) == Some(Kind::Ident)
            && !is_keyword(self.text_at(self.i + 1))
            && !self.text_at(self.i + 1).is_empty()
    }

    fn eat_text(&mut self, text: &str) -> bool {
        if self.at_text(text) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_op(&mut self, op: &str) -> bool {
        if self.at_op(op) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn bump(&mut self) -> bool {
        if self.i < self.toks.len() {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn eof(&self) -> bool {
        self.i >= self.toks.len()
    }

    fn tok_start(&self, index: usize) -> u32 {
        self.toks
            .get(index)
            .map_or(self.src.len() as u32, |token| token.start)
    }

    fn tok_end(&self, index: usize) -> u32 {
        self.toks
            .get(index)
            .map_or(self.src.len() as u32, |token| token.end)
    }
}

fn qualify(scope: &Scope, local: &str) -> String {
    if scope.name.is_empty() {
        local.to_owned()
    } else {
        format!("{}::{local}", scope.name)
    }
}

fn label(end: u32, base: Precision, first: u32) -> Precision {
    if end > first {
        Precision::Partial
    } else {
        base
    }
}

fn assign_ordinals(nodes: &mut [DraftNode], path: &str) -> Result<(), LexError> {
    let mut counts = BTreeMap::new();
    for draft in nodes.iter().skip(1) {
        *counts
            .entry((draft.node.kind, draft.node.name.clone()))
            .or_insert(0u32) += 1;
    }
    let mut ordinals = BTreeMap::new();
    for draft in nodes.iter_mut().skip(1) {
        let node = &mut draft.node;
        let key = (node.kind, node.name.clone());
        let next = ordinals.entry(key.clone()).or_insert(0u32);
        *next += 1;
        let ordinal = (counts.get(&key).copied().unwrap_or(0) > 1).then_some(*next);
        node.id = StableId::new(node.kind, path, Some(&node.name), ordinal)?;
    }
    Ok(())
}
