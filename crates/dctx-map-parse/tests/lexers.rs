// SPDX-License-Identifier: Apache-2.0
//! Python, Go and Java fixture facts through the Parser port.
//! Verifies SPEC-MAP-MODE sections 5 and 6.2: language, counts, precision and refusal.
//! Checked-in source is parsed without executing code or reading dependencies.
//! Store rules: file ownership, unique ids, and spans inside the file.
#![forbid(unsafe_code)]

use dctx_map_model::{EdgeKind, NodeKind, SymbolKind};
use dctx_map_parse::{Language, ParseError, ParsedFile, Parser, PureParser};
use std::collections::BTreeSet;

const PYTHON: &[u8] = include_bytes!("fixtures/sample.py");
const GO: &[u8] = include_bytes!("fixtures/sample.go");
const JAVA: &[u8] = include_bytes!("fixtures/Sample.java");
const BASIC_GO: &[u8] = include_bytes!("../../dctx-lex-go/tests/fixtures/basic.go");
const BASIC_JAVA: &[u8] = include_bytes!("../../dctx-lex-java/tests/fixtures/basic.java");

fn names(facts: &ParsedFile) -> Vec<&str> {
    facts.nodes.iter().map(|node| node.name.as_str()).collect()
}

fn references(facts: &ParsedFile) -> Vec<(EdgeKind, &str, Option<&str>)> {
    facts
        .references
        .iter()
        .map(|reference| {
            (
                reference.kind,
                reference.name.as_str(),
                reference.alias.as_deref(),
            )
        })
        .collect()
}

fn assert_structural(facts: &ParsedFile, parser: &str) {
    assert_eq!(facts.precision.coverage().as_str(), "structural");
    assert_eq!(facts.precision.completion().as_str(), "complete");
    assert_eq!(facts.precision.parser().as_str(), parser);
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    assert!(
        facts
            .edges
            .iter()
            .all(|edge| edge.kind == EdgeKind::Contains)
    );
}

/// Same checks as `dctx_map_store` `validate`, plus spans inside the file.
/// Place and Package nodes are inserted again by the query indexer, so a file
/// that already contains one fails the unique-id rule.
#[allow(
    clippy::naive_bytecount,
    reason = "Standard iterator newline counts keep fixture and bounded 8 MiB corpus checks dependency-free; this is test validation rather than a parser hot path."
)]
fn assert_store_accepts(facts: &ParsedFile, source: &[u8]) {
    let file_end = u32::try_from(source.iter().filter(|byte| **byte == b'\n').count()).unwrap() + 1;
    let mut nodes = facts.nodes.clone();
    let again = facts
        .nodes
        .iter()
        .filter(|node| matches!(node.kind, NodeKind::Place | NodeKind::Package))
        .cloned()
        .collect::<Vec<_>>();
    nodes.extend(again);
    let mut ids = BTreeSet::new();
    assert_eq!(nodes[0].kind, NodeKind::Module, "{}", facts.path);
    for node in &nodes {
        let identity =
            node.id
                .as_str()
                .split_once(':')
                .map(|(_, rest)| match rest.split_once('#') {
                    Some((path, _)) => path,
                    None => rest,
                });
        assert_eq!(node.path, facts.path, "{}", node.name);
        assert_eq!(identity, Some(facts.path.as_str()), "{}", node.id);
        assert!(
            node.id.as_str().starts_with(node.kind.letter()),
            "{}",
            node.id
        );
        assert!(
            node.lines[0] >= 1 && node.lines[1] >= node.lines[0],
            "{}",
            node.name
        );
        assert!(
            node.lines[1] <= file_end,
            "{} spans past the file",
            node.name
        );
        assert!(
            ids.insert(node.id.as_str().to_owned()),
            "duplicate id {}",
            node.id
        );
    }
    let mut owned = BTreeSet::from([facts.nodes[0].id.as_str()]);
    loop {
        let before = owned.len();
        for edge in &facts.edges {
            assert_eq!(edge.owner_path, facts.path);
            assert!(edge.kind.permits(edge.conf));
            assert!(ids.contains(edge.src.as_str()), "{}", edge.src);
            assert!(ids.contains(edge.dst.as_str()), "{}", edge.dst);
            if edge.kind == EdgeKind::Contains && owned.contains(edge.src.as_str()) {
                owned.insert(edge.dst.as_str());
            }
        }
        if owned.len() == before {
            break;
        }
    }
    for node in &facts.nodes {
        assert!(
            owned.contains(node.id.as_str()),
            "{} is not owned by the file node",
            node.name
        );
    }
}

#[test]
fn extensions_select_python_go_and_java() {
    assert_eq!(Language::for_path("pkg/sample.py"), Language::Python);
    assert_eq!(Language::for_path("pkg/sample.PYI"), Language::Python);
    assert_eq!(Language::for_path("pkg/sample.go"), Language::Go);
    assert_eq!(Language::for_path("src/Sample.JAVA"), Language::Java);
    assert_eq!(Language::for_path("pkg/sample.pyw"), Language::Unknown);
}

#[test]
fn python_fixture_counts_precision_and_repeat() {
    let facts = PureParser.parse("pkg/sample.py", PYTHON).unwrap();
    assert_eq!(facts.language, Language::Python);
    assert_structural(&facts, "dctx-lex-py:1");
    assert_store_accepts(&facts, PYTHON);
    assert_eq!(
        names(&facts),
        ["pkg/sample.py", "LIMIT", "run", "Box", "Box::size"]
    );
    assert_eq!(facts.nodes.len(), 5);
    assert_eq!(facts.edges.len(), 4);
    assert_eq!(facts.references.len(), 3);
    assert_eq!(facts.nodes[2].kind, NodeKind::Symbol);
    assert_eq!(facts.nodes[2].symbol_kind, Some(SymbolKind::Function));
    assert_eq!(facts.nodes[3].symbol_kind, Some(SymbolKind::Class));
    assert_eq!(facts.nodes[4].symbol_kind, Some(SymbolKind::Method));
    assert_eq!(
        references(&facts),
        [
            (EdgeKind::Imports, "os", None),
            (EdgeKind::Calls, "os.getcwd", None),
            (EdgeKind::Calls, "run", None),
        ]
    );
    let stub = PureParser.parse("pkg/sample.pyi", PYTHON).unwrap();
    assert_eq!(stub.language, Language::Python);
    assert_eq!(stub.nodes.len(), facts.nodes.len());
    assert_eq!(stub.edges.len(), facts.edges.len());
    assert_eq!(stub.references.len(), facts.references.len());
    assert_structural(&stub, "dctx-lex-py:1");
    assert_store_accepts(&stub, PYTHON);
    assert_eq!(facts, PureParser.parse("pkg/sample.py", PYTHON).unwrap());
}

#[test]
fn go_fixture_counts_precision_and_repeat() {
    let facts = PureParser.parse("pkg/sample.go", GO).unwrap();
    assert_eq!(facts.language, Language::Go);
    assert_structural(&facts, "dctx-lex-go:1");
    assert_store_accepts(&facts, GO);
    assert_eq!(names(&facts), ["pkg/sample.go", "demo", "Greet"]);
    assert_eq!(facts.nodes[1].kind, NodeKind::Symbol);
    assert_eq!(facts.nodes[1].id.as_str(), "s:pkg/sample.go#demo");
    assert_eq!(facts.nodes.len(), 3);
    assert_eq!(facts.edges.len(), 2);
    assert_eq!(facts.references.len(), 4);
    assert_eq!(facts.nodes[2].symbol_kind, Some(SymbolKind::Function));
    assert!(facts.nodes[2].public);
    assert_eq!(
        references(&facts),
        [
            (EdgeKind::Imports, "fmt", None),
            (EdgeKind::UsesType, "string", None),
            (EdgeKind::UsesType, "string", None),
            (EdgeKind::Calls, "fmt.Sprint", None),
        ]
    );
    assert_eq!(facts, PureParser.parse("pkg/sample.go", GO).unwrap());
}

#[test]
fn java_fixture_counts_precision_and_repeat() {
    let facts = PureParser.parse("src/Sample.java", JAVA).unwrap();
    assert_eq!(facts.language, Language::Java);
    assert_structural(&facts, "dctx-lex-java:2");
    assert_store_accepts(&facts, JAVA);
    assert_eq!(names(&facts), ["src/Sample.java", "Box", "Box::run"]);
    assert_eq!(facts.nodes.len(), 3);
    assert_eq!(facts.edges.len(), 2);
    assert_eq!(facts.references.len(), 2);
    assert_eq!(facts.nodes[1].symbol_kind, Some(SymbolKind::Class));
    assert_eq!(facts.nodes[2].symbol_kind, Some(SymbolKind::Method));
    assert_eq!(
        references(&facts),
        [
            (EdgeKind::Imports, "java.util.List", None),
            (EdgeKind::Calls, "List.of", None),
        ]
    );
    assert_eq!(facts, PureParser.parse("src/Sample.java", JAVA).unwrap());
}

#[test]
fn repository_fixtures_keep_store_ownership_identity_and_spans() {
    let go = PureParser
        .parse("crates/dctx-lex-go/tests/fixtures/basic.go", BASIC_GO)
        .unwrap();
    assert_store_accepts(&go, BASIC_GO);
    assert!(go.nodes.iter().all(|node| node.kind != NodeKind::Package));
    let java = PureParser
        .parse("crates/dctx-lex-java/tests/fixtures/basic.java", BASIC_JAVA)
        .unwrap();
    assert_store_accepts(&java, BASIC_JAVA);
}

#[test]
fn partial_input_is_refused_and_the_fixture_is_accepted() {
    assert!(matches!(
        PureParser.parse("bad.py", b"def broken("),
        Err(ParseError::Syntax { line: 1, .. })
    ));
    assert!(PureParser.parse("pkg/sample.py", PYTHON).is_ok());
    assert!(matches!(
        PureParser.parse("bad.go", b"package p\nfunc Broken("),
        Err(ParseError::Syntax { .. })
    ));
    assert!(PureParser.parse("pkg/sample.go", GO).is_ok());
    assert!(matches!(
        PureParser.parse("Bad.java", b"class A { void m() {"),
        Err(ParseError::Syntax { .. })
    ));
    assert!(PureParser.parse("src/Sample.java", JAVA).is_ok());
}
