//! Structural walker: scopes and top-level declaration dispatch.
//! Implements SPEC-MAP-MODE 5.1–5.4 for Go. A declaration owns the spans lexically
//! inside it. Uses lexer tokens; draft helpers, declarations and reference scans
//! are sibling modules.
mod declarations;
mod mod_helpers;
mod references;

use crate::domain::{body_fingerprint, DiagnosticReason, Dialect, LexError, LexedFile, Limits, Precision};
use crate::lexer::{keyword, Kind, Lexed, Token, NONE};
use dctx_map_model::{EdgeKind, Node, NodeKind, StableId, Summary, SummarySource, SymbolKind};
pub(crate) use mod_helpers::compact;
use mod_helpers::{comment_body, exported, gap_ok, normalize, sentence, Draft, DraftRef};

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

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    pub(crate) fn start_byte(&self, index: usize) -> u32 {
        self.toks.get(index).map_or(self.src.len() as u32, |token| token.start)
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
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
        let prev_end = token.checked_sub(1).map_or(0, |index| self.toks[index].end);
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
            item.local
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
