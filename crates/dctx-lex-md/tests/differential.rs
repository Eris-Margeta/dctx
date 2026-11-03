// SPDX-License-Identifier: Apache-2.0
//! Differential test against the tree-sitter Markdown adapter of dctx-map-parse (the oracle).
//! Checks decision 077 parity: doc ids, tree, lines, summaries, body fingerprints and references.
//! Uses dctx-map-parse only as a dev-dependency; the oracle fixture is shared, not copied.
use dctx_lex_md::extract;
use dctx_map_parse::{Parser, TreeSitterParser, body_fingerprint};

const FIXTURE: &[u8] = include_bytes!("../../dctx-map-parse/tests/fixtures/headings.md");

/// Inputs on which both extractors must agree exactly.
const CORPUS: &[&str] = &[
    "# A\n\nText with `code` and [link](docs/x.md).\n\n## B\n\nMore `x`.\n\n# C\n",
    "# One\n## Two\n### Three\nDeep body.\n## Four\n# Five\n",
    "Title\n=====\n\nFirst. Second.\n\nSub\n---\n\n`inner`\n",
    "# Code\n\n```rust\n# not a heading\n`nope`\n```\n\n~~~\n## also not\n~~~\n\n    # indented\n\nAfter `fence`.\n",
    "# Lists\n\n- item `x`\n- [y](src/y.rs)\n\n1. first `z`\n",
    "# Quote\n\n> quoted `q` and [r](r.md)\n",
    "# Multi\n\nline one\nline `two` here\nand [three](a/b.rs \"title\")\n",
    "intro `dropped`\n\n# Čuvaj Život\n\nUnicode `ključ`.\n\n# Čuvaj Život\n",
    "# Same\n\n# Same\n\n## Same\n",
    "#\n\nEmpty title.\n\n# 5.2 Stable IDs!\n",
    "# Images\n\n![alt](img/a.png) and ![b](<img/b c.png>)\n",
];

#[derive(Debug, PartialEq)]
struct DocNode {
    id: String,
    parent: String,
    signature: String,
    lines: [u32; 2],
    summary: String,
    fingerprint: String,
}

#[derive(Debug, PartialEq)]
struct Ref {
    src: String,
    name: String,
    lines: [u32; 2],
}

fn oracle(path: &str, bytes: &[u8]) -> (Vec<DocNode>, Vec<Ref>) {
    let parsed = TreeSitterParser.parse(path, bytes).unwrap();
    let parent = |id: &str| {
        parsed
            .edges
            .iter()
            .find(|edge| edge.dst.as_str() == id)
            .map(|edge| edge.src.to_string())
            .unwrap_or_default()
    };
    let nodes = parsed
        .nodes
        .iter()
        .skip(1)
        .map(|node| DocNode {
            id: node.id.to_string(),
            parent: parent(node.id.as_str()),
            signature: node.signature.clone(),
            lines: node.lines,
            summary: node.summary.text.clone(),
            fingerprint: node.fingerprint.clone(),
        })
        .collect();
    let refs = parsed
        .references
        .iter()
        .map(|reference| Ref {
            src: reference.src.to_string(),
            name: reference.name.clone(),
            lines: reference.lines,
        })
        .collect();
    (nodes, refs)
}

fn ours(path: &str, bytes: &[u8]) -> (Vec<DocNode>, Vec<Ref>) {
    let source = std::str::from_utf8(bytes).unwrap();
    let doc = extract(bytes).unwrap();
    let id = |index: usize| format!("d:{path}#{}", doc.stable_name(index).unwrap());
    let nodes = doc
        .headings
        .iter()
        .enumerate()
        .map(|(index, heading)| DocNode {
            id: id(index),
            parent: heading.parent.map_or(format!("m:{path}"), id),
            signature: heading.title.clone(),
            lines: heading.lines,
            summary: heading.summary.clone(),
            fingerprint: body_fingerprint(heading.body().text(source)),
        })
        .collect();
    let refs = doc
        .references
        .iter()
        .filter_map(|reference| {
            Some(Ref {
                src: id(reference.owner?),
                name: reference.name.clone(),
                lines: reference.lines,
            })
        })
        .collect();
    (nodes, refs)
}

#[test]
fn oracle_fixture_matches_tree_sitter_exactly() {
    assert_eq!(
        ours("docs/spec.md", FIXTURE),
        oracle("docs/spec.md", FIXTURE)
    );
}

#[test]
fn corpus_matches_tree_sitter_exactly() {
    for source in CORPUS {
        assert_eq!(
            ours("docs/case.md", source.as_bytes()),
            oracle("docs/case.md", source.as_bytes()),
            "differs on {source:?}"
        );
    }
}

#[test]
fn fixture_extraction_is_deterministic_and_complete() {
    let first = extract(FIXTURE).unwrap();
    assert_eq!(extract(FIXTURE).unwrap(), first);
    let levels: Vec<u8> = first.headings.iter().map(|h| h.level).collect();
    assert_eq!(levels, [1, 2, 3, 2, 1, 2]);
    assert_eq!(first.code_blocks.len(), 1);
    assert_eq!(first.front_matter, None);
}
