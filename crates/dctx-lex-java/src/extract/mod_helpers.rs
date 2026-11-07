// SPDX-License-Identifier: Apache-2.0
//! Fact draft assembly: ordinals, containment edges and precision labels.
//! Implements SPEC-MAP-MODE 5.1–5.4 source order for Java, as dctx-lex-js finalizes facts.
//! Depends on the draft types in `extract` and on lexer line maps.
//! [`super::extract`] returns the file; declaration scans update the package and fingerprints.
//! Word classes for modifiers, primitives and non-callees are shared by every scanner.

use super::DraftNode;
use crate::domain::{
    Completion, Diagnostic, Dialect, Labeled, LexError, LexedFile, Precision, RawReference,
    body_fingerprint, javadoc_summary,
};
use crate::lexer::Lexed;
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource,
};
use std::cmp::Reverse;
use std::collections::BTreeMap;

impl super::Draft {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling."
    )]
    pub(super) fn new(
        path: &str,
        source: &str,
        lexed: &Lexed,
        max_facts: usize,
    ) -> Result<Self, LexError> {
        let id = StableId::new(NodeKind::Module, path, None, None)?;
        let lines = source.lines().count().max(1) as u32;
        let summary = match leading_javadoc(source, lexed) {
            Some(text) => Summary {
                text,
                source: SummarySource::Doc,
            },
            None => Summary {
                text: String::new(),
                source: SummarySource::Signature,
            },
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

    pub(super) fn set_package(&mut self, name: &str) {
        if let Some(module) = self.nodes.first_mut() {
            module.node.signature = name.to_string();
            if module.node.summary.source == SummarySource::Signature {
                module.node.summary.text = name.to_string();
            }
        }
    }

    pub(super) fn set_fingerprint(&mut self, index: usize, body: &str) {
        if let Some(node) = self.nodes.get_mut(index) {
            node.node.fingerprint = body_fingerprint(body);
        }
    }

    /// Returns false when another fact would pass the limit. The file is then refused.
    pub(super) fn accept(&mut self) -> bool {
        if self.overflow || self.nodes.len() + self.refs.len() >= self.max_facts {
            self.overflow = true;
            false
        } else {
            true
        }
    }

    pub(super) fn finish(mut self, lexed: &Lexed) -> Result<LexedFile, LexError> {
        if self.overflow {
            return Err(LexError::TooManyFacts {
                limit: self.max_facts,
            });
        }
        let mut order: Vec<usize> = (1..self.nodes.len()).collect();
        order.sort_by_key(|&index| (self.nodes[index].start, Reverse(self.nodes[index].end)));
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
        self.refs
            .sort_by_key(|reference| (reference.start, Reverse(reference.end)));
        let mut diagnostics = lexed.diagnostics.clone();
        diagnostics.extend(self.problems.iter().map(|&(byte, reason)| Diagnostic {
            line: lexed.line(byte),
            byte,
            reason,
        }));
        diagnostics.sort_by_key(|diagnostic| diagnostic.byte);
        let first_problem = diagnostics.first().map_or(u32::MAX, |d| d.byte);
        let completion = if diagnostics.is_empty() {
            Completion::Complete
        } else {
            Completion::Partial
        };
        diagnostics.truncate(LexedFile::MAX_DIAGNOSTICS);
        let path = self.path.clone();
        let label = |end: u32, base: Precision| {
            if end > first_problem {
                Precision::Partial
            } else {
                base
            }
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
            .map(|node| Labeled {
                precision: if node.end == u32::MAX {
                    node.precision
                } else {
                    label(node.end, node.precision)
                },
                fact: node.node,
            })
            .collect();
        Ok(LexedFile {
            path,
            dialect: Dialect::Java,
            completion,
            diagnostics,
            nodes,
            edges,
            references,
        })
    }
}

fn assign_ordinals(nodes: &mut [DraftNode]) -> Result<(), LexError> {
    let mut counts = BTreeMap::<(NodeKind, String), u32>::new();
    for draft in nodes.iter().skip(1) {
        *counts
            .entry((draft.node.kind, draft.node.name.clone()))
            .or_insert(0) += 1;
    }
    let mut ordinals = BTreeMap::<(NodeKind, String), u32>::new();
    for draft in nodes.iter_mut().skip(1) {
        let node = &mut draft.node;
        let key = (node.kind, node.name.clone());
        let next = ordinals.entry(key.clone()).or_insert(0);
        *next += 1;
        let ordinal = (counts[&key] > 1).then_some(*next);
        node.id = StableId::new(node.kind, &node.path, Some(&node.name), ordinal)?;
    }
    Ok(())
}

fn leading_javadoc(source: &str, lexed: &Lexed) -> Option<String> {
    let comment = lexed.comments.iter().find(|comment| comment.javadoc)?;
    let before = source
        .get(..comment.start as usize)?
        .trim_start_matches('\u{feff}');
    if !before.chars().all(char::is_whitespace) {
        return None;
    }
    let text = source.get(comment.start as usize..comment.end as usize)?;
    javadoc_summary(text)
}

pub(crate) fn is_modifier_word(word: &str) -> bool {
    matches!(
        word,
        "public"
            | "protected"
            | "private"
            | "static"
            | "final"
            | "abstract"
            | "sealed"
            | "strictfp"
            | "native"
            | "synchronized"
            | "transient"
            | "volatile"
            | "default"
            | "transitive"
    )
}

pub(crate) fn is_primitive(word: &str) -> bool {
    matches!(
        word,
        "byte" | "short" | "int" | "long" | "char" | "float" | "double" | "boolean" | "void"
    )
}

pub(crate) fn is_non_callee(word: &str) -> bool {
    matches!(
        word,
        "if" | "for"
            | "while"
            | "switch"
            | "catch"
            | "synchronized"
            | "try"
            | "do"
            | "else"
            | "return"
            | "throw"
            | "break"
            | "continue"
            | "assert"
            | "case"
            | "default"
            | "new"
            | "class"
            | "interface"
            | "enum"
            | "record"
            | "package"
            | "import"
            | "instanceof"
            | "finally"
    )
}
