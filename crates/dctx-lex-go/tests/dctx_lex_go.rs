// SPDX-License-Identifier: Apache-2.0
//! Golden fixtures, abstentions and deterministic fuzz for the Go extractor.
//! Implements PARSERS-v1: partial facts for malformed input, no panic on arbitrary bytes.
#![forbid(unsafe_code)]

use dctx_lex_go::{Completion, Dialect, GoLexer, LexedFile, PARSER_ID, PARSER_REVISION, Precision};
use dctx_map_model::{EdgeKind, NodeKind, SummarySource, SymbolKind};

const BASIC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/basic.go"
));
const INCOMPLETE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/incomplete.go"
));

fn extract(path: &str, source: &str) -> LexedFile {
    GoLexer::default()
        .extract(path, source.as_bytes())
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn names(facts: &LexedFile) -> Vec<&str> {
    facts
        .nodes
        .iter()
        .map(|node| node.fact.name.as_str())
        .collect()
}

fn node<'a>(facts: &'a LexedFile, name: &str) -> &'a dctx_map_model::Node {
    &facts
        .nodes
        .iter()
        .find(|node| node.fact.name == name)
        .unwrap_or_else(|| panic!("missing {name} in {:?}", names(facts)))
        .fact
}

#[test]
fn golden_fixture_extracts_declarations_imports_and_references() {
    assert_eq!(PARSER_ID, "dctx-lex-go");
    assert_eq!(PARSER_REVISION, 1);
    let facts = extract("pkg/demo.go", BASIC);
    assert_eq!(facts.dialect, Dialect::Go);
    assert_eq!(
        facts.completion,
        Completion::Complete,
        "{:?}",
        facts.diagnostics
    );
    assert_eq!(facts.diagnostics, []);
    assert!(
        facts
            .nodes
            .iter()
            .all(|node| node.precision == Precision::Structural)
    );
    assert!(
        facts
            .edges
            .iter()
            .all(|edge| edge.precision == Precision::Structural)
    );
    assert!(
        facts
            .references
            .iter()
            .all(|reference| reference.precision == Precision::Structural)
    );
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    assert!(
        facts
            .nodes
            .iter()
            .all(|node| node.fact.fingerprint.len() == 64)
    );

    let got = names(&facts);
    let expect = [
        "pkg/demo.go",
        "demo",
        "Box",
        "Reader",
        "Reader::Read",
        "ID",
        "One",
        "Two",
        "Name",
        "Count",
        "Add",
        "Box::Serve",
        "main",
        "sKeep",
    ];
    assert_eq!(got, expect, "signatures: {:?}", signatures(&facts));

    let module = node(&facts, "pkg/demo.go");
    assert_eq!(module.kind, NodeKind::Module);
    assert_eq!(module.id.as_str(), "m:pkg/demo.go");
    assert_eq!(module.summary.text, "Package demo is a sample.");
    assert_eq!(module.summary.source, SummarySource::Doc);

    let package = node(&facts, "demo");
    assert_eq!(package.kind, NodeKind::Package);
    assert_eq!(package.id.as_str(), "k:pkg/demo.go");
    assert_eq!(package.symbol_kind, None);
    assert!(!package.public);
    assert_eq!(package.summary.text, "Package demo is a sample.");

    assert_symbol(
        &facts,
        "Box",
        Some(SymbolKind::Struct),
        true,
        "Box stores one value.",
    );
    assert_symbol(
        &facts,
        "Reader",
        Some(SymbolKind::Interface),
        true,
        "Reader reads bytes.",
    );
    assert_symbol(&facts, "Reader::Read", Some(SymbolKind::Method), true, "");
    assert_symbol(
        &facts,
        "ID",
        Some(SymbolKind::Type),
        true,
        "ID aliases int.",
    );
    assert_symbol(
        &facts,
        "One",
        Some(SymbolKind::Const),
        true,
        "One is first.",
    );
    let two = node(&facts, "Two");
    assert_eq!(two.symbol_kind, Some(SymbolKind::Const));
    assert!(two.public);
    assert_eq!(two.summary.source, SummarySource::Signature);
    assert_symbol(&facts, "Name", None, true, "");
    assert_symbol(&facts, "Count", None, true, "");
    assert_symbol(
        &facts,
        "Add",
        Some(SymbolKind::Function),
        true,
        "Add adds a and b.",
    );
    assert_eq!(node(&facts, "Add").signature, "func Add[T ~int](a, b T) T");
    assert_symbol(
        &facts,
        "Box::Serve",
        Some(SymbolKind::Method),
        true,
        "Serve handles a box.",
    );
    assert_eq!(
        node(&facts, "Box::Serve").signature,
        "func (s *Box[int]) Serve(r Reader) error"
    );
    assert_symbol(&facts, "main", Some(SymbolKind::Function), false, "");
    assert_symbol(&facts, "sKeep", Some(SymbolKind::Function), false, "");
    assert!(
        facts
            .nodes
            .iter()
            .all(|node| node.fact.name != "Decoy" && node.fact.name != "Hidden")
    );

    let read_id = node(&facts, "Reader::Read").id.as_str();
    let reader_id = node(&facts, "Reader").id.as_str();
    assert!(facts.edges.iter().any(|edge| {
        edge.fact.kind == EdgeKind::Contains
            && edge.fact.src.as_str() == reader_id
            && edge.fact.dst.as_str() == read_id
    }));

    let imports = refs(&facts, EdgeKind::Imports);
    assert_eq!(
        imports,
        vec![
            ("fmt".to_string(), None),
            ("example.com/alias".to_string(), Some("alias".to_string())),
            ("example.com/dot".to_string(), Some(".".to_string())),
            ("example.com/blank".to_string(), Some("_".to_string())),
            ("strings".to_string(), None),
            (
                "example.com/renamed".to_string(),
                Some("renamed".to_string())
            ),
        ]
    );
    assert_eq!(
        call_names(&facts),
        vec![
            "len",
            "fmt.Println",
            "strings.TrimSpace",
            "Add",
            "box.Serve"
        ]
    );
    let types = type_names(&facts);
    for required in [
        "T",
        "fmt.Stringer",
        "byte",
        "int",
        "error",
        "string",
        "Reader",
    ] {
        assert!(types.contains(&required), "{required} missing in {types:?}");
    }
    assert!(!call_names(&facts).iter().any(|name| name.contains('[')));
}

#[test]
fn incomplete_fixture_keeps_partial_facts() {
    let facts = extract("pkg/partial.go", INCOMPLETE);
    assert_eq!(facts.completion, Completion::Partial);
    assert!(
        facts
            .diagnostics
            .iter()
            .any(|d| d.reason == dctx_lex_go::DiagnosticReason::UnbalancedBracket)
    );
    assert!(
        facts
            .diagnostics
            .iter()
            .any(|d| d.reason == dctx_lex_go::DiagnosticReason::UnterminatedString)
    );
    for name in ["p", "Ready", "Broken", "Later", "tail"] {
        assert!(
            names(&facts).contains(&name),
            "{name} missing in {:?}",
            names(&facts)
        );
    }
    assert_eq!(node(&facts, "pkg/partial.go").kind, NodeKind::Module);
    assert_eq!(facts.nodes[0].precision, Precision::Structural);
    assert_eq!(node_precision(&facts, "Ready"), Precision::Structural);
    assert_eq!(node(&facts, "Ready").summary.text, "Ready is complete.");
    for name in ["Broken", "Later", "tail"] {
        assert_eq!(node_precision(&facts, name), Precision::Partial, "{name}");
    }
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    assert!(facts.nodes.iter().all(|node| node.fact.name != "Decoy"));
}

#[test]
fn ambiguous_selectors_and_composite_receivers_abstain() {
    let calls = extract(
        "p.go",
        "package p\nfunc f() {\n\tF[int](1)\n\tobj.Field\n\tobj.Method()\n}\n",
    );
    assert_eq!(
        calls.completion,
        Completion::Complete,
        "{:?}",
        calls.diagnostics
    );
    assert_eq!(call_names(&calls), vec!["obj.Method"]);

    let receiver = extract(
        "p.go",
        "package p\nfunc (s []byte) M() {}\nfunc (s *Box) Ok() {}\n",
    );
    assert_eq!(receiver.completion, Completion::Partial);
    assert!(
        names(&receiver).contains(&"Box::Ok"),
        "{:?}",
        names(&receiver)
    );
    assert!(!names(&receiver).iter().any(|name| name.ends_with('M')));
}

fn assert_symbol(
    facts: &LexedFile,
    name: &str,
    symbol: Option<SymbolKind>,
    public: bool,
    doc: &str,
) {
    let found = node(facts, name);
    assert_eq!(found.kind, NodeKind::Symbol, "{name}");
    assert_eq!(found.symbol_kind, symbol, "{name}");
    assert_eq!(found.public, public, "{name}");
    if !doc.is_empty() {
        assert_eq!(found.summary.source, SummarySource::Doc, "{name}");
        assert_eq!(found.summary.text, doc, "{name}");
    }
}

fn node_precision(facts: &LexedFile, name: &str) -> Precision {
    facts
        .nodes
        .iter()
        .find(|node| node.fact.name == name)
        .unwrap_or_else(|| panic!("missing {name}"))
        .precision
}

fn signatures(facts: &LexedFile) -> Vec<(&str, &str)> {
    facts
        .nodes
        .iter()
        .map(|node| (node.fact.name.as_str(), node.fact.signature.as_str()))
        .collect()
}

fn refs(facts: &LexedFile, kind: EdgeKind) -> Vec<(String, Option<String>)> {
    facts
        .references
        .iter()
        .filter(|reference| reference.fact.kind == kind)
        .map(|reference| (reference.fact.name.clone(), reference.fact.alias.clone()))
        .collect()
}

fn call_names(facts: &LexedFile) -> Vec<&str> {
    facts
        .references
        .iter()
        .filter(|reference| reference.fact.kind == EdgeKind::Calls)
        .map(|reference| reference.fact.name.as_str())
        .collect()
}

fn type_names(facts: &LexedFile) -> Vec<&str> {
    facts
        .references
        .iter()
        .filter(|reference| reference.fact.kind == EdgeKind::UsesType)
        .map(|reference| reference.fact.name.as_str())
        .collect()
}

fn file_lines(source: &str) -> u32 {
    u32::try_from(source.bytes().filter(|byte| *byte == b'\n').count())
        .expect("bounded test coordinate")
        + 1
}

fn check_invariants(source: &str, facts: &LexedFile) {
    let lines = file_lines(source);
    assert_ne!(facts.nodes, []);
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    assert!(facts.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
    if facts.diagnostics.is_empty() {
        assert_eq!(facts.completion, Completion::Complete);
    } else {
        assert_eq!(facts.completion, Completion::Partial);
    }
    for node in &facts.nodes {
        let [start, end] = node.fact.lines;
        assert!(
            start >= 1 && start <= end && end <= lines,
            "{start}..{end} of {lines}"
        );
        assert_eq!(node.fact.fingerprint.len(), 64);
    }
    for reference in &facts.references {
        let [start, end] = reference.fact.lines;
        assert!(start >= 1 && start <= end && end <= lines);
        assert_ne!(reference.fact.name, "");
    }
}

fn assert_stable(path: &str, bytes: &[u8]) {
    let lexer = GoLexer::default();
    let first = lexer.extract(path, bytes);
    let second = lexer.extract(path, bytes);
    match (&first, &second) {
        (Ok(left), Ok(right)) => {
            if let Ok(source) = std::str::from_utf8(bytes) {
                check_invariants(source, left);
            }
            assert_eq!(left, right);
        }
        (Err(left), Err(right)) => assert_eq!(left.to_string(), right.to_string()),
        _ => panic!("non-deterministic outcome for {path}"),
    }
}

fn xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

#[test]
#[allow(
    clippy::cast_possible_truncation,
    reason = "The fuzz length deliberately uses the PRNG's native low word before applying the 301-byte bound."
)]
fn arbitrary_bytes_and_truncated_fixtures_stay_bounded() {
    let mut state = 0x1234_5678_9abc_def0_u64;
    for _ in 0..200 {
        let len = (xorshift(&mut state) as usize) % 301;
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(xorshift(&mut state).to_le_bytes()[0]);
        }
        assert_stable("f.go", &bytes);
        assert_stable("f.rs", &bytes);
    }
    for source in [BASIC, INCOMPLETE] {
        for len in 0..=source.len() {
            let prefix = &source.as_bytes()[..len];
            let facts = GoLexer::default()
                .extract("f.go", prefix)
                .unwrap_or_else(|error| panic!("prefix {len}: {error}"));
            check_invariants(&source[..len], &facts);
            assert_stable("f.go", prefix);
        }
    }
}
