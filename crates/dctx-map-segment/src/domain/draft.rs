// SPDX-License-Identifier: Apache-2.0
//! Owned drafts the builder interns into dictionary ids.
//! Implements SEGMENTS-v1 section inputs before they become canonical records.
//! Depends on [`super::kinds`]. [`super::builder`] validates these at insertion and again while encoding.
//! String ids are assigned only at build, from one sorted dictionary.

use std::collections::BTreeSet;

use super::error::SegmentError;
use super::kinds::{
    Completion, Confidence, Coverage, DeclarationKind, EdgeKind, Field, Language, SegmentRole,
    SummarySource,
};

/// A declaration before its strings are interned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclarationDraft {
    /// Local name. Empty is allowed.
    pub name: String,
    /// Declaration tag.
    pub kind: DeclarationKind,
    /// Inclusive start byte.
    pub span_start: u64,
    /// Exclusive end byte.
    pub span_end: u64,
    /// Summary text.
    pub summary: String,
    /// Summary provenance.
    pub summary_source: SummarySource,
    /// Public for ranking.
    pub public: bool,
}

/// An unresolved reference before its strings are interned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawRefDraft {
    /// Source name.
    pub src: String,
    /// Relation. `contains` is refused.
    pub kind: EdgeKind,
    /// Unresolved spelling.
    pub spelling: String,
    /// Lexical scope. Empty is allowed.
    pub scope: String,
    /// Alias, when the reference has one.
    pub alias: Option<String>,
    /// One-based start line.
    pub line_start: u32,
    /// One-based end line.
    pub line_end: u32,
}

/// One file-local fact before interning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactDraft {
    /// SHA-256 of the file bytes.
    pub digest: [u8; 32],
    /// Byte length covered by [`Self::digest`].
    pub byte_length: u64,
    /// Language tag.
    pub language: Language,
    /// Dialect string.
    pub dialect: String,
    /// Parser name.
    pub parser_id: String,
    /// Parser revision.
    pub parser_revision: String,
    /// Extraction-plan digest.
    pub extraction_plan: [u8; 32],
    /// Declarations in caller order.
    pub declarations: Vec<DeclarationDraft>,
    /// Unresolved references in caller order.
    pub raw_refs: Vec<RawRefDraft>,
    /// Extraction coverage.
    pub coverage: Coverage,
    /// Extraction completion.
    pub completion: Completion,
    /// Reason text. Empty only when completion is complete.
    pub completion_reason: String,
    /// Extractor name.
    pub extractor: String,
    /// Caller-supplied generation.
    pub generation: u64,
    /// Imported model label.
    pub model: Option<String>,
    /// Body fingerprint, when the caller supplied one.
    pub body_fingerprint: Option<[u8; 32]>,
}

/// Document address inside a term row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocId {
    /// Index into the length table.
    Index(u32),
    /// Public id of a view node.
    Public(String),
}

/// One posting hit before document ids are resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostingDraft {
    /// Document address.
    pub doc: DocId,
    /// Absolute token positions.
    pub positions: Vec<u32>,
}

/// One field/term row before the term is interned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermDraft {
    /// Identifier, path, or doc. Path is refused on a fact segment.
    pub field: Field,
    /// Term text.
    pub term: String,
    /// Hits. Empty is refused.
    pub hits: Vec<PostingDraft>,
}

/// View identity before its strings are interned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewKeyDraft {
    /// Trust namespace. Non-empty.
    pub trust: String,
    /// Root id. Non-empty.
    pub root: String,
    /// Source snapshot digest.
    pub snapshot: [u8; 32],
    /// Overlay generation.
    pub overlay_generation: u64,
    /// Selection policy digest.
    pub policy: [u8; 32],
    /// Resolver revision. Non-empty.
    pub resolver_revision: String,
    /// Resolver environment digest.
    pub environment: [u8; 32],
}

/// One view node before dense ids are assigned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeDraft {
    /// Root-relative path. Non-empty.
    pub path: String,
    /// Public id. Non-empty and unique.
    pub public_id: String,
    /// SHA-256 of the fact's file bytes.
    pub digest: [u8; 32],
    /// Byte length covered by [`Self::digest`].
    pub byte_length: u64,
    /// Language tag.
    pub language: Language,
    /// Dialect string.
    pub dialect: String,
    /// Parser name.
    pub parser_id: String,
    /// Parser revision.
    pub parser_revision: String,
    /// Extraction-plan digest.
    pub extraction_plan: [u8; 32],
    /// Ordinal of the fact inside that key.
    pub local_fact_id: u32,
    /// Public for ranking.
    pub public: bool,
    /// Tier-0 for ranking.
    pub tier0: bool,
}

/// Far end of an edge before dense ids are assigned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EdgeEnd {
    /// Public id of the target node.
    Resolved(String),
    /// Unresolved spelling. Non-empty.
    Unresolved(String),
}

/// One forward edge before it is sorted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeDraft {
    /// Public id of the source node.
    pub source: String,
    /// Relation.
    pub kind: EdgeKind,
    /// Confidence permitted for [`Self::kind`].
    pub confidence: Confidence,
    /// Resolved node or unresolved spelling.
    pub target: EdgeEnd,
}

pub(crate) fn check_fact(draft: &FactDraft) -> Result<(), SegmentError> {
    for declaration in &draft.declarations {
        if declaration.span_end < declaration.span_start || declaration.span_end > draft.byte_length {
            return Err(SegmentError::Facts("span"));
        }
    }
    for reference in &draft.raw_refs {
        if reference.kind == EdgeKind::Contains {
            return Err(SegmentError::Facts("edge"));
        }
        if reference.line_start == 0 || reference.line_end < reference.line_start {
            return Err(SegmentError::Facts("lines"));
        }
    }
    match draft.completion {
        Completion::Complete if !draft.completion_reason.is_empty() => Err(SegmentError::Facts("reason")),
        Completion::Partial | Completion::Refused if draft.completion_reason.is_empty() => {
            Err(SegmentError::Facts("reason"))
        }
        _ => Ok(()),
    }
}

pub(crate) fn check_term(draft: &TermDraft, role: SegmentRole) -> Result<(), SegmentError> {
    if role == SegmentRole::Fact && draft.field == Field::Path {
        return Err(SegmentError::Postings("path"));
    }
    if draft.hits.is_empty() {
        return Err(SegmentError::Postings("frequency"));
    }
    let mut indexes = BTreeSet::new();
    let mut names = BTreeSet::new();
    for hit in &draft.hits {
        if !positions_increase(&hit.positions) {
            return Err(SegmentError::Postings(if hit.positions.is_empty() {
                "frequency"
            } else {
                "zero delta"
            }));
        }
        match &hit.doc {
            DocId::Index(id) => {
                if !indexes.insert(*id) {
                    return Err(SegmentError::Postings("zero delta"));
                }
            }
            DocId::Public(name) => {
                if name.is_empty() || !names.insert(name.as_str()) {
                    return Err(SegmentError::Postings("doc"));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn check_view_key(draft: &ViewKeyDraft) -> Result<(), SegmentError> {
    if draft.trust.is_empty() || draft.root.is_empty() || draft.resolver_revision.is_empty() {
        return Err(SegmentError::Nodes("key"));
    }
    Ok(())
}

pub(crate) fn check_node(draft: &NodeDraft) -> Result<(), SegmentError> {
    if draft.path.is_empty() {
        return Err(SegmentError::Nodes("empty path"));
    }
    if draft.public_id.is_empty() {
        return Err(SegmentError::Nodes("empty public"));
    }
    Ok(())
}

pub(crate) fn check_edge(draft: &EdgeDraft) -> Result<(), SegmentError> {
    if draft.source.is_empty() {
        return Err(SegmentError::Adjacency("node"));
    }
    if !draft.kind.permits(draft.confidence) {
        return Err(SegmentError::Adjacency("confidence"));
    }
    match &draft.target {
        EdgeEnd::Resolved(name) if name.is_empty() => Err(SegmentError::Adjacency("node")),
        EdgeEnd::Unresolved(name) if name.is_empty() => Err(SegmentError::Adjacency("name")),
        _ => Ok(()),
    }
}

fn positions_increase(positions: &[u32]) -> bool {
    if positions.is_empty() {
        return false;
    }
    let mut ordered = positions.to_vec();
    ordered.sort_unstable();
    ordered.windows(2).all(|pair| pair[1] > pair[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_refs_and_empty_unresolved_names_are_refused() {
        let mut fact = empty_fact();
        fact.raw_refs.push(RawRefDraft {
            src: "a".to_owned(),
            kind: EdgeKind::Contains,
            spelling: "b".to_owned(),
            scope: String::new(),
            alias: None,
            line_start: 1,
            line_end: 1,
        });
        assert_eq!(check_fact(&fact).unwrap_err(), SegmentError::Facts("edge"));
        fact.raw_refs.clear();
        assert!(check_fact(&fact).is_ok());
        let edge = EdgeDraft {
            source: "a".to_owned(),
            kind: EdgeKind::Calls,
            confidence: Confidence::Name,
            target: EdgeEnd::Unresolved(String::new()),
        };
        assert_eq!(check_edge(&edge).unwrap_err(), SegmentError::Adjacency("name"));
        assert_eq!(
            check_term(
                &TermDraft {
                    field: Field::Path,
                    term: "a.rs".to_owned(),
                    hits: vec![PostingDraft { doc: DocId::Index(0), positions: vec![0] }],
                },
                SegmentRole::Fact
            )
            .unwrap_err(),
            SegmentError::Postings("path")
        );
    }

    fn empty_fact() -> FactDraft {
        FactDraft {
            digest: [0; 32],
            byte_length: 0,
            language: Language::Rust,
            dialect: "d".to_owned(),
            parser_id: "p".to_owned(),
            parser_revision: "r".to_owned(),
            extraction_plan: [0; 32],
            declarations: Vec::new(),
            raw_refs: Vec::new(),
            coverage: Coverage::Syntax,
            completion: Completion::Complete,
            completion_reason: String::new(),
            extractor: "e".to_owned(),
            generation: 0,
            model: None,
            body_fingerprint: None,
        }
    }
}
