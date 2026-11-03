// SPDX-License-Identifier: Apache-2.0
//! Structural fact assembly, lexical scopes and bounded source coordinates.
//! Implements SPEC-MAP-MODE 5–6.2 with file-owned identities and containment.
//! Sibling walkers consume opaque lexer tokens; no names are resolved here.
mod declarations;
mod members;
mod references;

use crate::domain::{
    Completion, DiagnosticReason, Dialect, Labeled, LexError, LexedFile, Limits, Precision,
    RawReference, body_fingerprint,
};
use crate::lexer::{Kind, Lexed};
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource, SymbolKind,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Default)]
struct Scope {
    owner: usize,
    name: String,
    class: bool,
    executable: bool,
}

struct Item {
    local: String,
    symbol: Option<SymbolKind>,
    first: usize,
    last: usize,
    sig_end: usize,
    public: bool,
}

struct Walker<'a> {
    src: &'a str,
    lexed: &'a Lexed,
    out: LexedFile,
    limits: &'a Limits,
    ordinals: BTreeMap<String, u32>,
}

#[allow(clippy::cast_possible_truncation, reason = "Source offsets and line counts fit u32 because PhpLexer::extract enforces the 8 MiB input ceiling.")]
pub(crate) fn extract(
    path: &str,
    src: &str,
    lexed: &Lexed,
    limits: &Limits,
) -> Result<LexedFile, LexError> {
    let file = Node {
        id: StableId::new(NodeKind::Module, path, None, None)?,
        kind: NodeKind::Module,
        symbol_kind: None,
        path: path.to_string(),
        lines: [1, src.lines().count().max(1) as u32],
        name: path.to_string(),
        signature: String::new(),
        fingerprint: body_fingerprint(src),
        public: false,
        summary: Summary {
            text: String::new(),
            source: SummarySource::Signature,
        },
    };
    let mut w = Walker {
        src,
        lexed,
        limits,
        ordinals: BTreeMap::new(),
        out: LexedFile {
            path: path.to_string(),
            dialect: Dialect::Php,
            completion: Completion::Complete,
            diagnostics: lexed.diagnostics.clone(),
            nodes: vec![Labeled {
                fact: file,
                precision: Precision::Structural,
            }],
            edges: Vec::new(),
            references: Vec::new(),
        },
    };
    w.capacity(0)?;
    w.range(0, lexed.tokens.len(), Scope::default())?;
    w.out.diagnostics.sort_by_key(|d| d.byte);
    if !w.out.diagnostics.is_empty() {
        w.out.completion = Completion::Partial;
        for node in &mut w.out.nodes {
            if w.out
                .diagnostics
                .iter()
                .any(|d| node.fact.lines[0] <= d.line && d.line <= node.fact.lines[1])
            {
                node.precision = Precision::Partial;
            }
        }
        let partial: BTreeSet<_> = w
            .out
            .nodes
            .iter()
            .filter(|n| n.precision == Precision::Partial)
            .map(|n| &n.fact.id)
            .collect();
        for edge in &mut w.out.edges {
            if partial.contains(&edge.fact.dst) {
                edge.precision = Precision::Partial;
            }
        }
        for reference in &mut w.out.references {
            if w.out
                .diagnostics
                .iter()
                .any(|d| reference.fact.lines[1] >= d.line)
            {
                reference.precision = Precision::Partial;
            }
        }
    }
    Ok(w.out)
}

impl Walker<'_> {
    fn text(&self, i: usize) -> &str {
        self.lexed.tokens.get(i).map_or("", |t| {
            if t.kind == Kind::Boundary {
                ";"
            } else {
                &self.src[t.start..t.end]
            }
        })
    }

    fn is(&self, i: usize, word: &str) -> bool {
        self.text(i).eq_ignore_ascii_case(word)
    }
    fn kind(&self, i: usize) -> Option<Kind> {
        self.lexed.tokens.get(i).map(|t| t.kind)
    }
    fn start(&self, i: usize) -> usize {
        self.lexed.tokens.get(i).map_or(self.src.len(), |t| t.start)
    }
    fn end(&self, i: usize) -> usize {
        self.lexed.tokens.get(i).map_or(self.src.len(), |t| t.end)
    }
    fn close(&self, i: usize, end: usize) -> Option<usize> {
        self.lexed
            .partner
            .get(i)
            .copied()
            .flatten()
            .filter(|&p| p > i && p < end)
    }
    fn after(&self, i: usize, end: usize) -> usize {
        self.close(i, end).map_or(i + 1, |p| p + 1)
    }
    fn line(&self, byte: usize) -> u32 {
        self.lexed
            .line(byte)
            .min(self.out.nodes[0].fact.lines[1])
            .max(1)
    }
    #[allow(clippy::cast_possible_truncation, reason = "Source offsets and line counts fit u32 because PhpLexer::extract enforces the 8 MiB input ceiling.")]
    fn problem(&mut self, i: usize, reason: DiagnosticReason) {
        self.out.completion = Completion::Partial;
        if self.out.diagnostics.len() < LexedFile::MAX_DIAGNOSTICS {
            self.out.diagnostics.push(crate::Diagnostic {
                byte: self.start(i) as u32,
                line: self.line(self.start(i)),
                reason,
            });
        }
    }
    fn capacity(&self, added: usize) -> Result<(), LexError> {
        if self.out.nodes.len() + self.out.references.len() + added > self.limits.max_facts {
            Err(LexError::TooManyFacts {
                limit: self.limits.max_facts,
            })
        } else {
            Ok(())
        }
    }

    fn item(&mut self, item: Item, scope: &Scope) -> Result<Scope, LexError> {
        self.capacity(1)?;
        let name = if scope.name.is_empty() {
            item.local
        } else {
            format!("{}::{}", scope.name, item.local)
        };
        let count = self.ordinals.entry(name.clone()).or_default();
        *count += 1;
        let Ok(id) = StableId::new(
            NodeKind::Symbol,
            &self.out.path,
            Some(&name),
            (*count > 1).then_some(*count),
        ) else {
            self.problem(item.first, DiagnosticReason::UnsupportedName);
            return Ok(scope.clone());
        };
        let signature = normalize(&self.src[self.start(item.first)..item.sig_end]);
        let summary = self.doc_before(item.first).map_or_else(
            || Summary {
                text: signature.clone(),
                source: SummarySource::Signature,
            },
            |text| Summary {
                text,
                source: SummarySource::Doc,
            },
        );
        let node = Node {
            id: id.clone(),
            kind: NodeKind::Symbol,
            symbol_kind: item.symbol,
            path: self.out.path.clone(),
            lines: [
                self.line(self.start(item.first)),
                self.line(self.end(item.last).saturating_sub(1)),
            ],
            name: name.clone(),
            signature,
            fingerprint: body_fingerprint(&self.src[self.start(item.first)..self.end(item.last)]),
            public: item.public,
            summary,
        };
        self.out.edges.push(Labeled {
            fact: Edge {
                src: self.out.nodes[scope.owner].fact.id.clone(),
                dst: id,
                kind: EdgeKind::Contains,
                conf: Confidence::Exact,
                owner_path: self.out.path.clone(),
            },
            precision: Precision::Structural,
        });
        let owner = self.out.nodes.len();
        self.out.nodes.push(Labeled {
            fact: node,
            precision: Precision::Structural,
        });
        Ok(Scope {
            owner,
            name,
            class: false,
            executable: false,
        })
    }

    fn reference(
        &mut self,
        scope: &Scope,
        kind: EdgeKind,
        name: String,
        alias: Option<String>,
        site: (usize, Precision),
    ) -> Result<(), LexError> {
        let (at, precision) = site;
        self.capacity(1)?;
        if !name.is_empty() {
            self.out.references.push(Labeled {
                fact: RawReference {
                    src: self.out.nodes[scope.owner].fact.id.clone(),
                    kind,
                    name,
                    scope: scope.name.clone(),
                    alias,
                    lines: [
                        self.line(self.start(at)),
                        self.line(self.end(at).saturating_sub(1)),
                    ],
                    owner_path: self.out.path.clone(),
                },
                precision,
            });
        }
        Ok(())
    }

    fn name(&self, mut i: usize, end: usize) -> (String, usize) {
        let mut name = String::new();
        if i < end && self.is(i, "\\") {
            if self.end(i) != self.start(i + 1) {
                return (name, i + 1);
            }
            name.push('\\');
            i += 1;
        }
        if i >= end || self.kind(i) != Some(Kind::Ident) {
            return (String::new(), i);
        }
        name.push_str(self.text(i));
        i += 1;
        while i + 1 < end
            && self.is(i, "\\")
            && self.kind(i + 1) == Some(Kind::Ident)
            && self.end(i - 1) == self.start(i)
            && self.end(i) == self.start(i + 1)
        {
            name.push('\\');
            name.push_str(self.text(i + 1));
            i += 2;
        }
        (name, i)
    }

    fn statement_end(&self, mut i: usize, end: usize) -> usize {
        while i < end && !matches!(self.text(i), ";" | "}") {
            i = self.after(i, end);
        }
        i
    }
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
