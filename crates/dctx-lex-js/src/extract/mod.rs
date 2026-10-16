//! Structural walker over the token stream: shared state, scopes and fact emission.
//! Implements SPEC-MAP-MODE 5.1–5.4 and 6.2 with the dctx-map-parse ownership rules:
//! a declaration owns everything lexically inside it. Uses `lexer` tokens and `assemble`.
mod assemble;
mod calls;
mod classes;
mod expressions;
mod functions;
mod members;
mod modules;
mod statements;
mod typedecl;
mod types;

use crate::domain::{
    DiagnosticReason, Dialect, LexError, LexedFile, Limits, Precision, body_fingerprint, jsdoc,
};
use crate::lexer::{Kind, Lexed, NONE, Token};
use assemble::Draft;
use dctx_map_model::{EdgeKind, NodeKind, StableId, Summary, SummarySource, SymbolKind};
use std::collections::HashMap;

/// Extracts facts from an already-lexed file.
pub(crate) fn extract(
    path: &str,
    source: &str,
    dialect: Dialect,
    lexed: &Lexed,
    limits: &Limits,
) -> Result<LexedFile, LexError> {
    let draft = Draft::new(path, source, lexed, limits.max_facts)?;
    let mut walker = Walker {
        src: source,
        toks: &lexed.tokens,
        partner: &lexed.partner,
        lexed,
        ts: dialect.typed(),
        draft,
        angles: HashMap::new(),
        binding: None,
        dry: 0,
        depth: 0,
        max_depth: limits.max_depth.saturating_mul(2).saturating_add(8),
        refused: None,
    };
    walker.statements(0, lexed.tokens.len(), &Scope::default());
    if let Some(error) = walker.refused.take() {
        return Err(error);
    }
    walker.draft.finish(dialect, lexed, limits.max_facts)
}

/// Lexical owner of nested facts; `name` is `::`-qualified, empty at the root.
#[derive(Debug, Clone, Default)]
pub(crate) struct Scope {
    pub owner: usize,
    pub name: String,
    pub test: bool,
}

/// A `const x = <value>` site, so `require` can bind its pattern.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Binding {
    pub pattern: usize,
    pub value: usize,
    pub end: usize,
}

/// A recognised declaration, in token and byte coordinates.
pub(crate) struct Item {
    pub kind: NodeKind,
    pub symbol: SymbolKind,
    pub local: String,
    pub first: usize,
    pub last: usize,
    pub sig_end: u32,
    pub body: (u32, u32),
    pub public: bool,
    pub doc: Option<usize>,
}

pub(crate) struct Walker<'a> {
    pub src: &'a str,
    pub toks: &'a [Token],
    pub partner: &'a [u32],
    pub lexed: &'a Lexed,
    pub ts: bool,
    pub draft: Draft,
    pub angles: HashMap<usize, usize>,
    pub binding: Option<Binding>,
    pub dry: u32,
    depth: u32,
    max_depth: u32,
    pub refused: Option<LexError>,
}

/// Words that never name a callee or a declaration.
const RESERVED: [&str; 37] = [
    "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete",
    "do", "else", "export", "extends", "finally", "for", "function", "if", "import", "in",
    "instanceof", "new", "return", "super", "switch", "this", "throw", "try", "typeof", "var",
    "void", "while", "with", "yield", "await", "null", "true", "false",
];

impl<'a> Walker<'a> {
    pub fn kind(&self, i: usize) -> Option<Kind> {
        self.toks.get(i).map(|t| t.kind)
    }

    pub fn text(&self, i: usize) -> &'a str {
        self.toks
            .get(i)
            .map_or("", |t| &self.src[t.start as usize..t.end as usize])
    }

    /// Punctuator or word with exactly this spelling.
    pub fn is(&self, i: usize, spelling: &str) -> bool {
        matches!(self.kind(i), Some(Kind::Punct | Kind::Ident)) && self.text(i) == spelling
    }

    pub fn ident(&self, i: usize) -> bool {
        self.kind(i) == Some(Kind::Ident)
    }

    /// An identifier usable as a binding or callee name.
    pub fn word(&self, i: usize) -> bool {
        self.ident(i) && !RESERVED.contains(&self.text(i))
    }

    pub fn nl(&self, i: usize) -> bool {
        self.toks.get(i).is_some_and(|t| t.nl)
    }

    /// Partner of the opener at `i`, or `end` when it is unmatched or out of range.
    pub fn close(&self, i: usize, end: usize) -> usize {
        match self.partner.get(i).copied() {
            Some(p) if p != NONE && (p as usize) > i && (p as usize) < end => p as usize,
            _ => end,
        }
    }

    /// Index just after the group opened at `i`.
    pub fn after(&self, i: usize, end: usize) -> usize {
        (self.close(i, end) + 1).min(end)
    }

    pub fn start_byte(&self, i: usize) -> u32 {
        self.toks
            .get(i)
            .map_or(self.src.len() as u32, |t| t.start)
    }

    pub fn end_byte(&self, i: usize) -> u32 {
        self.toks.get(i).map_or(self.src.len() as u32, |t| t.end)
    }

    pub fn line(&self, byte: u32) -> u32 {
        self.lexed.line(byte)
    }

    pub fn ok(&self) -> bool {
        self.refused.is_none()
    }

    /// Enters one nesting level; refuses the file past the depth bound.
    pub fn enter(&mut self, at: usize) -> bool {
        self.depth += 1;
        if self.depth > self.max_depth {
            let line = self.line(self.start_byte(at));
            self.refused.get_or_insert(LexError::TooDeep {
                line,
                limit: self.max_depth,
            });
            return false;
        }
        self.ok()
    }

    pub fn leave(&mut self) {
        self.depth -= 1;
    }

    /// JSDoc summary of the comment directly before token `anchor`.
    pub fn doc(&self, anchor: usize) -> Option<String> {
        let index = self.toks.get(anchor)?.doc.checked_sub(1)?;
        let (start, end) = *self.lexed.comments.get(index as usize)?;
        jsdoc(&self.src[start as usize..end as usize])
    }

    /// Emits a declaration and returns the scope its contents belong to.
    pub fn item(&mut self, item: Item, scope: &Scope) -> Scope {
        if self.dry > 0 || !self.ok() {
            return scope.clone();
        }
        let name = if scope.name.is_empty() {
            item.local
        } else {
            format!("{}::{}", scope.name, item.local)
        };
        let start = self.start_byte(item.first);
        let end = self.end_byte(item.last).max(start);
        let path = self.draft.path.clone();
        let id = match StableId::new(item.kind, &path, Some(&name), None) {
            Ok(id) if !name.contains('~') => id,
            _ => {
                self.draft.problem(start, DiagnosticReason::UnsupportedName);
                return scope.clone();
            }
        };
        let sig_end = (item.sig_end.clamp(start, end)) as usize;
        let signature = self.src[start as usize..sig_end]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let summary = match item.doc.and_then(|anchor| self.doc(anchor)) {
            Some(text) => Summary {
                text,
                source: SummarySource::Doc,
            },
            None => Summary {
                text: signature.clone(),
                source: SummarySource::Signature,
            },
        };
        let (body_start, body_end) = item.body;
        let body = self
            .src
            .get(body_start as usize..body_end as usize)
            .unwrap_or_default();
        let precision = if item.kind == NodeKind::Test {
            Precision::Convention
        } else {
            Precision::Structural
        };
        let lines = [self.line(start), self.line(end.saturating_sub(1).max(start))];
        let node = dctx_map_model::Node {
            id,
            kind: item.kind,
            symbol_kind: Some(item.symbol),
            path,
            lines,
            name: name.clone(),
            signature,
            fingerprint: body_fingerprint(body),
            public: item.public,
            summary,
        };
        match self
            .draft
            .node(node, scope.owner, (start, end), precision)
        {
            Some(owner) => Scope {
                owner,
                name,
                test: scope.test || item.kind == NodeKind::Test,
            },
            None => scope.clone(),
        }
    }

    /// Emits one reference; `site` gives its lines, `key` its traversal position.
    #[allow(clippy::too_many_arguments)]
    pub fn refer(
        &mut self,
        src: usize,
        scope: &Scope,
        kind: EdgeKind,
        name: String,
        alias: Option<String>,
        site: (usize, usize),
        key: (usize, usize),
        precision: Precision,
    ) {
        if self.dry > 0 || name.is_empty() || !self.ok() {
            return;
        }
        let lines = [
            self.line(self.start_byte(site.0)),
            self.line(self.end_byte(site.1).saturating_sub(1)),
        ];
        let lines = [lines[0], lines[1].max(lines[0])];
        self.draft.reference(assemble::DraftRef {
            src,
            kind,
            name,
            scope: scope.name.clone(),
            alias,
            lines,
            start: self.start_byte(key.0),
            end: self.end_byte(key.1),
            precision,
        });
    }
}
