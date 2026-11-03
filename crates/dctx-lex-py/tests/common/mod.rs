// SPDX-License-Identifier: Apache-2.0
//! Shared extraction, lookup and fact-invariant helpers for integration tests.
//! Covers the Python structural extractor contract (decision 077/decision 078).
//! Uses dctx-lex-py and dctx-map-model; fixtures remain inline in fixtures.rs.
#![forbid(unsafe_code)]

// Each test binary uses a different subset of the shared helpers and fixtures.
#![allow(dead_code)]

pub mod fixtures;

use dctx_lex_py::{body_fingerprint, Completion, Labeled, LexedFile, Precision, PyLexer};
use dctx_map_model::{Confidence, EdgeKind, Node, NodeKind};

pub fn extract(path: &str, source: &str) -> LexedFile {
    PyLexer::default()
        .extract(path, source.as_bytes())
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

pub fn line_of(source: &str, needle: &str) -> u32 {
    let byte = source
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle}"));
    u32::try_from(source[..byte].bytes().filter(|byte| *byte == b'\n').count()).expect("bounded test coordinate") + 1
}

pub fn last_line_of(source: &str, needle: &str) -> u32 {
    let byte = source
        .rfind(needle)
        .unwrap_or_else(|| panic!("missing {needle}"));
    u32::try_from(source[..byte].bytes().filter(|byte| *byte == b'\n').count()).expect("bounded test coordinate") + 1
}

pub fn node<'a>(facts: &'a LexedFile, name: &str) -> &'a Labeled<Node> {
    facts
        .nodes
        .iter()
        .find(|item| item.fact.name == name)
        .unwrap_or_else(|| panic!("missing {name}"))
}

pub fn names(facts: &LexedFile) -> Vec<&str> {
    facts.nodes.iter().map(|item| item.fact.name.as_str()).collect()
}

pub fn call_names(facts: &LexedFile) -> Vec<&str> {
    facts
        .references
        .iter()
        .filter(|item| item.fact.kind == EdgeKind::Calls)
        .map(|item| item.fact.name.as_str())
        .collect()
}

pub fn line_limit(source: &str) -> u32 {
    if source.contains('\r') {
        u32::MAX
    } else {
        u32::try_from(source.split_inclusive('\n').count().max(1)).expect("bounded test coordinate")
    }
}

pub fn check_invariants(source: &str, facts: &LexedFile) {
    assert_ne!(facts.nodes, []);
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    let module = &facts.nodes[0];
    assert_eq!(module.fact.kind, NodeKind::Module);
    assert!(module.fact.symbol_kind.is_none());
    assert!(!module.fact.public);
    assert_eq!(module.fact.name, facts.path);
    assert_eq!(module.fact.fingerprint, body_fingerprint(source));
    assert_eq!(module.precision, Precision::Structural);
    let limit = line_limit(source);
    let ids: Vec<&str> = facts.nodes.iter().map(|item| item.fact.id.as_str()).collect();
    for item in &facts.nodes {
        let [start, end] = item.fact.lines;
        assert!(start >= 1 && start <= end && end <= limit);
        assert_eq!(item.fact.path, facts.path);
    }
    for item in &facts.references {
        let [start, end] = item.fact.lines;
        assert!(start >= 1 && start <= end && end <= limit);
        assert!(ids.contains(&item.fact.src.as_str()));
        assert!(matches!(item.fact.kind, EdgeKind::Imports | EdgeKind::Calls));
        assert_eq!(item.fact.owner_path, facts.path);
    }
    for (index, edge) in facts.edges.iter().enumerate() {
        assert_eq!(edge.fact.kind, EdgeKind::Contains);
        assert_eq!(edge.fact.conf, Confidence::Exact);
        assert_eq!(edge.fact.owner_path, facts.path);
        assert_eq!(edge.fact.dst, facts.nodes[index + 1].fact.id);
        assert!(ids.contains(&edge.fact.src.as_str()));
        assert_eq!(edge.precision, facts.nodes[index + 1].precision);
    }
    assert!(facts.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
    let mut previous = 0u32;
    for diagnostic in &facts.diagnostics {
        assert!(diagnostic.byte >= previous);
        assert!(diagnostic.line >= 1);
        previous = diagnostic.byte;
    }
    if facts.diagnostics.is_empty() {
        assert_eq!(facts.completion, Completion::Complete);
        assert!(facts.nodes.iter().all(|item| item.precision != Precision::Partial));
        assert!(facts.references.iter().all(|item| item.precision != Precision::Partial));
    } else {
        assert_eq!(facts.completion, Completion::Partial);
    }
}
