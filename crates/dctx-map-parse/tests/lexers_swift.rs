// SPDX-License-Identifier: Apache-2.0
//! Swift fixture facts through the Parser port, without executing source.
//! Verifies SPEC-MAP-MODE sections 5 and 6.2: detection, precision and refusal.
//! Every `ParsedFile` is checked for store ownership, unique ids and valid spans.
#![forbid(unsafe_code)]

use dctx_map_model::{EdgeKind, NodeKind, SymbolKind};
use dctx_map_parse::{Language, ParseError, ParsedFile, Parser, PureParser};
use std::collections::BTreeSet;

const SWIFT: &[u8] = include_bytes!("fixtures/sample.swift");

/// Mirrors store validation, including the indexer's structural-node insertion.
#[allow(clippy::naive_bytecount, reason = "Standard iterator newline counts keep fixture and bounded 8 MiB corpus checks dependency-free; this is test validation rather than a parser hot path.")]
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
        assert!(node.id.as_str().starts_with(node.kind.letter()));
        assert!(node.lines[0] >= 1 && node.lines[1] >= node.lines[0]);
        assert!(node.lines[1] <= file_end, "{}", node.name);
        assert!(ids.insert(node.id.as_str().to_owned()), "{}", node.id);
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
        assert!(owned.contains(node.id.as_str()), "{}", node.name);
    }
    for reference in &facts.references {
        assert_eq!(reference.owner_path, facts.path);
        assert!(ids.contains(reference.src.as_str()));
        assert!(reference.lines[0] >= 1 && reference.lines[0] <= reference.lines[1]);
        assert!(reference.lines[1] <= file_end);
    }
}

fn checked_parse(path: &str, source: &[u8]) -> ParsedFile {
    let facts = PureParser.parse(path, source).unwrap();
    assert_store_accepts(&facts, source);
    assert_eq!(facts.language, Language::Swift);
    assert_eq!(facts.precision.coverage().as_str(), "structural");
    assert_eq!(facts.precision.completion().as_str(), "complete");
    assert_eq!(facts.precision.parser().as_str(), "dctx-lex-swift:1");
    let repeated = PureParser.parse(path, source).unwrap();
    assert_store_accepts(&repeated, source);
    assert_eq!(facts, repeated);
    #[cfg(feature = "syntax-tree-sitter")]
    {
        let oracle = dctx_map_parse::TreeSitterParser
            .parse(path, source)
            .unwrap();
        assert_store_accepts(&oracle, source);
        assert_eq!(facts, oracle);
    }
    facts
}

#[test]
fn extensions_select_swift_case_insensitively() {
    for extension in ["swift", "SWIFT", "Swift"] {
        assert_eq!(
            Language::for_path(&format!("Sources/App.{extension}")),
            Language::Swift
        );
    }
    assert_eq!(Language::for_path("Sources/App.swiftx"), Language::Unknown);
}

#[test]
fn fixture_counts_precision_and_determinism() {
    for extension in ["swift", "SWIFT", "Swift"] {
        let path = format!("Sources/App.{extension}");
        let facts = checked_parse(&path, SWIFT);
        assert_eq!(
            facts
                .nodes
                .iter()
                .map(|node| node.name.as_str())
                .collect::<Vec<_>>(),
            [&path, "Box", "Box::run", "helper"]
        );
        assert_eq!(facts.nodes.len(), 4);
        assert_eq!(facts.edges.len(), 3);
        assert_eq!(facts.references.len(), 2);
        assert_eq!(
            facts
                .references
                .iter()
                .map(|reference| (reference.kind, reference.name.as_str()))
                .collect::<Vec<_>>(),
            [
                (EdgeKind::Imports, "Foundation"),
                (EdgeKind::Calls, "helper")
            ]
        );
        assert_eq!(facts.references[0].alias, None);
        assert_eq!(facts.references[1].scope, "Box::run");
        assert_eq!(facts.references[1].src, facts.nodes[2].id);
        assert_eq!(facts.nodes[1].summary.text, "Runs a job.");
        assert_eq!(facts.nodes[1].symbol_kind, Some(SymbolKind::Struct));
        assert_eq!(facts.nodes[2].symbol_kind, Some(SymbolKind::Method));
        assert_eq!(facts.nodes[3].symbol_kind, Some(SymbolKind::Function));
    }
}

#[test]
fn partial_and_diagnostic_input_is_refused_with_positive_controls() {
    for bad in [
        b"struct Broken {".as_slice(),
        b"/* unterminated".as_slice(),
        b"let text = \"unterminated".as_slice(),
        b"let items = [1, 2".as_slice(),
        b"#externalMacro(module: \"Demo\", type: \"Macro\")".as_slice(),
    ] {
        for path in ["bad.swift", "bad.SWIFT"] {
            assert!(matches!(
                PureParser.parse(path, bad),
                Err(ParseError::Syntax { path: refused, line }) if refused == path && line >= 1
            ));
            #[cfg(feature = "syntax-tree-sitter")]
            assert!(matches!(
                dctx_map_parse::TreeSitterParser.parse(path, bad),
                Err(ParseError::Syntax { .. })
            ));
            checked_parse(path, SWIFT);
        }
    }
}

#[test]
fn empty_and_duplicate_declarations_remain_store_valid() {
    let empty = checked_parse("empty.swift", b"");
    assert_eq!(empty.nodes.len(), 1);
    assert!(empty.edges.is_empty() && empty.references.is_empty());
    let duplicate = checked_parse(
        "dup.swift",
        b"func run() { helper() }\nfunc run() { helper() }\nfunc helper() {}\n",
    );
    assert_ne!(duplicate.nodes[1].id, duplicate.nodes[2].id);
    assert_eq!(duplicate.references[0].src, duplicate.nodes[1].id);
    assert_eq!(duplicate.references[1].src, duplicate.nodes[2].id);
}

#[test]
fn typed_refusals_have_positive_controls() {
    assert!(matches!(
        PureParser.parse("bad.swift", &[0xff]),
        Err(ParseError::InvalidUtf8(_))
    ));
    assert!(matches!(
        PureParser.parse("../bad.swift", SWIFT),
        Err(ParseError::InvalidId(_))
    ));
    let oversized = vec![b' '; dctx_lex_swift::Limits::CEILING.max_bytes + 1];
    assert!(matches!(
        PureParser.parse("large.swift", &oversized),
        Err(ParseError::TooLarge)
    ));
    let deep = "{".repeat(dctx_lex_swift::Limits::CEILING.max_depth as usize + 1);
    assert!(matches!(
        PureParser.parse("deep.swift", deep.as_bytes()),
        Err(ParseError::Syntax { path, line: 1 }) if path == "deep.swift"
    ));
    checked_parse("good.swift", SWIFT);
}
