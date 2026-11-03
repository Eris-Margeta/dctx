// SPDX-License-Identifier: Apache-2.0
//! Structural walker: scopes, fact assembly and the per-file entry.
//! Implements SPEC-MAP-MODE 5.1–5.4 for Ruby. A declaration owns the spans lexically
//! inside it. Declarations, attributes and references are sibling modules.
mod declarations;
mod members;
mod references;

use crate::domain::{
    body_fingerprint, comment_summary, DiagnosticReason, LexError, LexedFile, Limits, Precision,
};
use crate::lexer::{Lexed, Token};
use dctx_map_model::{EdgeKind, Node, NodeKind, StableId, Summary, SummarySource, SymbolKind};
use std::collections::BTreeMap;

/// Lexical owner of nested facts. `name` is `::`-qualified and empty at the file root.
#[derive(Clone)]
pub(super) struct Scope {
    /// Index of the owning draft node.
    pub owner: usize,
    /// Qualified name, empty at the file root.
    pub name: String,
    /// Class, module and file bodies extract `def`. Method bodies do not.
    pub defining: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Vis {
    Public,
    Private,
    Protected,
}

pub(super) struct Sym {
    pub name: String,
    pub symbol: SymbolKind,
    pub signature: String,
    pub start: u32,
    pub end: u32,
    pub public: bool,
    pub precision: Precision,
    pub doc: Option<String>,
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
    start: u32,
    end: u32,
    precision: Precision,
}

pub(super) struct OutRef<'a> {
    pub owner: usize,
    pub scope: &'a str,
    pub kind: EdgeKind,
    pub name: String,
    pub start: u32,
    pub end: u32,
    pub precision: Precision,
}

pub(super) struct Walker<'a> {
    src: &'a str,
    toks: &'a [Token],
    lex: &'a Lexed,
    i: usize,
    nodes: Vec<DraftNode>,
    refs: Vec<DraftRef>,
    facts: usize,
    max_facts: usize,
    depth: u32,
    max_depth: u32,
    refused: Option<LexError>,
    problems: Vec<(u32, DiagnosticReason)>,
    path: String,
    file_lines: u32,
    vis: Vis,
    modfn: bool,
    vis_stack: Vec<(Vis, bool)>,
}

/// Extracts facts from an already-lexed Ruby file.
pub(crate) fn extract(
    path: &str,
    source: &str,
    lexed: &Lexed,
    limits: &Limits,
) -> Result<LexedFile, LexError> {
    let _ = StableId::new(NodeKind::Module, path, None, None)?;
    let mut walker = Walker::open(path, source, lexed, limits)?;
    let root = Scope {
        owner: 0,
        name: String::new(),
        defining: true,
    };
    walker.until_end(&root, false);
    walker.finish(lexed)
}

impl<'a> Walker<'a> {
    fn open(
        path: &str,
        source: &'a str,
        lexed: &'a Lexed,
        limits: &Limits,
    ) -> Result<Self, LexError> {
        if limits.max_facts == 0 {
            return Err(LexError::TooManyFacts { limit: 0 });
        }
        let lines = file_lines(source);
        let (summary, source_kind) = file_summary(path, source, lexed);
        let module = Node {
            id: StableId::new(NodeKind::Module, path, None, None)?,
            kind: NodeKind::Module,
            symbol_kind: None,
            path: path.to_owned(),
            lines: [1, lines],
            name: path.to_owned(),
            signature: String::new(),
            fingerprint: body_fingerprint(source),
            public: false,
            summary: Summary {
                text: summary,
                source: source_kind,
            },
        };
        Ok(Self {
            src: source,
            toks: &lexed.tokens,
            lex: lexed,
            i: 0,
            nodes: vec![DraftNode {
                node: module,
                parent: 0,
                start: 0,
                end: u32::try_from(source.len()).unwrap_or(u32::MAX),
                precision: Precision::Structural,
            }],
            refs: Vec::new(),
            facts: 1,
            max_facts: limits.max_facts,
            depth: 0,
            max_depth: limits.max_depth,
            refused: None,
            problems: Vec::new(),
            path: path.to_owned(),
            file_lines: lines,
            vis: Vis::Public,
            modfn: false,
            vis_stack: Vec::new(),
        })
    }

    pub(super) fn until_end(&mut self, scope: &Scope, expect: bool) {
        loop {
            if self.refused.is_some() {
                return;
            }
            self.skip_sep();
            if self.eof() {
                if expect {
                    self.problem_here(DiagnosticReason::UnterminatedBlock);
                }
                return;
            }
            if self.at_word("end") && !self.prev_member() {
                if expect {
                    self.bump();
                    self.finish_block(scope);
                    return;
                }
                self.problem_here(DiagnosticReason::Malformed);
                self.bump();
                continue;
            }
            if self.mid_keyword() {
                let word = self.cur().to_owned();
                self.bump();
                if matches!(word.as_str(), "elsif" | "when" | "rescue" | "in") {
                    self.skip_header(scope);
                }
                continue;
            }
            let mark = self.i;
            self.statement(scope);
            if self.i == mark {
                self.bump();
            }
        }
    }

    fn statement(&mut self, scope: &Scope) {
        if self.refused.is_some() || self.eof() {
            return;
        }
        if scope.defining && self.modifier(scope) {
            return;
        }
        if scope.defining && self.declaration(scope) {
            return;
        }
        if self.import_stmt(scope) {
            return;
        }
        if scope.defining && self.attr_stmt(scope) {
            return;
        }
        if scope.defining && self.alias_stmt(scope) {
            return;
        }
        if scope.defining && self.const_stmt(scope) {
            return;
        }
        if self.block_word() {
            self.block_stmt(scope);
            return;
        }
        self.scan_statement(scope);
    }

    pub(super) fn block_stmt(&mut self, scope: &Scope) {
        let word = self.cur().to_owned();
        if !self.nest() {
            return;
        }
        self.bump();
        if matches!(word.as_str(), "if" | "unless" | "while" | "until" | "for" | "case") {
            self.skip_header(scope);
        }
        let inner = if word == "do" {
            Scope {
                defining: false,
                ..scope.clone()
            }
        } else {
            scope.clone()
        };
        self.until_end(&inner, true);
        self.unnest();
    }

    fn finish_block(&mut self, scope: &Scope) {
        // `end if cond`, `end while cond`, and `end rescue expr` belong to the block.
        if self.at_word("if")
            || self.at_word("unless")
            || self.at_word("while")
            || self.at_word("until")
            || self.at_word("rescue")
        {
            self.bump();
            self.scan_statement(scope);
        }
    }

    pub(super) fn nest(&mut self) -> bool {
        if self.depth >= self.max_depth {
            self.refused = Some(LexError::TooDeep {
                line: self.here_line(),
                limit: self.max_depth,
            });
            return false;
        }
        self.depth += 1;
        true
    }

    pub(super) fn unnest(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    pub(super) fn push_vis(&mut self) {
        self.vis_stack.push((self.vis, self.modfn));
        self.vis = Vis::Public;
        self.modfn = false;
    }

    pub(super) fn pop_vis(&mut self) {
        if let Some((vis, modfn)) = self.vis_stack.pop() {
            self.vis = vis;
            self.modfn = modfn;
        }
    }

    pub(super) fn method_public(&self, over: Option<Vis>, forced_modfn: bool) -> bool {
        if forced_modfn || self.modfn {
            return true;
        }
        matches!(over.unwrap_or(self.vis), Vis::Public)
    }

    pub(super) fn set_vis(&mut self, word: &str) {
        match word {
            "private" => self.vis = Vis::Private,
            "protected" => self.vis = Vis::Protected,
            "public" => self.vis = Vis::Public,
            "module_function" => self.modfn = true,
            _ => {}
        }
    }

    pub(super) fn emit_symbol(&mut self, scope: &Scope, sym: Sym) -> Option<usize> {
        if !name_ok(&sym.name) {
            self.problem(sym.start, DiagnosticReason::UnsupportedName);
            return None;
        }
        if !self.charge() {
            return None;
        }
        let summary = match sym.doc {
            Some(text) => Summary {
                text,
                source: SummarySource::Doc,
            },
            None => Summary {
                text: cap(&sym.signature),
                source: SummarySource::Signature,
            },
        };
        let node = Node {
            id: StableId::new(NodeKind::Symbol, &self.path, Some(&sym.name), None).ok()?,
            kind: NodeKind::Symbol,
            symbol_kind: Some(sym.symbol),
            path: self.path.clone(),
            lines: self.span_lines(sym.start, sym.end),
            name: sym.name,
            signature: sym.signature,
            fingerprint: body_fingerprint(self.slice(sym.start, sym.end)),
            public: sym.public,
            summary,
        };
        let index = self.nodes.len();
        self.nodes.push(DraftNode {
            node,
            parent: scope.owner,
            start: sym.start,
            end: sym.end,
            precision: sym.precision,
        });
        Some(index)
    }

    pub(super) fn patch(&mut self, index: usize) {
        let cursor = self.cursor_end();
        let Some(start) = self.nodes.get(index).map(|draft| draft.start) else {
            return;
        };
        let end = cursor.max(start);
        let lines = self.span_lines(start, end);
        let fingerprint = body_fingerprint(self.slice(start, end));
        let Some(draft) = self.nodes.get_mut(index) else {
            return;
        };
        draft.end = end;
        draft.node.lines = lines;
        draft.node.fingerprint = fingerprint;
    }

    pub(super) fn emit_ref(&mut self, site: OutRef<'_>) {
        if site.name.is_empty() || !self.charge() {
            return;
        }
        self.refs.push(DraftRef {
            src: site.owner,
            kind: site.kind,
            name: site.name,
            scope: site.scope.to_owned(),
            start: site.start,
            end: site.end.max(site.start),
            precision: site.precision,
        });
    }

    fn charge(&mut self) -> bool {
        if self.facts >= self.max_facts {
            self.refused = Some(LexError::TooManyFacts {
                limit: self.max_facts,
            });
            false
        } else {
            self.facts += 1;
            true
        }
    }

    pub(super) fn problem(&mut self, byte: u32, reason: DiagnosticReason) {
        self.problems.push((byte, reason));
    }

    pub(super) fn problem_here(&mut self, reason: DiagnosticReason) {
        let byte = self.tok_start();
        self.problem(byte, reason);
    }
}

pub(super) fn qualify(scope: &Scope, name: &str) -> String {
    if scope.name.is_empty() {
        name.to_owned()
    } else {
        format!("{}::{name}", scope.name)
    }
}

fn file_lines(source: &str) -> u32 {
    let newlines = source.bytes().filter(|byte| *byte == b'\n').count();
    u32::try_from(newlines).unwrap_or(u32::MAX).saturating_add(1).max(1)
}

fn file_summary(path: &str, source: &str, lexed: &Lexed) -> (String, SummarySource) {
    let mut expect = 1u32;
    let mut parts = Vec::new();
    for &(start, end) in &lexed.comments {
        let start_line = lexed.line(start);
        if start_line > expect {
            break;
        }
        parts.push(source.get(start as usize..end as usize).unwrap_or(""));
        expect = lexed.line(end.saturating_sub(1).max(start)).saturating_add(1);
    }
    match comment_summary(&parts.join("\n")) {
        Some(text) => (text, SummarySource::Doc),
        None => (path.to_owned(), SummarySource::Signature),
    }
}

fn labeled(end: u32, base: Precision, first: u32) -> Precision {
    if end > first {
        Precision::Partial
    } else {
        base
    }
}

fn cap(text: &str) -> String {
    if text.chars().count() > 200 {
        text.chars().take(200).collect()
    } else {
        text.to_owned()
    }
}

fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 400
        && !name.contains(['#', '~'])
        && !name.chars().any(char::is_control)
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
        let key = (draft.node.kind, draft.node.name.clone());
        let next = ordinals.entry(key.clone()).or_insert(0u32);
        *next += 1;
        let ordinal = (counts.get(&key).copied().unwrap_or(0) > 1).then_some(*next);
        draft.node.id = StableId::new(draft.node.kind, path, Some(&draft.node.name), ordinal)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::RubyLexer;

    #[test]
    fn duplicate_methods_receive_ordinals() {
        let facts = RubyLexer::default()
            .extract("p.rb", b"def f\nend\ndef f\nend\n")
            .expect("extract");
        let ids: Vec<_> = facts.nodes.iter().map(|node| node.fact.id.as_str()).collect();
        assert!(ids.contains(&"s:p.rb#f~1"), "{ids:?}");
        assert!(ids.contains(&"s:p.rb#f~2"), "{ids:?}");
        assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    }
}
