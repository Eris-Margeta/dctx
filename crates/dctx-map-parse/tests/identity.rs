//! Identity, location, and fingerprint invariants through the public port.
//! Verifies SPEC-MAP-MODE section 5.2 across edits and duplicate items.
//! Uses proptest plus small Rust sources; consumed by the crate test suite.
#![forbid(unsafe_code)]
use dctx_map_model::EdgeKind;
use dctx_map_parse::{Parser, PureParser};
use proptest::prelude::*;
use std::collections::BTreeSet;

proptest! {
    #[test]
    fn javascript_arrows_preserve_ids_on_line_shifts_and_bodies_on_renames(
        name in "[a-z][a-z0-9_]{0,20}", blank_lines in 0usize..30,
    ) {
        for path in ["source.js", "source.ts"] {
            let source = format!("const item_{name} = () => work();");
            let original = PureParser.parse(path, source.as_bytes()).unwrap();
            let shifted = PureParser.parse(path,
                format!("{}{source}", "\n".repeat(blank_lines)).as_bytes()).unwrap();
            let renamed = PureParser.parse(path, b"const renamed = () => work();").unwrap();
            prop_assert_eq!(&original.nodes[1].id, &shifted.nodes[1].id);
            prop_assert_eq!(&original.nodes[1].fingerprint, &renamed.nodes[1].fingerprint);
            prop_assert_eq!(&original.references[0].src, &shifted.references[0].src);
            prop_assert_eq!(original.nodes[1].lines[0] + blank_lines as u32, shifted.nodes[1].lines[0]);
        }
    }

    #[test]
    fn line_shifts_preserve_identity_and_body_fingerprints(
        name in "[a-z][a-z0-9_]{0,20}", blank_lines in 0usize..30,
    ) {
        let source = format!("fn item_{name}() {{ call(); }}");
        let original = PureParser.parse("lib.rs", source.as_bytes()).unwrap();
        let shifted = PureParser.parse("lib.rs",
            format!("{}{source}", "\n".repeat(blank_lines)).as_bytes()).unwrap();
        prop_assert_eq!(&original.nodes[1].id, &shifted.nodes[1].id);
        prop_assert_eq!(&original.nodes[1].fingerprint, &shifted.nodes[1].fingerprint);
        prop_assert_eq!(original.nodes[1].lines[0] + blank_lines as u32, shifted.nodes[1].lines[0]);
        prop_assert_eq!(&original.references[0].src, &shifted.references[0].src);
    }

    #[test]
    fn duplicate_items_have_unique_ids_and_valid_edge_endpoints(count in 2usize..12) {
        let source = "impl Item { fn work() { call(); } }\n".repeat(count);
        let facts = PureParser.parse("lib.rs", source.as_bytes()).unwrap();
        let ids = facts.nodes.iter().map(|n| &n.id).collect::<BTreeSet<_>>();
        prop_assert_eq!(ids.len(), facts.nodes.len());
        for edge in &facts.edges {
            prop_assert!(ids.contains(&edge.src));
            prop_assert!(ids.contains(&edge.dst));
        }
        for reference in &facts.references { prop_assert!(ids.contains(&reference.src)); }
        let calls = facts.references.iter().filter(|r| r.kind == EdgeKind::Calls).collect::<Vec<_>>();
        for (index, call) in calls.iter().enumerate() {
            prop_assert_eq!(call.src.as_str(), format!("s:lib.rs#Item::work~{}", index + 1));
        }
    }
}

#[test]
fn renames_keep_body_fingerprints_and_paths_change_ids() {
    let a = PureParser
        .parse("a.rs", b"fn old_name() { action(); }")
        .unwrap();
    let b = PureParser
        .parse("b.rs", b"fn new_name() { action(); }")
        .unwrap();
    assert_ne!(a.nodes[1].id, b.nodes[1].id);
    assert_eq!(a.nodes[1].fingerprint, b.nodes[1].fingerprint);
    let a = PureParser.parse("a.rs", b"struct Before;").unwrap();
    let b = PureParser.parse("b.rs", b"struct After;").unwrap();
    assert_eq!(a.nodes[1].fingerprint, b.nodes[1].fingerprint);
}

#[test]
fn raw_identifiers_and_long_id_aliases_are_supported() {
    let source = b"fn r#match() {}";
    let facts = PureParser.parse("lib.rs", source).unwrap();
    assert_eq!(facts.nodes[1].id.as_str(), "s:lib.rs#match");
    let path = format!("{}/lib.rs", "a".repeat(121));
    let facts = PureParser.parse(&path, source).unwrap();
    assert!(facts.nodes[1].id.alias().is_some());
}
