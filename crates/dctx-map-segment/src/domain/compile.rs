// SPDX-License-Identifier: Apache-2.0
//! Turns drafts into packed section payloads.
//! Implements SEGMENTS-v1 kind order for one fact or view role.
//! Depends on the section codecs. [`super::builder`] calls [`sections`] and does not decode.
//! The dictionary is the sorted set of every string the role actually stores.

use std::collections::{BTreeMap, BTreeSet};

use super::adjacency::{self, ForwardEdge, Target, forward_key};
use super::dictionary::{self, OwnedStrings};
use super::directory::{BuiltSection, REQUIRED};
use super::draft::{
    self, DocId, EdgeDraft, EdgeEnd, FactDraft, NodeDraft, TermDraft, ViewKeyDraft,
};
use super::error::SegmentError;
use super::fact_record;
use super::facts::{self, Declaration, Fact, FactKey, Provenance, RawRef, validate_fact};
use super::kinds::{Completion, SectionKind, SegmentRole};
use super::limits::DecoderLimits;
use super::nodes::{self, ViewKey, ViewNode};
use super::postings::{self, Postings, TermHit, TermRow};
use super::prefix;

/// Borrowed drafts for one build.
pub(crate) struct Input<'a> {
    pub(crate) role: SegmentRole,
    pub(crate) facts: &'a [FactDraft],
    pub(crate) terms: &'a [TermDraft],
    pub(crate) lengths: Option<&'a [u32]>,
    pub(crate) view_key: Option<&'a ViewKeyDraft>,
    pub(crate) nodes: &'a [NodeDraft],
    pub(crate) edges: &'a [EdgeDraft],
}

pub(crate) fn sections(
    input: &Input<'_>,
    limits: &DecoderLimits,
) -> Result<Vec<BuiltSection>, SegmentError> {
    match input.role {
        SegmentRole::Fact => fact_sections(input, limits),
        SegmentRole::View => view_sections(input, limits),
    }
}

fn fact_sections(
    input: &Input<'_>,
    limits: &DecoderLimits,
) -> Result<Vec<BuiltSection>, SegmentError> {
    if input.view_key.is_some() || !input.nodes.is_empty() || !input.edges.is_empty() {
        return Err(SegmentError::Role("forbidden"));
    }
    for fact in input.facts {
        draft::check_fact(fact)?;
    }
    for term in input.terms {
        draft::check_term(term, SegmentRole::Fact)?;
    }
    let dict = fact_dictionary(input, limits)?;
    let mut facts = Vec::new();
    for fact in input.facts {
        facts.push(compile_fact(fact, &dict)?);
    }
    facts.sort_by(|left, right| facts::cmp_key(&left.key, &right.key));
    let postings = compile_postings(
        SegmentRole::Fact,
        input.terms,
        input.lengths,
        &BTreeMap::new(),
        0,
        &dict,
    )?;
    let mut sections = Vec::new();
    push_dictionary(&mut sections, &dict)?;
    push_prefixes(&mut sections, &dict)?;
    let (payload, count) = fact_record::encode(&facts, limits)?;
    sections.push(section(SectionKind::Facts, true, count, payload));
    push_postings(&mut sections, postings, SegmentRole::Fact, limits)?;
    Ok(sections)
}

fn view_sections(
    input: &Input<'_>,
    limits: &DecoderLimits,
) -> Result<Vec<BuiltSection>, SegmentError> {
    if !input.facts.is_empty() {
        return Err(SegmentError::Role("fact"));
    }
    let key_draft = input.view_key.ok_or(SegmentError::Role("view key"))?;
    draft::check_view_key(key_draft)?;
    for node in input.nodes {
        draft::check_node(node)?;
    }
    for edge in input.edges {
        draft::check_edge(edge)?;
    }
    for term in input.terms {
        draft::check_term(term, SegmentRole::View)?;
    }
    let dict = view_dictionary(input, limits)?;
    let (nodes, by_public) = compile_nodes(input.nodes, &dict)?;
    let edges = compile_edges(input.edges, &by_public, &dict)?;
    let postings = compile_postings(
        SegmentRole::View,
        input.terms,
        input.lengths,
        &by_public,
        nodes.len(),
        &dict,
    )?;
    let key = ViewKey {
        trust: dict.id(&key_draft.trust)?,
        root: dict.id(&key_draft.root)?,
        snapshot: key_draft.snapshot,
        overlay_generation: key_draft.overlay_generation,
        policy: key_draft.policy,
        resolver_revision: dict.id(&key_draft.resolver_revision)?,
        environment: key_draft.environment,
    };
    let node_count = u32::try_from(nodes.len()).map_err(|_| SegmentError::Arithmetic)?;
    let mut sections = Vec::new();
    push_dictionary(&mut sections, &dict)?;
    push_prefixes(&mut sections, &dict)?;
    push_postings(&mut sections, postings, SegmentRole::View, limits)?;
    let (payload, count) = nodes::encode(&key, &nodes, dict.as_slice(), limits)?;
    sections.push(section(SectionKind::Nodes, true, count, payload));
    let (payload, count) = adjacency::encode_forward(&edges, node_count, dict.as_slice(), limits)?;
    sections.push(section(SectionKind::Forward, true, count, payload));
    let (resolved, unresolved) = adjacency::reverse_of(&edges);
    let (payload, count) = adjacency::encode_reverse(&resolved, &unresolved, limits)?;
    sections.push(section(SectionKind::Reverse, true, count, payload));
    Ok(sections)
}

fn fact_dictionary(
    input: &Input<'_>,
    limits: &DecoderLimits,
) -> Result<OwnedStrings, SegmentError> {
    let mut set = BTreeSet::new();
    for fact in input.facts {
        remember(&mut set, &fact.dialect, limits)?;
        remember(&mut set, &fact.parser_id, limits)?;
        remember(&mut set, &fact.parser_revision, limits)?;
        remember(&mut set, &fact.extractor, limits)?;
        for declaration in &fact.declarations {
            remember(&mut set, &declaration.name, limits)?;
            remember(&mut set, &declaration.summary, limits)?;
        }
        for reference in &fact.raw_refs {
            remember(&mut set, &reference.src, limits)?;
            remember(&mut set, &reference.spelling, limits)?;
            remember(&mut set, &reference.scope, limits)?;
            if let Some(alias) = &reference.alias {
                remember(&mut set, alias, limits)?;
            }
        }
        if fact.completion != Completion::Complete {
            remember(&mut set, &fact.completion_reason, limits)?;
        }
        if let Some(model) = &fact.model {
            remember(&mut set, model, limits)?;
        }
    }
    for term in input.terms {
        remember(&mut set, &term.term, limits)?;
    }
    OwnedStrings::from_set(set, limits)
}

fn view_dictionary(
    input: &Input<'_>,
    limits: &DecoderLimits,
) -> Result<OwnedStrings, SegmentError> {
    let mut set = BTreeSet::new();
    if let Some(key) = input.view_key {
        remember(&mut set, &key.trust, limits)?;
        remember(&mut set, &key.root, limits)?;
        remember(&mut set, &key.resolver_revision, limits)?;
    }
    for node in input.nodes {
        remember(&mut set, &node.path, limits)?;
        remember(&mut set, &node.public_id, limits)?;
        remember(&mut set, &node.dialect, limits)?;
        remember(&mut set, &node.parser_id, limits)?;
        remember(&mut set, &node.parser_revision, limits)?;
    }
    for edge in input.edges {
        if let EdgeEnd::Unresolved(name) = &edge.target {
            remember(&mut set, name, limits)?;
        }
    }
    for term in input.terms {
        remember(&mut set, &term.term, limits)?;
    }
    OwnedStrings::from_set(set, limits)
}

fn compile_fact(draft: &FactDraft, dict: &OwnedStrings) -> Result<Fact, SegmentError> {
    let mut declarations = Vec::new();
    for declaration in &draft.declarations {
        declarations.push(Declaration {
            name: dict.id(&declaration.name)?,
            kind: declaration.kind,
            span_start: declaration.span_start,
            span_end: declaration.span_end,
            summary: dict.id(&declaration.summary)?,
            summary_source: declaration.summary_source,
            public: declaration.public,
        });
    }
    let mut raw_refs = Vec::new();
    for reference in &draft.raw_refs {
        raw_refs.push(RawRef {
            src: dict.id(&reference.src)?,
            kind: reference.kind,
            spelling: dict.id(&reference.spelling)?,
            scope: dict.id(&reference.scope)?,
            alias: match &reference.alias {
                Some(alias) => Some(dict.id(alias)?),
                None => None,
            },
            line_start: reference.line_start,
            line_end: reference.line_end,
        });
    }
    let fact = Fact {
        key: FactKey {
            digest: draft.digest,
            byte_length: draft.byte_length,
            language: draft.language,
            dialect: dict.id(&draft.dialect)?,
            parser_id: dict.id(&draft.parser_id)?,
            parser_revision: dict.id(&draft.parser_revision)?,
            extraction_plan: draft.extraction_plan,
        },
        declarations,
        raw_refs,
        provenance: Provenance {
            coverage: draft.coverage,
            completion: draft.completion,
            completion_reason: match draft.completion {
                Completion::Complete => 0,
                Completion::Partial | Completion::Refused => dict.id(&draft.completion_reason)?,
            },
            extractor: dict.id(&draft.extractor)?,
            generation: draft.generation,
            model: match &draft.model {
                Some(model) => Some(dict.id(model)?),
                None => None,
            },
            body_fingerprint: draft.body_fingerprint,
        },
    };
    validate_fact(&fact, dict.as_slice())?;
    Ok(fact)
}

fn compile_nodes<'a>(
    drafts: &'a [NodeDraft],
    dict: &OwnedStrings,
) -> Result<(Vec<ViewNode>, BTreeMap<&'a str, u32>), SegmentError> {
    let mut order: Vec<usize> = (0..drafts.len()).collect();
    order.sort_by(|&left, &right| drafts[left].public_id.cmp(&drafts[right].public_id));
    let mut nodes = Vec::new();
    let mut by_public = BTreeMap::new();
    let mut paths = BTreeSet::new();
    for index in order {
        let draft = &drafts[index];
        if !paths.insert(draft.path.as_str()) {
            return Err(SegmentError::Nodes("duplicate path"));
        }
        let dense = u32::try_from(nodes.len()).map_err(|_| SegmentError::Arithmetic)?;
        if by_public.insert(draft.public_id.as_str(), dense).is_some() {
            return Err(SegmentError::Nodes("duplicate public"));
        }
        nodes.push(ViewNode {
            dense_id: dense,
            path: dict.id(&draft.path)?,
            public_id: dict.id(&draft.public_id)?,
            fact: FactKey {
                digest: draft.digest,
                byte_length: draft.byte_length,
                language: draft.language,
                dialect: dict.id(&draft.dialect)?,
                parser_id: dict.id(&draft.parser_id)?,
                parser_revision: dict.id(&draft.parser_revision)?,
                extraction_plan: draft.extraction_plan,
            },
            local_fact_id: draft.local_fact_id,
            public: draft.public,
            tier0: draft.tier0,
        });
    }
    Ok((nodes, by_public))
}

fn compile_edges(
    drafts: &[EdgeDraft],
    nodes: &BTreeMap<&str, u32>,
    dict: &OwnedStrings,
) -> Result<Vec<ForwardEdge>, SegmentError> {
    let mut edges = Vec::new();
    for draft in drafts {
        let source = *nodes
            .get(draft.source.as_str())
            .ok_or(SegmentError::Adjacency("node"))?;
        let target = match &draft.target {
            EdgeEnd::Resolved(name) => Target::Resolved(
                *nodes
                    .get(name.as_str())
                    .ok_or(SegmentError::Adjacency("node"))?,
            ),
            EdgeEnd::Unresolved(name) => Target::Unresolved(dict.id(name)?),
        };
        edges.push(ForwardEdge {
            source,
            kind: draft.kind,
            confidence: draft.confidence,
            target,
        });
    }
    edges.sort_by_key(forward_key);
    Ok(edges)
}

fn compile_postings(
    role: SegmentRole,
    terms: &[TermDraft],
    lengths: Option<&[u32]>,
    nodes: &BTreeMap<&str, u32>,
    node_count: usize,
    dict: &OwnedStrings,
) -> Result<Option<Postings>, SegmentError> {
    if lengths.is_none() && terms.is_empty() {
        return Ok(None);
    }
    let Some(lengths) = lengths else {
        return Err(SegmentError::Postings("length"));
    };
    if role == SegmentRole::View && lengths.len() != node_count {
        return Err(SegmentError::Role("length"));
    }
    let doc_count = u32::try_from(lengths.len()).map_err(|_| SegmentError::Limit("records"))?;
    let mut rows = Vec::new();
    for term in terms {
        let mut hits = Vec::new();
        for hit in &term.hits {
            let mut positions = hit.positions.clone();
            positions.sort_unstable();
            hits.push(TermHit {
                doc: resolve_doc(&hit.doc, role, nodes, doc_count)?,
                positions,
            });
        }
        hits.sort_by_key(|hit| hit.doc);
        rows.push(TermRow {
            field: term.field,
            term_id: dict.id(&term.term)?,
            hits,
        });
    }
    rows.sort_by_key(|row| (row.field.code(), row.term_id));
    Ok(Some(Postings {
        lengths: lengths.to_vec(),
        rows,
    }))
}

fn resolve_doc(
    doc: &DocId,
    role: SegmentRole,
    nodes: &BTreeMap<&str, u32>,
    doc_count: u32,
) -> Result<u32, SegmentError> {
    let id = match doc {
        DocId::Index(id) => *id,
        DocId::Public(name) => {
            if role != SegmentRole::View {
                return Err(SegmentError::Postings("doc"));
            }
            *nodes
                .get(name.as_str())
                .ok_or(SegmentError::Postings("doc"))?
        }
    };
    if id >= doc_count {
        return Err(SegmentError::Postings("doc"));
    }
    Ok(id)
}

fn remember<'a>(
    set: &mut BTreeSet<&'a str>,
    value: &'a str,
    limits: &DecoderLimits,
) -> Result<(), SegmentError> {
    if usize::try_from(limits.max_string_bytes).is_ok_and(|max| value.len() > max) {
        return Err(SegmentError::Limit("string"));
    }
    set.insert(value);
    if u64::try_from(set.len()).unwrap_or(u64::MAX) > limits.max_records {
        return Err(SegmentError::Limit("records"));
    }
    Ok(())
}

fn push_dictionary(
    sections: &mut Vec<BuiltSection>,
    dict: &OwnedStrings,
) -> Result<(), SegmentError> {
    let (payload, count) = dictionary::encode(dict.as_slice())?;
    sections.push(section(SectionKind::Dictionary, true, count, payload));
    Ok(())
}

fn push_prefixes(
    sections: &mut Vec<BuiltSection>,
    dict: &OwnedStrings,
) -> Result<(), SegmentError> {
    let specs = prefix::sparse_first_char(dict.as_slice());
    if specs.is_empty() {
        return Ok(());
    }
    let (payload, count) = prefix::encode(&specs)?;
    sections.push(section(SectionKind::Prefix, false, count, payload));
    Ok(())
}

fn push_postings(
    sections: &mut Vec<BuiltSection>,
    postings: Option<Postings>,
    role: SegmentRole,
    limits: &DecoderLimits,
) -> Result<(), SegmentError> {
    let Some(postings) = postings else {
        return Ok(());
    };
    let (payload, count) = postings::encode(&postings, role, limits)?;
    sections.push(section(SectionKind::Postings, false, count, payload));
    Ok(())
}

fn section(kind: SectionKind, required: bool, record_count: u64, payload: Vec<u8>) -> BuiltSection {
    BuiltSection {
        kind: kind.code(),
        schema: 1,
        flags: if required { REQUIRED } else { 0 },
        record_count,
        payload,
    }
}
