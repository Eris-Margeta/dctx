//! Golden fixtures, store invariants and deterministic fuzz for the C extractor.
//! Implements PARSERS-v1: partial facts for malformed input, no panic on arbitrary bytes.
//! Every node is owned by the file module and every span lies inside the file.
#![forbid(unsafe_code)]

use dctx_lex_c::{
    Completion, DiagnosticReason, Dialect, LexedFile, Precision, CLexer, PARSER_ID, PARSER_REVISION,
};
use dctx_map_model::{EdgeKind, NodeKind, SummarySource, SymbolKind};
use std::collections::BTreeSet;

const BASIC: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/basic.c"));
const HEADER: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/basic.hpp"));
const INCOMPLETE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/incomplete.c"
));

fn extract(path: &str, source: &str) -> LexedFile {
    CLexer::default()
        .extract(path, source.as_bytes())
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn names(facts: &LexedFile) -> Vec<&str> {
    facts.nodes.iter().map(|node| node.fact.name.as_str()).collect()
}

fn node<'a>(facts: &'a LexedFile, name: &str) -> &'a dctx_map_model::Node {
    &facts
        .nodes
        .iter()
        .find(|node| node.fact.name == name)
        .unwrap_or_else(|| panic!("missing {name} in {:?}", names(facts)))
        .fact
}

fn named<'a>(facts: &'a LexedFile, name: &str) -> Vec<&'a dctx_map_model::Node> {
    facts
        .nodes
        .iter()
        .filter(|node| node.fact.name == name)
        .map(|node| &node.fact)
        .collect()
}

fn assert_symbol(facts: &LexedFile, name: &str, symbol: Option<SymbolKind>, public: bool, doc: &str) {
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

fn refs<'a>(facts: &'a LexedFile, kind: EdgeKind) -> Vec<(&'a str, Precision)> {
    facts
        .references
        .iter()
        .filter(|reference| reference.fact.kind == kind)
        .map(|reference| (reference.fact.name.as_str(), reference.precision))
        .collect()
}

fn file_lines(source: &str) -> u32 {
    source.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1
}

fn check_invariants(path: &str, source: &str, facts: &LexedFile) {
    let lines = file_lines(source);
    assert!(!facts.nodes.is_empty());
    assert_eq!(facts.nodes[0].fact.kind, NodeKind::Module);
    assert_eq!(facts.nodes[0].fact.name, path);
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    assert!(facts.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
    if facts.diagnostics.is_empty() {
        assert_eq!(facts.completion, Completion::Complete);
    } else {
        assert_eq!(facts.completion, Completion::Partial);
    }
    let mut ids = BTreeSet::new();
    for node in &facts.nodes {
        assert_eq!(node.fact.path, path, "{}", node.fact.name);
        assert!(node.fact.id.as_str().starts_with(node.fact.kind.letter()), "{}", node.fact.id);
        let identity = node.fact.id.as_str().split_once(':').map(|(_, rest)| match rest.split_once('#') {
            Some((file, _)) => file,
            None => rest,
        });
        assert_eq!(identity, Some(path), "{}", node.fact.id);
        let [start, end] = node.fact.lines;
        let name = &node.fact.name;
        assert!(start >= 1 && start <= end && end <= lines, "{name} {start}..{end} of {lines}");
        assert_eq!(node.fact.fingerprint.len(), 64);
        assert!(ids.insert(node.fact.id.as_str().to_owned()), "duplicate {}", node.fact.id);
        assert!(!matches!(node.fact.kind, NodeKind::Place | NodeKind::Package));
    }
    let mut owned = BTreeSet::from([facts.nodes[0].fact.id.as_str()]);
    loop {
        let before = owned.len();
        for edge in &facts.edges {
            assert_eq!(edge.fact.owner_path, path);
            assert!(edge.fact.kind.permits(edge.fact.conf));
            assert!(ids.contains(edge.fact.src.as_str()), "{}", edge.fact.src);
            assert!(ids.contains(edge.fact.dst.as_str()), "{}", edge.fact.dst);
            if edge.fact.kind == EdgeKind::Contains && owned.contains(edge.fact.src.as_str()) {
                owned.insert(edge.fact.dst.as_str());
            }
        }
        if owned.len() == before {
            break;
        }
    }
    for node in &facts.nodes {
        assert!(owned.contains(node.fact.id.as_str()), "{} is not owned by the file", node.fact.name);
    }
    for reference in &facts.references {
        let [start, end] = reference.fact.lines;
        assert!(start >= 1 && start <= end && end <= lines, "{}", reference.fact.name);
        assert_eq!(reference.fact.owner_path, path);
        assert!(ids.contains(reference.fact.src.as_str()), "{}", reference.fact.src);
        assert_ne!(reference.fact.kind, EdgeKind::Contains);
        assert!(!reference.fact.name.is_empty());
    }
}

fn assert_stable(path: &str, bytes: &[u8]) {
    let lexer = CLexer::default();
    let first = lexer.extract(path, bytes);
    let second = lexer.extract(path, bytes);
    match (&first, &second) {
        (Ok(left), Ok(right)) => {
            if let Ok(source) = std::str::from_utf8(bytes) {
                check_invariants(path, source, left);
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
fn golden_c_fixture_extracts_includes_macros_and_calls() {
    assert_eq!(PARSER_ID, "dctx-lex-c");
    assert_eq!(PARSER_REVISION, 1);
    let facts = extract("basic.c", BASIC);
    assert_eq!(facts.dialect, Dialect::C);
    assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
    check_invariants("basic.c", BASIC, &facts);
    let expect = [
        "basic.c",
        "LIMIT",
        "SHOW",
        "configured",
        "add",
        "Box",
        "Box::width",
        "Box::height",
        "Color",
        "Color::Red",
        "Color::Blue",
        "Word",
        "Word::as_int",
        "Word::bytes",
        "hidden_fn",
        "splice",
        "main",
    ];
    assert_eq!(names(&facts), expect);
    assert_eq!(node(&facts, "basic.c").summary.text, "File demo is a sample.");
    assert_eq!(node(&facts, "basic.c").summary.source, SummarySource::Doc);
    assert_symbol(&facts, "LIMIT", Some(SymbolKind::Macro), true, "");
    assert_symbol(&facts, "SHOW", Some(SymbolKind::Macro), true, "");
    assert_symbol(&facts, "configured", Some(SymbolKind::Function), true, "");
    assert_eq!(node_precision(&facts, "configured"), Precision::Convention);
    assert_symbol(&facts, "add", Some(SymbolKind::Function), true, "Add adds two ints.");
    assert_eq!(node_precision(&facts, "add"), Precision::Structural);
    assert_symbol(&facts, "Box", Some(SymbolKind::Struct), true, "");
    assert_eq!(names(&facts).iter().filter(|name| **name == "Box").count(), 1);
    assert_symbol(&facts, "Box::width", None, true, "");
    assert_symbol(&facts, "Color", Some(SymbolKind::Enum), true, "");
    assert_symbol(&facts, "Color::Red", Some(SymbolKind::Const), true, "");
    assert_symbol(&facts, "Word", Some(SymbolKind::Type), true, "");
    assert!(node(&facts, "Word").signature.contains("union Word"));
    assert_symbol(&facts, "hidden_fn", Some(SymbolKind::Function), false, "");
    assert_symbol(&facts, "splice", Some(SymbolKind::Function), true, "");
    assert_symbol(&facts, "main", Some(SymbolKind::Function), true, "");
    assert!(!names(&facts).iter().any(|name| {
        matches!(*name, "decoy" | "Hidden" | "hidden_in_comment" | "spl" | "ice")
    }));
    assert_eq!(
        refs(&facts, EdgeKind::Imports),
        [
            ("stdio.h", Precision::Structural),
            ("box.h", Precision::Structural),
            ("extra.h", Precision::Convention),
        ]
    );
    assert_eq!(
        refs(&facts, EdgeKind::Calls),
        [("add", Precision::Structural), ("printf", Precision::Structural)]
    );
    let members = refs(&facts, EdgeKind::UsesType)
        .into_iter()
        .filter(|(name, _)| name.contains('.') || name.contains("->"))
        .collect::<Vec<_>>();
    assert_eq!(
        members,
        [
            ("box.width", Precision::Convention),
            ("node->width", Precision::Convention),
            ("box.width", Precision::Convention),
        ]
    );
}

#[test]
fn golden_header_extracts_namespace_templates_and_methods() {
    let facts = extract("basic.hpp", HEADER);
    assert_eq!(facts.dialect, Dialect::Cpp);
    assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
    check_invariants("basic.hpp", HEADER, &facts);
    let expect = [
        "basic.hpp",
        "API",
        "demo",
        "demo::Box",
        "demo::Box::Box",
        "demo::Box::area",
        "demo::Box::value",
        "demo::Box::hidden",
        "demo::Box::area",
        "demo::Name",
        "demo::Kind",
        "demo::Kind::One",
        "demo::Kind::Two",
        "demo::Bits",
        "demo::Bits::raw",
        "demo::Bits::bytes",
        "demo::run",
    ];
    assert_eq!(names(&facts), expect, "{:?}", signatures(&facts));
    assert_eq!(node(&facts, "basic.hpp").summary.text, "Shapes live in demo.");
    assert_symbol(&facts, "API", Some(SymbolKind::Macro), true, "");
    assert_symbol(&facts, "demo", None, true, "");
    assert!(node(&facts, "demo").signature.starts_with("namespace"));
    assert_symbol(&facts, "demo::Box", Some(SymbolKind::Class), true, "A boxed value.");
    assert!(node(&facts, "demo::Box").signature.contains("template"));
    assert!(node(&facts, "demo::Box").signature.contains("class Box"));
    assert_symbol(&facts, "demo::Box::Box", Some(SymbolKind::Method), true, "");
    assert_symbol(&facts, "demo::Box::value", None, true, "");
    assert_symbol(&facts, "demo::Box::hidden", None, false, "");
    let areas = named(&facts, "demo::Box::area");
    assert_eq!(areas.len(), 2);
    assert!(areas.iter().all(|area| area.symbol_kind == Some(SymbolKind::Method)));
    assert_eq!(areas[1].summary.text, "Area multiplies the sides.");
    assert_eq!(areas[1].summary.source, SummarySource::Doc);
    assert!(facts.nodes.iter().any(|node| node.fact.id.as_str() == "s:basic.hpp#demo::Box::area~1"));
    assert!(facts.nodes.iter().any(|node| node.fact.id.as_str() == "s:basic.hpp#demo::Box::area~2"));
    let class_id = node(&facts, "demo::Box").id.as_str();
    let demo_id = node(&facts, "demo").id.as_str();
    let inline_id = facts.nodes.iter().find(|node| node.fact.name == "demo::Box::area").unwrap().fact.id.as_str();
    let outline_id = facts
        .nodes
        .iter()
        .filter(|node| node.fact.name == "demo::Box::area")
        .nth(1)
        .unwrap()
        .fact
        .id
        .as_str();
    assert!(facts.edges.iter().any(|edge| edge.fact.src.as_str() == class_id && edge.fact.dst.as_str() == inline_id));
    assert!(facts.edges.iter().any(|edge| edge.fact.src.as_str() == demo_id && edge.fact.dst.as_str() == outline_id));
    assert_symbol(&facts, "demo::Name", Some(SymbolKind::Type), true, "");
    assert_symbol(&facts, "demo::Kind", Some(SymbolKind::Enum), true, "");
    assert_symbol(&facts, "demo::Kind::One", Some(SymbolKind::Const), true, "");
    assert_symbol(&facts, "demo::Bits", Some(SymbolKind::Type), true, "");
    assert!(node(&facts, "demo::Bits").signature.contains("union"));
    assert_symbol(&facts, "demo::run", Some(SymbolKind::Function), true, "");
    assert_eq!(refs(&facts, EdgeKind::Imports), [("string", Precision::Structural), ("box.h", Precision::Structural)]);
    assert_eq!(refs(&facts, EdgeKind::Calls), [("box.area", Precision::Structural)]);
    assert!(facts.references.iter().any(|reference| {
        reference.fact.kind == EdgeKind::UsesType && reference.fact.name == "std::string"
    }));
    assert!(facts.references.iter().any(|reference| {
        reference.fact.kind == EdgeKind::UsesType
            && reference.fact.name == "node->value"
            && reference.precision == Precision::Convention
    }));
    assert!(!names(&facts).iter().any(|name| name.contains("Hidden") || name.contains("fake")));
    assert!(!refs(&facts, EdgeKind::Calls).iter().any(|(name, _)| *name == "fake" || *name == "Hidden"));
}

#[test]
fn incomplete_fixture_keeps_partial_facts() {
    let facts = extract("incomplete.c", INCOMPLETE);
    check_invariants("incomplete.c", INCOMPLETE, &facts);
    assert!(facts.diagnostics.iter().any(|d| d.reason == DiagnosticReason::UnbalancedBracket));
    assert!(facts.diagnostics.iter().any(|d| d.reason == DiagnosticReason::UnterminatedString));
    assert_eq!(names(&facts), ["incomplete.c", "ready", "broken", "later", "tail"]);
    assert_eq!(facts.nodes[0].precision, Precision::Structural);
    assert_eq!(node_precision(&facts, "ready"), Precision::Structural);
    assert_eq!(node(&facts, "ready").summary.text, "Ready is complete.");
    for name in ["broken", "later", "tail"] {
        assert_eq!(node_precision(&facts, name), Precision::Partial, "{name}");
    }
    assert_eq!(node(&facts, "tail").symbol_kind, None);
    assert_eq!(node(&facts, "later").symbol_kind, Some(SymbolKind::Function));
}

#[test]
fn macro_names_and_macro_introduced_types_abstain() {
    let source = "#define NAME name\nint NAME(void);\n#define STRUCT struct\nSTRUCT Hidden { int a; };\n";
    let facts = extract("hide.c", source);
    check_invariants("hide.c", source, &facts);
    assert_eq!(facts.completion, Completion::Partial);
    assert!(facts.diagnostics.iter().any(|d| d.reason == DiagnosticReason::HiddenByMacro));
    assert_eq!(names(&facts), ["hide.c", "NAME", "STRUCT"]);
    assert!(facts.nodes.iter().all(|node| node.fact.symbol_kind != Some(SymbolKind::Function)));
    assert!(facts.nodes.iter().all(|node| node.fact.symbol_kind != Some(SymbolKind::Struct)));
}

fn signatures(facts: &LexedFile) -> Vec<(&str, &str)> {
    facts
        .nodes
        .iter()
        .map(|node| (node.fact.name.as_str(), node.fact.signature.as_str()))
        .collect()
}

#[test]
fn arbitrary_bytes_and_truncated_fixtures_stay_bounded() {
    let mut state = 0x1234_5678_9abc_def0_u64;
    for _ in 0..200 {
        let len = (xorshift(&mut state) as usize) % 301;
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(xorshift(&mut state) as u8);
        }
        assert_stable("f.c", &bytes);
        assert_stable("f.rs", &bytes);
    }
    for (path, source) in [("basic.c", BASIC), ("basic.hpp", HEADER), ("incomplete.c", INCOMPLETE)] {
        for len in 0..=source.len() {
            let prefix = &source.as_bytes()[..len];
            let facts = CLexer::default()
                .extract(path, prefix)
                .unwrap_or_else(|error| panic!("{path} prefix {len}: {error}"));
            check_invariants(path, &source[..len], &facts);
            assert_stable(path, prefix);
        }
    }
}
