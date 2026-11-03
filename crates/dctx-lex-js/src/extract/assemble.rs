// SPDX-License-Identifier: Apache-2.0
//! Fact assembly: pre-order sorting, duplicate ordinals, containment edges and labels.
//! Implements SPEC-MAP-MODE 5.1–5.4 exactly as dctx-map-parse finalizes its facts.
//! Uses model constructors; the walker feeds drafts, `extract` returns the result.
use crate::domain::{
    Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexError, LexedFile, Precision,
    RawReference, body_fingerprint, first_comment, jsdoc,
};
use crate::lexer::Lexed;
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource,
};
use std::cmp::Reverse;
use std::collections::BTreeMap;

pub(crate) struct DraftNode {
    pub node: Node,
    parent: usize,
    start: u32,
    end: u32,
    precision: Precision,
}

pub(crate) struct DraftRef {
    pub src: usize,
    pub kind: EdgeKind,
    pub name: String,
    pub scope: String,
    pub alias: Option<String>,
    pub lines: [u32; 2],
    pub start: u32,
    pub end: u32,
    pub precision: Precision,
}

pub(crate) struct Draft {
    pub path: String,
    pub nodes: Vec<DraftNode>,
    refs: Vec<DraftRef>,
    problems: Vec<(u32, DiagnosticReason)>,
    max_facts: usize,
    overflow: bool,
}

impl Draft {
    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    pub fn new(path: &str, source: &str, lexed: &Lexed, max_facts: usize) -> Result<Self, LexError> {
        let id = StableId::new(NodeKind::Module, path, None, None)?;
        let mut text = source.lines().find_map(first_comment).unwrap_or_default();
        if let Some(&(start, end)) = lexed.comments.first()
            && !lexed.hashbang
            && lexed.tokens.first().is_none_or(|t| t.start > start)
            && let Some(doc) = jsdoc(&source[start as usize..end as usize])
        {
            text = doc;
        }
        let module = Node {
            id,
            kind: NodeKind::Module,
            symbol_kind: None,
            path: path.into(),
            lines: [1, source.lines().count().max(1) as u32],
            name: path.into(),
            signature: String::new(),
            fingerprint: body_fingerprint(source),
            public: false,
            summary: Summary {
                text,
                source: SummarySource::Doc,
            },
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
            overflow: false,
        })
    }

    fn full(&mut self) -> bool {
        self.overflow |= self.nodes.len() + self.refs.len() >= self.max_facts;
        self.overflow
    }

    pub fn node(
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

    pub fn reference(&mut self, reference: DraftRef) {
        if !self.full() {
            self.refs.push(reference);
        }
    }

    pub fn problem(&mut self, byte: u32, reason: DiagnosticReason) {
        self.problems.push((byte, reason));
    }

    pub fn finish(
        mut self,
        dialect: Dialect,
        lexed: &Lexed,
        max_facts: usize,
    ) -> Result<LexedFile, LexError> {
        if self.overflow {
            return Err(LexError::TooManyFacts { limit: max_facts });
        }
        let mut order: Vec<usize> = (1..self.nodes.len()).collect();
        order.sort_by_key(|&i| (self.nodes[i].start, Reverse(self.nodes[i].end)));
        let mut remap = vec![0; self.nodes.len()];
        for (position, &old) in order.iter().enumerate() {
            remap[old] = position + 1;
        }
        let mut slots: Vec<Option<DraftNode>> = self.nodes.drain(..).map(Some).collect();
        let mut nodes = Vec::with_capacity(slots.len());
        nodes.extend(slots[0].take());
        for &old in &order {
            nodes.extend(slots[old].take());
        }
        for node in nodes.iter_mut().skip(1) {
            node.parent = remap[node.parent];
        }
        assign_ordinals(&mut nodes)?;
        self.refs.sort_by_key(|r| (r.start, Reverse(r.end)));
        let first_problem = lexed.diagnostics.first().map_or(u32::MAX, |d| d.byte);
        let label = |end: u32, base: Precision| {
            if end > first_problem {
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
            .map(|r| Labeled {
                precision: label(r.end, r.precision),
                fact: RawReference {
                    src: nodes[remap[r.src]].node.id.clone(),
                    kind: r.kind,
                    name: r.name,
                    scope: r.scope,
                    alias: r.alias,
                    lines: r.lines,
                    owner_path: path.clone(),
                },
            })
            .collect();
        let mut diagnostics = lexed.diagnostics.clone();
        diagnostics.extend(self.problems.iter().map(|&(byte, reason)| Diagnostic {
            line: lexed.line(byte),
            byte,
            reason,
        }));
        diagnostics.sort_by_key(|d| d.byte);
        let completion = if diagnostics.is_empty() {
            Completion::Complete
        } else {
            Completion::Partial
        };
        diagnostics.truncate(LexedFile::MAX_DIAGNOSTICS);
        let nodes = nodes
            .into_iter()
            .map(|n| Labeled {
                precision: if n.end == u32::MAX {
                    n.precision
                } else {
                    label(n.end, n.precision)
                },
                fact: n.node,
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

/// Adds `~n` ordinals to every (kind, name) that occurs more than once.
fn assign_ordinals(nodes: &mut [DraftNode]) -> Result<(), LexError> {
    let mut counts = BTreeMap::new();
    for draft in nodes.iter().skip(1) {
        *counts
            .entry((draft.node.kind, draft.node.name.clone()))
            .or_insert(0_u32) += 1;
    }
    let mut ordinals = BTreeMap::new();
    for draft in nodes.iter_mut().skip(1) {
        let node = &mut draft.node;
        let key = (node.kind, node.name.clone());
        let next = ordinals.entry(key.clone()).or_insert(0_u32);
        *next += 1;
        let ordinal = (counts[&key] > 1).then_some(*next);
        node.id = StableId::new(node.kind, &node.path, Some(&node.name), ordinal)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{JsLexer, Limits, Precision};

    #[test]
    fn duplicates_get_ordinals_and_edges_follow_node_order() {
        let facts = JsLexer::default()
            .extract("t.js", b"it('same', () => run()); it('same', () => run());")
            .unwrap();
        let ids: Vec<_> = facts.nodes.iter().map(|n| n.fact.id.as_str()).collect();
        assert_eq!(ids, ["m:t.js", "t:t.js#same~1", "t:t.js#same~2"]);
        assert_eq!(facts.edges[1].fact.dst.as_str(), "t:t.js#same~2");
        assert_eq!(facts.references[2].fact.src.as_str(), "t:t.js#same~2");
    }

    #[test]
    fn facts_past_the_first_problem_are_partial_and_limits_refuse() {
        let facts = JsLexer::default()
            .extract("p.js", b"function a() {}\nfunction b() { run(\n")
            .unwrap();
        assert_eq!(facts.nodes[1].precision, Precision::Structural);
        assert_eq!(facts.nodes[2].precision, Precision::Partial);
        let tight = JsLexer::new(Limits {
            max_facts: 2,
            ..Limits::default()
        });
        assert!(tight.extract("p.js", b"a(); b(); c();").is_err());
        assert!(tight.extract("p.js", b"a();").is_ok());
    }
}
