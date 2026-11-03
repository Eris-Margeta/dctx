// SPDX-License-Identifier: Apache-2.0
//! Ruby fixture facts through the Parser port, without executing source.
//! Verifies SPEC-MAP-MODE sections 5 and 6.2: detection, precision and refusal.
//! Every `ParsedFile` is checked for store ownership, unique ids and valid spans.
#![forbid(unsafe_code)]

use dctx_map_model::{EdgeKind, NodeKind, SymbolKind};
use dctx_map_parse::{Language, ParseError, ParsedFile, Parser, PureParser};
use std::collections::BTreeSet;

const RUBY: &[u8] = include_bytes!("fixtures/sample.rb");

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
    assert_eq!(facts.language, Language::Ruby);
    assert_eq!(facts.precision.coverage().as_str(), "structural");
    assert_eq!(facts.precision.completion().as_str(), "complete");
    assert_eq!(facts.precision.parser().as_str(), "dctx-lex-rb:1");
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
fn extensions_select_ruby_case_insensitively() {
    for extension in ["rb", "rake", "gemspec", "RB", "RAKE", "GEMSPEC"] {
        assert_eq!(
            Language::for_path(&format!("src/sample.{extension}")),
            Language::Ruby
        );
    }
    assert_eq!(Language::for_path("src/sample.rbx"), Language::Unknown);
}

#[test]
fn fixture_counts_precision_and_determinism() {
    for extension in ["rb", "rake", "gemspec", "RB", "RAKE", "GEMSPEC"] {
        let path = format!("src/sample.{extension}");
        let facts = checked_parse(&path, RUBY);
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
        assert!(
            facts
                .edges
                .iter()
                .all(|edge| edge.kind == EdgeKind::Contains)
        );
        assert_eq!(facts.references.len(), 2);
        assert_eq!(
            facts
                .references
                .iter()
                .map(|reference| (reference.kind, reference.name.as_str()))
                .collect::<Vec<_>>(),
            [
                (EdgeKind::Imports, "demo/work"),
                (EdgeKind::Calls, "helper")
            ]
        );
        assert_eq!(facts.references[0].alias, None);
        assert_eq!(facts.references[1].scope, "Box::run");
        assert_eq!(facts.references[1].src, facts.nodes[2].id);
        assert_eq!(facts.nodes[0].summary.text, "Ruby adapter fixture.");
        assert_eq!(facts.nodes[1].summary.text, "Runs a job.");
        assert_eq!(facts.nodes[1].symbol_kind, Some(SymbolKind::Class));
        assert_eq!(facts.nodes[2].symbol_kind, Some(SymbolKind::Method));
        assert_eq!(facts.nodes[3].symbol_kind, Some(SymbolKind::Function));
    }
}

#[test]
fn partial_and_diagnostic_input_is_refused_with_positive_controls() {
    for bad in [
        b"class Broken\n".as_slice(),
        b"=begin\nunterminated".as_slice(),
        b"text = \"unterminated".as_slice(),
        b"items = [1, 2".as_slice(),
    ] {
        for path in ["bad.rb", "bad.rake", "bad.gemspec"] {
            assert!(matches!(
                PureParser.parse(path, bad),
                Err(ParseError::Syntax { path: refused, line }) if refused == path && line >= 1
            ));
            #[cfg(feature = "syntax-tree-sitter")]
            assert!(matches!(
                dctx_map_parse::TreeSitterParser.parse(path, bad),
                Err(ParseError::Syntax { .. })
            ));
            checked_parse(path, RUBY);
        }
    }
}

#[test]
fn empty_scripts_and_duplicate_declarations_remain_store_valid() {
    let empty = checked_parse("empty.rb", b"");
    assert_eq!(empty.nodes.len(), 1);
    assert!(empty.edges.is_empty() && empty.references.is_empty());
    let script = checked_parse("main.rake", b"#!/usr/bin/env ruby\ndef main; end\n");
    assert_eq!(script.nodes[1].name, "main");
    let duplicate = checked_parse("dup.rb", b"def run; end\ndef run; end\n");
    assert_eq!(duplicate.nodes[1].id.as_str(), "s:dup.rb#run~1");
    assert_eq!(duplicate.nodes[2].id.as_str(), "s:dup.rb#run~2");
}

#[test]
fn typed_refusals_have_positive_controls() {
    assert!(matches!(
        PureParser.parse("bad.rb", &[0xff]),
        Err(ParseError::InvalidUtf8(_))
    ));
    assert!(matches!(
        PureParser.parse("../bad.rb", RUBY),
        Err(ParseError::InvalidId(_))
    ));
    let oversized = vec![b' '; dctx_lex_rb::Limits::CEILING.max_bytes + 1];
    assert!(matches!(
        PureParser.parse("large.rb", &oversized),
        Err(ParseError::TooLarge)
    ));
    let deep = "{".repeat(dctx_lex_rb::Limits::CEILING.max_depth as usize + 1);
    assert!(matches!(
        PureParser.parse("deep.rb", deep.as_bytes()),
        Err(ParseError::Syntax { path, line: 1 }) if path == "deep.rb"
    ));
    checked_parse("good.rb", RUBY);
}
