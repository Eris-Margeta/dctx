//! Default parser precision and the optional tree-sitter differential oracle.
//! Implements PARSERS-v1 coverage labels for the public Parser port.
//! Uses checked-in fixtures; the oracle module compiles only with syntax-tree-sitter.
#![forbid(unsafe_code)]
use dctx_map_model::{Completion, Coverage};
use dctx_map_parse::{Parser, PureParser};

#[test]
fn pure_parser_records_coverage_for_each_language_family() {
    let rust = PureParser.parse("lib.rs", b"fn valid() {}").unwrap();
    assert_eq!(rust.precision.coverage(), Coverage::Syntax);
    assert_eq!(rust.precision.completion(), &Completion::Complete);
    assert!(rust.precision.parser().as_str().starts_with("dctx-lex-rust/"));

    let script = PureParser.parse("view.tsx", b"export const View = () => 1;").unwrap();
    assert_eq!(script.language, dctx_map_parse::Language::Tsx);
    assert_eq!(script.precision.coverage(), Coverage::Structural);
    assert!(script.precision.parser().as_str().starts_with("dctx-lex-js"));

    let notes = PureParser.parse("notes.md", b"# Title\n\nBody.\n").unwrap();
    assert_eq!(notes.precision.coverage(), Coverage::Structural);
    assert_eq!(notes.precision.parser().as_str(), "dctx-lex-md:1");

    let other = PureParser.parse("notes.xyz", b"// Only a module.\n").unwrap();
    assert_eq!(other.precision.coverage(), Coverage::Opaque);
    assert_eq!(other.precision.completion().as_str(), "complete");
    assert_eq!(other.nodes.len(), 1);
}

#[cfg(feature = "syntax-tree-sitter")]
mod oracle {
    use super::*;
    use dctx_map_model::EdgeKind;
    use dctx_map_parse::TreeSitterParser;
    use std::collections::BTreeSet;

    fn ids(parse: &dyn Fn(&str, &[u8]) -> dctx_map_parse::ParsedFile, path: &str, bytes: &[u8]) -> BTreeSet<String> {
        parse(path, bytes)
            .nodes
            .iter()
            .map(|node| node.id.to_string())
            .collect()
    }

    #[test]
    fn pure_parser_matches_tree_sitter_fixture_ids() {
        let rust = include_bytes!("fixtures/rust/src/lib.rs");
        let markdown = include_bytes!("fixtures/headings.md");
        let javascript = include_bytes!("fixtures/js/module.mjs");
        for (path, bytes) in [
            ("src/lib.rs", rust.as_slice()),
            ("docs/spec.md", markdown.as_slice()),
            ("module.mjs", javascript.as_slice()),
        ] {
            let pure = PureParser.parse(path, bytes).unwrap();
            let oracle = TreeSitterParser.parse(path, bytes).unwrap();
            assert_eq!(
                ids(&|p, b| PureParser.parse(p, b).unwrap(), path, bytes),
                ids(&|p, b| TreeSitterParser.parse(p, b).unwrap(), path, bytes),
                "{path}"
            );
            let refs = |file: &dctx_map_parse::ParsedFile| {
                file.references
                    .iter()
                    .map(|reference| {
                        (
                            reference.kind,
                            reference.name.clone(),
                            reference.scope.clone(),
                            reference.alias.clone(),
                        )
                    })
                    .collect::<BTreeSet<_>>()
            };
            assert_eq!(refs(&pure), refs(&oracle), "{path}");
            assert!(pure.edges.iter().all(|edge| edge.kind == EdgeKind::Contains));
            let _ = oracle;
        }
    }
}
