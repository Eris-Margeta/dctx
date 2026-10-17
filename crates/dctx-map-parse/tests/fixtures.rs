//! Golden per-file graph facts for a Rust crate and Markdown document.
//! Checks SPEC-MAP-MODE sections 5.1–5.3 and 6.2 through the Parser port.
//! Uses checked-in fixtures; no filesystem writes or compiler invocation.
#![forbid(unsafe_code)]
use dctx_map_model::{Confidence, EdgeKind, NodeKind, SymbolKind};
use dctx_map_parse::{ParsedFile, Parser, PureParser};

const RUST: &[u8] = include_bytes!("fixtures/rust/src/lib.rs");
const SUPPORT: &[u8] = include_bytes!("fixtures/rust/src/support.rs");
const MARKDOWN: &[u8] = include_bytes!("fixtures/headings.md");

fn nodes(facts: &ParsedFile) -> Vec<(&str, NodeKind, Option<SymbolKind>)> {
    facts
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.kind, n.symbol_kind))
        .collect()
}

#[test]
fn rust_fixture_has_the_exact_node_set() {
    use NodeKind::{Module, Symbol, Test};
    use SymbolKind::{Const, Enum, Function, Impl, Macro, Method, Struct, Trait, Type};
    let facts = PureParser.parse("src/lib.rs", RUST).unwrap();
    assert_eq!(
        nodes(&facts),
        [
            ("m:src/lib.rs", Module, None),
            ("s:src/lib.rs#Widget", Symbol, Some(Struct)),
            ("s:src/lib.rs#Mode", Symbol, Some(Enum)),
            ("s:src/lib.rs#Work", Symbol, Some(Trait)),
            ("s:src/lib.rs#Work::run", Symbol, Some(Method)),
            ("s:src/lib.rs#Widget::impl<Work>", Symbol, Some(Impl)),
            ("s:src/lib.rs#Widget::run", Symbol, Some(Method)),
            ("s:src/lib.rs#Widget::impl", Symbol, Some(Impl)),
            ("s:src/lib.rs#Widget::value", Symbol, Some(Method)),
            ("s:src/lib.rs#LIMIT", Symbol, Some(Const)),
            ("s:src/lib.rs#Alias", Symbol, Some(Type)),
            ("s:src/lib.rs#local_value", Symbol, Some(Macro)),
            ("s:src/lib.rs#tests", Symbol, None),
            ("t:src/lib.rs#tests::checks", Test, Some(Function)),
        ]
    );
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    for edge in &facts.edges {
        assert_eq!(edge.kind, EdgeKind::Contains);
        assert_eq!(edge.conf, Confidence::Exact);
        assert_eq!(edge.owner_path, "src/lib.rs");
        assert!(facts.nodes.iter().any(|n| n.id == edge.src));
        assert!(facts.nodes.iter().any(|n| n.id == edge.dst));
    }
    let support = PureParser.parse("src/support.rs", SUPPORT).unwrap();
    assert_eq!(
        nodes(&support),
        [
            ("m:src/support.rs", Module, None),
            ("s:src/support.rs#Value", Symbol, Some(Type)),
            ("s:src/support.rs#helper", Symbol, Some(Function)),
        ]
    );
    assert!(support.references.is_empty());
}

#[test]
fn rust_fixture_has_the_exact_raw_reference_set() {
    use EdgeKind::{Calls, Implements, Imports, Reexports, Tests, UsesType};
    let facts = PureParser.parse("src/lib.rs", RUST).unwrap();
    let actual = facts
        .references
        .iter()
        .map(|r| {
            (
                r.src.as_str(),
                r.kind,
                r.name.as_str(),
                r.scope.as_str(),
                r.alias.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            ("m:src/lib.rs", Imports, "support", "", None),
            (
                "m:src/lib.rs",
                Imports,
                "support::helper",
                "",
                Some("assist")
            ),
            (
                "m:src/lib.rs",
                Reexports,
                "support::helper",
                "",
                Some("assist")
            ),
            ("m:src/lib.rs", Imports, "support::Value", "", None),
            ("m:src/lib.rs", Reexports, "support::Value", "", None),
            ("m:src/lib.rs", Imports, "support::helper", "", None),
            (
                "s:src/lib.rs#Widget::impl<Work>",
                Implements,
                "Work",
                "Widget",
                None
            ),
            (
                "s:src/lib.rs#Widget::impl<Work>",
                UsesType,
                "Widget",
                "Widget",
                None
            ),
            (
                "s:src/lib.rs#Widget::run",
                Calls,
                "helper",
                "Widget::run",
                None
            ),
            (
                "s:src/lib.rs#Widget::impl",
                UsesType,
                "Widget",
                "Widget",
                None
            ),
            (
                "s:src/lib.rs#Widget::value",
                Calls,
                "self.run",
                "Widget::value",
                None
            ),
            ("s:src/lib.rs#Alias", UsesType, "Widget", "Alias", None),
            ("m:src/lib.rs", Imports, "super::Widget", "tests", None),
            (
                "t:src/lib.rs#tests::checks",
                Calls,
                "value.value",
                "tests::checks",
                None
            ),
            (
                "t:src/lib.rs#tests::checks",
                Tests,
                "value.value",
                "tests::checks",
                None
            ),
        ]
    );
    for reference in &facts.references {
        assert_eq!(reference.owner_path, "src/lib.rs");
        assert!(reference.lines[0] > 0 && reference.lines[1] >= reference.lines[0]);
    }
}

#[test]
fn markdown_fixture_has_the_exact_heading_tree_and_links() {
    let facts = PureParser.parse("docs/spec.md", MARKDOWN).unwrap();
    let ids = facts
        .nodes
        .iter()
        .map(|n| n.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "m:docs/spec.md",
            "d:docs/spec.md#map-mode",
            "d:docs/spec.md#stable-ids~1",
            "d:docs/spec.md#examples",
            "d:docs/spec.md#stable-ids~2",
            "d:docs/spec.md#appendix",
            "d:docs/spec.md#details"
        ]
    );
    let parents = facts
        .edges
        .iter()
        .map(|e| (e.src.as_str(), e.dst.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        parents,
        [
            (ids[0], ids[1]),
            (ids[1], ids[2]),
            (ids[2], ids[3]),
            (ids[1], ids[4]),
            (ids[0], ids[5]),
            (ids[5], ids[6])
        ]
    );
    let references = facts
        .references
        .iter()
        .map(|r| (r.src.as_str(), r.kind, r.name.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        references,
        [
            (ids[1], EdgeKind::Documents, "Widget"),
            (ids[1], EdgeKind::Documents, "src/lib.rs"),
            (ids[3], EdgeKind::Documents, "Widget::value"),
            (ids[5], EdgeKind::Documents, "support::helper"),
        ]
    );
    assert_eq!(facts.nodes[1].summary.text, "Traverse code.");
}

#[test]
fn same_bytes_produce_identical_complete_facts() {
    let parser: &dyn Parser = &PureParser;
    for (path, bytes) in [
        ("src/lib.rs", RUST),
        ("src/support.rs", SUPPORT),
        ("spec.md", MARKDOWN),
    ] {
        let expected = parser.parse(path, bytes).unwrap();
        for _ in 0..4 {
            assert_eq!(parser.parse(path, bytes).unwrap(), expected);
        }
    }
}
