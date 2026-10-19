//! Swift structural extraction and file-owned graph assembly.
//! Implements SPEC-MAP-MODE 5.1–5.4 using only lexical evidence.
//! Declarations, members and unresolved references live in sibling modules.
mod declarations;
mod members;
mod references;

use crate::domain::*;
use crate::lexer::{Kind, Lexed};
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource, SymbolKind,
};
use std::collections::BTreeMap;

pub(crate) fn extract(
    path: &str,
    src: &str,
    lex: &Lexed,
    limits: &Limits,
) -> Result<LexedFile, LexError> {
    let id = StableId::new(NodeKind::Module, path, None, None)?;
    let root = Node {
        id,
        kind: NodeKind::Module,
        symbol_kind: None,
        path: path.into(),
        lines: [1, lex.line(src.len() as u32)],
        name: path.into(),
        signature: String::new(),
        fingerprint: body_fingerprint(src),
        public: false,
        summary: Summary {
            text: String::new(),
            source: SummarySource::Signature,
        },
    };
    let mut walker = Walker {
        src,
        lex,
        limits,
        output: LexedFile {
            path: path.into(),
            dialect: Dialect::Swift,
            completion: Completion::Complete,
            diagnostics: lex.diagnostics.clone(),
            nodes: vec![Labeled {
                fact: root,
                precision: Precision::Structural,
            }],
            edges: Vec::new(),
            references: Vec::new(),
        },
        counts: BTreeMap::new(),
        ranges: vec![(0, src.len() as u32)],
    };
    walker.check()?;
    walker.walk(0, lex.tokens.len(), 0, false)?;
    walker.finish_ids()?;
    if !walker.output.diagnostics.is_empty() {
        walker.output.completion = Completion::Partial;
        for (node, &(start, end)) in walker.output.nodes.iter_mut().zip(&walker.ranges) {
            if walker
                .output
                .diagnostics
                .iter()
                .any(|d| start <= d.byte && d.byte <= end)
            {
                node.precision = Precision::Partial;
            }
        }
        let labels: BTreeMap<_, _> = walker
            .output
            .nodes
            .iter()
            .map(|n| (&n.fact.id, n.precision))
            .collect();
        for edge in &mut walker.output.edges {
            if let Some(&precision) = labels.get(&edge.fact.dst) {
                edge.precision = precision;
            }
        }
    }
    Ok(walker.output)
}

struct Walker<'a> {
    src: &'a str,
    lex: &'a Lexed,
    limits: &'a Limits,
    output: LexedFile,
    counts: BTreeMap<String, u32>,
    ranges: Vec<(u32, u32)>,
}

impl Walker<'_> {
    fn text(&self, i: usize) -> &str {
        self.lex
            .tokens
            .get(i)
            .map_or("", |t| &self.src[t.start as usize..t.end as usize])
    }

    fn ident(&self, i: usize) -> bool {
        self.lex
            .tokens
            .get(i)
            .is_some_and(|t| t.kind == Kind::Ident)
    }

    fn start(&self, i: usize) -> u32 {
        self.lex
            .tokens
            .get(i)
            .map_or(self.src.len() as u32, |t| t.start)
    }

    fn end(&self, i: usize) -> u32 {
        self.lex
            .tokens
            .get(i)
            .map_or(self.src.len() as u32, |t| t.end)
    }

    fn close(&self, i: usize) -> Option<usize> {
        self.lex
            .partner
            .get(i)
            .copied()
            .flatten()
            .filter(|&j| j > i)
    }

    fn check(&self) -> Result<(), LexError> {
        if self.output.nodes.len() + self.output.edges.len() + self.output.references.len()
            > self.limits.max_facts
        {
            Err(LexError::TooManyFacts {
                limit: self.limits.max_facts,
            })
        } else {
            Ok(())
        }
    }

    fn problem(&mut self, i: usize, reason: DiagnosticReason) {
        let byte = self.start(i);
        if self.output.diagnostics.len() < LexedFile::MAX_DIAGNOSTICS {
            self.output.diagnostics.push(Diagnostic {
                byte,
                line: self.lex.line(byte),
                reason,
            });
        }
    }

    fn finish_ids(&mut self) -> Result<(), LexError> {
        let mut seen = BTreeMap::<String, u32>::new();
        let mut mapping = BTreeMap::new();
        for node in self.output.nodes.iter_mut().skip(1) {
            let name = &node.fact.name;
            let ordinal = seen.entry(name.clone()).or_default();
            *ordinal += 1;
            let id = StableId::new(
                NodeKind::Symbol,
                &self.output.path,
                Some(name),
                (self.counts[name] > 1).then_some(*ordinal),
            )?;
            mapping.insert(node.fact.id.clone(), id.clone());
            node.fact.id = id;
        }
        for edge in &mut self.output.edges {
            if let Some(id) = mapping.get(&edge.fact.src) {
                edge.fact.src = id.clone();
            }
            if let Some(id) = mapping.get(&edge.fact.dst) {
                edge.fact.dst = id.clone();
            }
        }
        for reference in &mut self.output.references {
            if let Some(id) = mapping.get(&reference.fact.src) {
                reference.fact.src = id.clone();
            }
        }
        self.output.diagnostics.sort_by_key(|d| d.byte);
        Ok(())
    }

    fn add(&mut self, item: &Item, owner: usize) -> Result<Option<usize>, LexError> {
        let local = item.name.trim_matches('`');
        let parent = &self.output.nodes[owner].fact;
        let name = if owner == 0 {
            local.to_string()
        } else {
            format!("{}::{local}", parent.name)
        };
        let count = self.counts.entry(name.clone()).or_default();
        *count += 1;
        let Ok(id) = StableId::new(
            NodeKind::Symbol,
            &self.output.path,
            Some(&name),
            Some(*count),
        ) else {
            self.problem(item.first, DiagnosticReason::UnsupportedName);
            return Ok(None);
        };
        let signature =
            normalize(&self.src[self.start(item.first) as usize..item.signature as usize]);
        let summary = self.doc(item.first).map_or_else(
            || Summary {
                text: signature.clone(),
                source: SummarySource::Signature,
            },
            |text| Summary {
                text,
                source: SummarySource::Doc,
            },
        );
        let start = self.start(item.first);
        let end = self.end(item.last);
        let node = Node {
            id: id.clone(),
            kind: NodeKind::Symbol,
            symbol_kind: item.kind,
            path: self.output.path.clone(),
            lines: [
                self.lex.line(start),
                self.lex.line(end.saturating_sub(1).max(start)),
            ],
            name,
            signature,
            fingerprint: body_fingerprint(
                &self.src[if item.signature == end {
                    start
                } else {
                    item.signature
                } as usize..end as usize],
            ),
            public: item.public,
            summary,
        };
        let index = self.output.nodes.len();
        self.output.edges.push(Labeled {
            fact: Edge {
                src: self.output.nodes[owner].fact.id.clone(),
                dst: id,
                kind: EdgeKind::Contains,
                conf: Confidence::Exact,
                owner_path: self.output.path.clone(),
            },
            precision: item.precision,
        });
        self.output.nodes.push(Labeled {
            fact: node,
            precision: item.precision,
        });
        self.ranges.push((start, end));
        self.check()?;
        Ok(Some(index))
    }
}

struct Item {
    name: String,
    kind: Option<SymbolKind>,
    first: usize,
    last: usize,
    signature: u32,
    public: bool,
    precision: Precision,
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Compiler directives that are expressions or availability checks, not macros.
/// Unknown `#name` tokens stay opaque and produce a diagnostic.
pub(super) fn expression_directive(text: &str) -> bool {
    matches!(
        text,
        "#available"
            | "#unavailable"
            | "#selector"
            | "#keyPath"
            | "#colorLiteral"
            | "#imageLiteral"
            | "#fileLiteral"
            | "#file"
            | "#fileID"
            | "#filePath"
            | "#line"
            | "#column"
            | "#function"
            | "#dsohandle"
            | "#warning"
            | "#error"
            | "#sourceLocation"
    )
}
