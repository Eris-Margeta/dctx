// SPDX-License-Identifier: Apache-2.0
//! Draft assembly, duplicate ordinals and signature or doc text helpers.
//! Implements SPEC-MAP-MODE 5.1–5.4 fact order, containment edges and summaries.
//! Depends on lexer positions and dctx-map-model nodes. The walker in `extract`
//! emits into a draft here and finishes it into a `LexedFile`.
use crate::domain::{
    Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexError, LexedFile, Precision,
    RawReference, body_fingerprint,
};
use crate::lexer::Lexed;
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource,
};
use std::cmp::Reverse;
use std::collections::BTreeMap;

pub(super) fn exported(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

pub(crate) fn compact(value: &str) -> String {
    value.chars().filter(|ch| !ch.is_whitespace()).collect()
}

pub(super) fn normalize(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn sentence(value: &str) -> String {
    let value = normalize(value);
    match value.find(". ") {
        Some(end) => value[..=end].to_string(),
        None => value,
    }
}

pub(super) fn comment_body(src: &str, start: u32, end: u32) -> String {
    let Some(text) = src.get(start as usize..end as usize) else {
        return String::new();
    };
    if let Some(rest) = text.strip_prefix("//") {
        return rest.trim().to_string();
    }
    if let Some(inner) = text
        .strip_prefix("/*")
        .and_then(|body| body.strip_suffix("*/"))
    {
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
pub(super) fn gap_ok(src: &str, from: u32, to: u32) -> bool {
    if from > to {
        return false;
    }
    let Some(gap) = src.get(from as usize..to as usize) else {
        return false;
    };
    gap.bytes().filter(|&byte| byte == b'\n').count() <= 1
        && gap
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

pub(super) struct DraftNode {
    pub(super) node: Node,
    parent: usize,
    start: u32,
    end: u32,
    precision: Precision,
}

pub(super) struct DraftRef {
    pub(super) src: usize,
    pub(super) kind: EdgeKind,
    pub(super) name: String,
    pub(super) scope: String,
    pub(super) alias: Option<String>,
    pub(super) lines: [u32; 2],
    pub(super) start: u32,
    pub(super) end: u32,
    pub(super) precision: Precision,
}

pub(super) struct Draft {
    pub(super) path: String,
    pub(super) nodes: Vec<DraftNode>,
    refs: Vec<DraftRef>,
    problems: Vec<(u32, DiagnosticReason)>,
    max_facts: usize,
    overflow: bool,
}

impl Draft {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling."
    )]
    pub(super) fn new(path: &str, source: &str, max_facts: usize) -> Result<Self, LexError> {
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

    pub(super) fn full(&mut self) -> bool {
        if self.nodes.len() + self.refs.len() >= self.max_facts {
            self.overflow = true;
            true
        } else {
            false
        }
    }

    pub(super) fn node(
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

    pub(super) fn reference(&mut self, reference: DraftRef) {
        if !self.full() {
            self.refs.push(reference);
        }
    }

    pub(super) fn problem(&mut self, byte: u32, reason: DiagnosticReason) {
        self.problems.push((byte, reason));
    }

    pub(super) fn finish(
        mut self,
        lexed: &Lexed,
        dialect: Dialect,
        max_facts: usize,
    ) -> Result<LexedFile, LexError> {
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
        self.refs
            .sort_by_key(|reference| (reference.start, Reverse(reference.end)));
        let mut diagnostics = lexed.diagnostics.clone();
        diagnostics.extend(self.problems.iter().map(|&(byte, reason)| Diagnostic {
            line: lexed.line(byte),
            byte,
            reason,
        }));
        diagnostics.sort_by_key(|diagnostic| (diagnostic.byte, diagnostic.reason as u8));
        let first = diagnostics
            .first()
            .map_or(u32::MAX, |diagnostic| diagnostic.byte);
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
        *counts
            .entry((draft.node.kind, draft.node.name.clone()))
            .or_insert(0_u32) += 1;
    }
    let mut ordinals = BTreeMap::new();
    for draft in nodes
        .iter_mut()
        .skip(1)
        .filter(|draft| named(draft.node.kind))
    {
        let key = (draft.node.kind, draft.node.name.clone());
        let next = ordinals.entry(key.clone()).or_insert(0_u32);
        *next += 1;
        let ordinal = (counts[&key] > 1).then_some(*next);
        draft.node.id = StableId::new(
            draft.node.kind,
            &draft.node.path,
            Some(&draft.node.name),
            ordinal,
        )?;
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
        let ids: Vec<_> = facts
            .nodes
            .iter()
            .map(|node| node.fact.id.as_str())
            .collect();
        assert!(ids.contains(&"s:p.go#F~1"), "{ids:?}");
        assert!(ids.contains(&"s:p.go#F~2"), "{ids:?}");
        assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    }
}
