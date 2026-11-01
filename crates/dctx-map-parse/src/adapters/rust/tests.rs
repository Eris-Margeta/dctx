use crate::{Parser, PureParser};
use dctx_map_model::{EdgeKind, NodeKind, SymbolKind};

#[test]
fn macro_bodies_are_not_expanded_into_false_symbols() {
    let source = b"macro_rules! make { () => { fn phantom() {} } } make!(); fn real() {}";
    let facts = PureParser.parse("lib.rs", source).unwrap();
    assert_eq!(
        facts.nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
        ["lib.rs", "make", "real"]
    );
}

#[test]
fn docs_attributes_and_methods_keep_their_meaning() {
    let source = b"pub struct A; impl A { /// Does work. More detail.\n pub fn work(&self) {} }\n#[tokio::test]\nasync fn checks() {}";
    let parsed = PureParser.parse("a.rs", source).unwrap();
    let method = parsed.nodes.iter().find(|n| n.name == "A::work").unwrap();
    assert_eq!(method.symbol_kind, Some(SymbolKind::Method));
    assert_eq!(method.summary.text, "Does work.");
    assert!(method.public);
    assert_eq!(parsed.nodes.last().unwrap().kind, NodeKind::Test);
}

#[test]
fn nested_imports_and_calls_keep_syntactic_targets() {
    let facts = PureParser
        .parse("lib.rs", b"use crate::a::{self, b::{C as D, E}, *};")
        .unwrap();
    let imports = facts
        .references
        .iter()
        .map(|r| (r.name.as_str(), r.alias.as_deref()))
        .collect::<Vec<_>>();
    assert_eq!(
        imports,
        [
            ("crate::a", None),
            ("crate::a::b::C", Some("D")),
            ("crate::a::b::E", None),
            ("crate::a::*", None)
        ]
    );
    let facts = PureParser
        .parse("lib.rs", b"fn run() { crate::work::<Thing>(); worker.run(); }")
        .unwrap();
    let calls = facts
        .references
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| r.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(calls, ["crate::work", "worker.run"]);
}
