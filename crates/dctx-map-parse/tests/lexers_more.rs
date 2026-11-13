// SPDX-License-Identifier: Apache-2.0
//! C/C++, configuration and C# fixture facts through the Parser port.
//! Verifies SPEC-MAP-MODE sections 5 and 6.2: detection, precision and refusal.
//! Uses checked-in source without executing it or reading dependencies.
//! Store rules: file ownership, unique ids, and spans inside the file.
#![forbid(unsafe_code)]

use dctx_map_model::{EdgeKind, NodeKind, SymbolKind};
use dctx_map_parse::{Language, ParseError, ParsedFile, Parser, PureParser};
use std::collections::BTreeSet;

const C: &[u8] = include_bytes!("fixtures/sample.c");
const TOML: &[u8] = include_bytes!("fixtures/sample.toml");
const YAML: &[u8] = include_bytes!("fixtures/sample.yaml");
const CS: &[u8] = include_bytes!("fixtures/Sample.cs");

/// Same checks as `dctx_map_store` validation, plus spans inside the file.
/// Place and Package nodes are inserted again by the query indexer.
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
    for reference in &facts.references {
        assert_eq!(reference.owner_path, facts.path);
        assert!(ids.contains(reference.src.as_str()));
        assert!(reference.lines[0] >= 1 && reference.lines[0] <= reference.lines[1]);
        assert!(reference.lines[1] <= file_end);
    }
}

fn check(
    path: &str,
    source: &[u8],
    language: Language,
    parser: &str,
    names: &[&str],
    references: &[(EdgeKind, &str)],
) -> ParsedFile {
    let facts = PureParser.parse(path, source).unwrap();
    assert_eq!(facts.language, language);
    assert_eq!(facts.precision.coverage().as_str(), "structural");
    assert_eq!(facts.precision.completion().as_str(), "complete");
    assert_eq!(facts.precision.parser().as_str(), parser);
    assert_eq!(
        facts
            .nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect::<Vec<_>>(),
        names
    );
    assert_eq!(facts.nodes.len(), names.len());
    assert_eq!(facts.edges.len(), names.len() - 1);
    assert!(
        facts
            .edges
            .iter()
            .all(|edge| edge.kind == EdgeKind::Contains)
    );
    assert_eq!(facts.references.len(), references.len());
    assert_eq!(
        facts
            .references
            .iter()
            .map(|r| (r.kind, r.name.as_str()))
            .collect::<Vec<_>>(),
        references
    );
    assert_store_accepts(&facts, source);
    assert_eq!(facts, PureParser.parse(path, source).unwrap());
    #[cfg(feature = "syntax-tree-sitter")]
    assert_eq!(
        facts,
        dctx_map_parse::TreeSitterParser
            .parse(path, source)
            .unwrap()
    );
    facts
}

#[test]
fn extensions_select_c_config_and_csharp_case_insensitively() {
    for extension in ["c", "h", "cc", "cpp", "cxx", "hpp", "hh"] {
        for ext in [extension.to_owned(), extension.to_ascii_uppercase()] {
            assert_eq!(
                Language::for_path(&format!("src/sample.{ext}")),
                Language::C
            );
        }
    }
    for extension in ["json", "jsonc", "toml", "yaml", "yml"] {
        for ext in [extension.to_owned(), extension.to_ascii_uppercase()] {
            assert_eq!(
                Language::for_path(&format!("cfg/sample.{ext}")),
                Language::Config
            );
        }
    }
    assert_eq!(Language::for_path("src/Sample.CS"), Language::CSharp);
    assert_eq!(Language::for_path("src/Sample.csx"), Language::Unknown);
}

#[test]
fn c_and_cpp_counts_precision_and_repeat() {
    for extension in ["c", "h", "cc", "cpp", "cxx", "hpp", "hh", "CPP"] {
        let path = format!("src/sample.{extension}");
        let facts = check(
            &path,
            C,
            Language::C,
            "dctx-lex-c:1",
            &[&path, "run"],
            &[
                (EdgeKind::Imports, "box.h"),
                (EdgeKind::UsesType, "int"),
                (EdgeKind::UsesType, "void"),
            ],
        );
        assert_eq!(facts.nodes[1].symbol_kind, Some(SymbolKind::Function));
    }
}

#[test]
fn toml_and_yaml_counts_precision_and_repeat() {
    for (path, source) in [
        ("cfg/sample.toml", TOML),
        ("cfg/sample.yaml", YAML),
        ("cfg/sample.YML", YAML),
    ] {
        check(
            path,
            source,
            Language::Config,
            "dctx-lex-cfg:1",
            &[path, "package", "package.name"],
            &[],
        );
    }
}

#[test]
fn json_and_jsonc_route_to_the_config_lexer() {
    for (path, source) in [
        ("cfg/sample.json", b"{\"name\":\"demo\"}".as_slice()),
        (
            "cfg/sample.JSONC",
            b"{/* comment */ \"name\":\"demo\"}".as_slice(),
        ),
    ] {
        check(
            path,
            source,
            Language::Config,
            "dctx-lex-cfg:1",
            &[path, "name"],
            &[],
        );
    }
}

#[test]
fn csharp_counts_precision_and_repeat() {
    let facts = check(
        "src/Sample.cs",
        CS,
        Language::CSharp,
        "dctx-lex-cs:3",
        &["src/Sample.cs", "Box"],
        &[(EdgeKind::Imports, "System")],
    );
    assert_eq!(facts.nodes[1].symbol_kind, Some(SymbolKind::Class));
    assert!(facts.nodes[1].public);
}

#[test]
fn partial_input_is_refused_with_positive_controls() {
    for (path, bad, good) in [
        ("bad.c", b"int broken(void) {".as_slice(), C),
        ("bad.cpp", b"class Box {".as_slice(), C),
        ("bad.toml", b"[package".as_slice(), TOML),
        ("bad.yaml", b"name: [".as_slice(), YAML),
        ("Bad.cs", b"class Box {".as_slice(), CS),
        ("bad.json", b"{\"name\":".as_slice(), b"{}".as_slice()),
        ("bad.jsonc", b"/* open".as_slice(), b"{}".as_slice()),
    ] {
        assert!(
            matches!(PureParser.parse(path, bad), Err(ParseError::Syntax { .. })),
            "{path}"
        );
        let facts = PureParser.parse(path, good).unwrap();
        assert_store_accepts(&facts, good);
    }
}
