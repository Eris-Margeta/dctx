// SPDX-License-Identifier: Apache-2.0
//! Exact JavaScript and TypeScript fixture facts through the Parser port.
//! Verifies SPEC-MAP-MODE sections 5 and 6.2 and CM-9 determinism.
//! Checked-in source is parsed without executing code or reading dependencies.
#![forbid(unsafe_code)]
use dctx_map_model::{EdgeKind, NodeKind, SummarySource, SymbolKind};
use dctx_map_parse::{Language, ParseError, ParsedFile, Parser, PureParser};

const JS: &[u8] = include_bytes!("fixtures/js/module.mjs");
const TS: &[u8] = include_bytes!("fixtures/ts/module.ts");

fn symbols(facts: &ParsedFile) -> Vec<(&str, NodeKind, Option<SymbolKind>)> {
    facts
        .nodes
        .iter()
        .map(|n| (n.name.as_str(), n.kind, n.symbol_kind))
        .collect()
}

fn references(facts: &ParsedFile) -> Vec<(&str, EdgeKind, &str, &str, Option<&str>)> {
    facts
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
        .collect()
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "Keep the complete JavaScript fixture and its exact ordered node/reference expectations together."
)]
fn javascript_fixture_has_exact_nodes_and_references() {
    use EdgeKind::{Calls, Imports, Reexports, Tests};
    use NodeKind::{Module, Symbol, Test};
    use SymbolKind::{Class, Function, Method};
    let facts = PureParser.parse("module.mjs", JS).unwrap();
    assert_eq!(
        symbols(&facts),
        [
            ("module.mjs", Module, None),
            ("run", Symbol, Some(Function)),
            ("arrow", Symbol, Some(Function)),
            ("Worker", Symbol, Some(Class)),
            ("Worker::execute", Symbol, Some(Method)),
            ("suite", Test, Some(Function)),
            ("suite::runs", Test, Some(Function)),
        ]
    );
    assert_eq!(
        references(&facts),
        [
            (
                "m:module.mjs",
                Imports,
                "./support.js::default",
                "",
                Some("runner")
            ),
            (
                "m:module.mjs",
                Imports,
                "./support.js::work",
                "",
                Some("assist")
            ),
            ("m:module.mjs", Imports, "./helpers.js", "", Some("helpers")),
            ("m:module.mjs", Imports, "./setup.js", "", None),
            ("m:module.mjs", Imports, "node:test::test", "", None),
            ("m:module.mjs", Imports, "node:test::describe", "", None),
            (
                "m:module.mjs",
                Reexports,
                "./support.js::work",
                "",
                Some("shared")
            ),
            ("m:module.mjs", Reexports, "./helpers.js::*", "", None),
            (
                "m:module.mjs",
                Imports,
                "node:assert::check",
                "",
                Some("verify")
            ),
            ("m:module.mjs", Imports, "node:assert::equal", "", None),
            ("m:module.mjs", Imports, "node:fs", "", Some("fs")),
            ("s:module.mjs#run", Calls, "assist", "run", None),
            ("s:module.mjs#arrow", Calls, "run", "arrow", None),
            (
                "s:module.mjs#Worker::execute",
                Calls,
                "arrow",
                "Worker::execute",
                None
            ),
            (
                "t:module.mjs#suite::runs",
                Calls,
                "run",
                "suite::runs",
                None
            ),
            (
                "t:module.mjs#suite::runs",
                Tests,
                "run",
                "suite::runs",
                None
            ),
            (
                "t:module.mjs#suite::runs",
                Calls,
                "verify",
                "suite::runs",
                None
            ),
            (
                "t:module.mjs#suite::runs",
                Tests,
                "verify",
                "suite::runs",
                None
            ),
        ]
    );
    assert_eq!(facts.nodes[1].summary.text, "Run work.");
    assert_eq!(
        facts.nodes.iter().map(|n| n.lines).collect::<Vec<_>>(),
        [
            [1, 24],
            [13, 13],
            [17, 17],
            [18, 21],
            [20, 20],
            [22, 24],
            [23, 23]
        ]
    );
    assert_eq!(
        facts.references.iter().map(|r| r.lines).collect::<Vec<_>>(),
        [
            4, 4, 5, 6, 7, 7, 8, 9, 10, 10, 11, 13, 17, 20, 23, 23, 23, 23
        ]
        .map(|line| [line, line])
    );
    assert_eq!(facts.nodes[2].summary.text, "Named arrow documentation.");
    assert_eq!(facts.nodes[4].summary.source, SummarySource::Doc);
    assert!(facts.nodes[1..4].iter().all(|n| n.public));
    assert_eq!(facts.edges[3].src.as_str(), "s:module.mjs#Worker");
    assert_eq!(facts.edges[5].src.as_str(), "t:module.mjs#suite");
}

#[test]
fn typescript_fixture_has_exact_nodes_and_references() {
    use EdgeKind::{Calls, Imports, Reexports, Tests, UsesType};
    use NodeKind::{Module, Symbol, Test};
    use SymbolKind::{Class, Function, Interface, Method, Type};
    let facts = PureParser.parse("module.ts", TS).unwrap();
    assert_eq!(
        symbols(&facts),
        [
            ("module.ts", Module, None),
            ("Worker", Symbol, Some(Interface)),
            ("Worker::run", Symbol, Some(Method)),
            ("Result", Symbol, Some(Type)),
            ("process", Symbol, Some(Function)),
            ("arrow", Symbol, Some(Function)),
            ("Service", Symbol, Some(Class)),
            ("Service::run", Symbol, Some(Method)),
            ("processes", Test, Some(Function)),
        ]
    );
    assert_eq!(
        references(&facts),
        [
            ("m:module.ts", Imports, "vitest::it", "", None),
            ("m:module.ts", Imports, "./types::Input", "", Some("Data")),
            ("m:module.ts", Reexports, "./types::Input", "", None),
            (
                "s:module.ts#Worker::run",
                UsesType,
                "Data",
                "Worker::run",
                None
            ),
            (
                "s:module.ts#Worker::run",
                UsesType,
                "Data",
                "Worker::run",
                None
            ),
            ("s:module.ts#Result", UsesType, "Data", "Result", None),
            ("s:module.ts#process", UsesType, "Data", "process", None),
            ("s:module.ts#process", UsesType, "Data", "process", None),
            ("s:module.ts#arrow", UsesType, "Data", "arrow", None),
            ("s:module.ts#arrow", UsesType, "Data", "arrow", None),
            ("s:module.ts#arrow", Calls, "process", "arrow", None),
            (
                "s:module.ts#Service::run",
                UsesType,
                "Data",
                "Service::run",
                None
            ),
            (
                "s:module.ts#Service::run",
                UsesType,
                "Data",
                "Service::run",
                None
            ),
            (
                "s:module.ts#Service::run",
                Calls,
                "arrow",
                "Service::run",
                None
            ),
            ("t:module.ts#processes", Calls, "process", "processes", None),
            ("t:module.ts#processes", Tests, "process", "processes", None),
        ]
    );
    assert_eq!(facts.nodes[4].summary.text, "Process one input.");
    assert_eq!(
        facts.references.iter().map(|r| r.lines).collect::<Vec<_>>(),
        [4, 5, 6, 8, 8, 9, 11, 11, 12, 12, 12, 14, 14, 14, 16, 16].map(|line| [line, line])
    );
}

#[test]
fn extensions_jsx_tsx_and_invalid_syntax_are_explicit() {
    for extension in ["js", "mjs", "cjs", "jsx", "JS"] {
        let facts = PureParser
            .parse(&format!("view.{extension}"), b"const View = () => <div />;")
            .unwrap();
        assert_eq!(facts.language, Language::JavaScript);
        assert_eq!(facts.nodes[1].name, "View");
    }
    for extension in ["ts", "mts", "cts", "TS"] {
        assert_eq!(
            Language::for_path(&format!("x.{extension}")),
            Language::TypeScript
        );
    }
    let facts = PureParser
        .parse("view.tsx", b"export const View = (x: Props) => <div />;")
        .unwrap();
    assert_eq!(facts.language, Language::Tsx);
    assert_eq!(facts.references[0].name, "Props");
    for path in ["x.js", "x.ts", "x.tsx"] {
        assert!(matches!(
            PureParser.parse(path, b"function broken("),
            Err(ParseError::Syntax { .. })
        ));
        assert!(PureParser.parse(path, b"function valid() {}").is_ok());
    }
}

#[test]
fn duplicate_tests_and_complete_facts_are_deterministic() {
    for (path, bytes) in [("module.mjs", JS), ("module.ts", TS)] {
        let facts = PureParser.parse(path, bytes).unwrap();
        assert_eq!(PureParser.parse(path, bytes).unwrap(), facts);
        assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
        for reference in &facts.references {
            assert_eq!(reference.owner_path, path);
            assert!(reference.lines[0] > 0 && reference.lines[1] >= reference.lines[0]);
        }
    }
    let facts = PureParser
        .parse(
            "test.js",
            b"it('same', () => run()); it('same', () => run());",
        )
        .unwrap();
    for (index, ordinal) in [(1, 1), (2, 2)] {
        assert_eq!(
            facts.nodes[index].id.as_str(),
            format!("t:test.js#same~{ordinal}")
        );
        assert_eq!(facts.edges[index - 1].dst, facts.nodes[index].id);
        assert_eq!(facts.references[(index - 1) * 2].src, facts.nodes[index].id);
    }
}
