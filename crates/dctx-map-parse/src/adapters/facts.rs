// SPDX-License-Identifier: Apache-2.0
//! Source-ordered fact assembly with duplicate-id finalization.
//! Implements SPEC-MAP-MODE sections 5.1–5.4 for every language adapter.
//! Uses public model constructors; language adapters supply source facts.
use crate::{Language, ParseError, ParsedFile, RawReference, body_fingerprint};
use dctx_map_model::{
    Completion, Confidence, Coverage, Edge, EdgeKind, EvidenceLabel, Node, NodeKind, Precision,
    StableId, Summary, SummarySource,
};
use std::collections::BTreeMap;

pub(super) struct Facts {
    pub file: ParsedFile,
    parents: Vec<(usize, usize)>,
    sources: Vec<usize>,
}

impl Facts {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "Both parser entry points cap source at 8 MiB before fact assembly, so its line count fits u32."
    )]
    pub fn new(
        path: &str,
        source: &str,
        language: Language,
        precision: Precision,
    ) -> Result<Self, ParseError> {
        let id = StableId::new(NodeKind::Module, path, None, None)?;
        let comment = source.lines().find_map(first_comment);
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
                text: comment.unwrap_or_default(),
                source: SummarySource::Doc,
            },
        };
        Ok(Self {
            file: ParsedFile {
                path: path.into(),
                language,
                nodes: vec![module],
                edges: Vec::new(),
                references: Vec::new(),
                precision,
            },
            parents: Vec::new(),
            sources: Vec::new(),
        })
    }

    pub fn node(&mut self, node: Node, parent: usize) -> usize {
        let index = self.file.nodes.len();
        self.file.nodes.push(node);
        self.parents.push((parent, index));
        index
    }

    pub fn reference(
        &mut self,
        src: usize,
        kind: EdgeKind,
        name: String,
        scope: &str,
        alias: Option<String>,
        lines: [u32; 2],
    ) {
        if name.is_empty() {
            return;
        }
        self.sources.push(src);
        self.file.references.push(RawReference {
            src: self.file.nodes[src].id.clone(),
            kind,
            name,
            scope: scope.into(),
            alias,
            lines,
            owner_path: self.file.path.clone(),
        });
    }

    pub fn finish(mut self) -> Result<ParsedFile, ParseError> {
        let mut counts = BTreeMap::new();
        for node in self.file.nodes.iter().skip(1) {
            *counts
                .entry((node.kind, node.name.clone()))
                .or_insert(0_u32) += 1;
        }
        let mut ordinals = BTreeMap::new();
        for node in self.file.nodes.iter_mut().skip(1) {
            let key = (node.kind, node.name.clone());
            let next = ordinals.entry(key.clone()).or_insert(0_u32);
            *next = next.checked_add(1).ok_or(ParseError::TooLarge)?;
            let ordinal = (counts[&key] > 1).then_some(*next);
            node.id = StableId::new(node.kind, &node.path, Some(&node.name), ordinal)?;
        }
        for (reference, src) in self.file.references.iter_mut().zip(self.sources) {
            reference.src = self.file.nodes[src].id.clone();
        }
        for (parent, child) in self.parents {
            self.file.edges.push(Edge {
                src: self.file.nodes[parent].id.clone(),
                dst: self.file.nodes[child].id.clone(),
                kind: EdgeKind::Contains,
                conf: Confidence::Exact,
                owner_path: self.file.path.clone(),
            });
        }
        Ok(self.file)
    }
}

/// File-level precision. Parser ids are static and must pass [`EvidenceLabel::new`].
pub(super) fn file_precision(
    coverage: Coverage,
    completion: Completion,
    parser: &str,
) -> Result<Precision, ParseError> {
    Ok(Precision::new(
        coverage,
        completion,
        EvidenceLabel::new(parser)?,
    ))
}

#[cfg(feature = "syntax-tree-sitter")]
pub(super) fn text<'a>(node: tree_sitter::Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

#[cfg(feature = "syntax-tree-sitter")]
pub(super) fn lines(node: tree_sitter::Node<'_>) -> [u32; 2] {
    let start = node.start_position().row as u32 + 1;
    let end = node.end_position();
    [
        start,
        (end.row as u32 + u32::from(end.column > 0)).max(start),
    ]
}

#[cfg(any(test, feature = "syntax-tree-sitter"))]
pub(super) fn compact(value: &str) -> String {
    value.chars().filter(|c| !c.is_whitespace()).collect()
}

pub(super) fn sentence(value: &str) -> String {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    match value.find(". ") {
        Some(end) => value[..=end].into(),
        None => value,
    }
}

fn first_comment(line: &str) -> Option<String> {
    let line = line.trim();
    ["//!", "///", "//", "#", "<!--", "/*", "--", ";"]
        .iter()
        .find_map(|prefix| line.strip_prefix(prefix))
        .map(|body| sentence(body.trim_end_matches("-->").trim_end_matches("*/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_and_summary_text_is_normalized() {
        assert_eq!(first_comment("  //! First. Second."), Some("First.".into()));
        assert_eq!(first_comment("<!-- Notes. -->"), Some("Notes.".into()));
        assert_eq!(first_comment("ordinary text"), None);
        assert_eq!(compact("crate :: Thing"), "crate::Thing");
    }
}
