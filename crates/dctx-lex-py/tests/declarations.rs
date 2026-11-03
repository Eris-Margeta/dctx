// SPDX-License-Identifier: Apache-2.0
//! Golden declaration facts, constants and imports for the Python extractor.
//! Covers the Python structural extractor contract (decision 077/decision 078).
//! Uses common helpers and fixtures with the public lexer and map-model APIs.
#![forbid(unsafe_code)]

mod common;

use common::fixtures::{CONSTS, CONT, IMPORTS_SRC, SAMPLE, SEMI};
use common::{call_names, check_invariants, extract, last_line_of, line_of, names, node};
use dctx_lex_py::{body_fingerprint, Completion, DiagnosticReason, Dialect, Precision};
use dctx_map_model::{Confidence, EdgeKind, NodeKind, SummarySource, SymbolKind};

#[test]
#[allow(clippy::too_many_lines, reason = "Keep each fact-assembly pass or golden-fixture verification together so related identities and expected records can be reviewed in order.")]
fn sample_module_matches_the_golden_facts() {
    let facts = extract("pkg/sample.py", SAMPLE);
    let again = extract("pkg/sample.py", SAMPLE);
    assert_eq!(facts, again);
    check_invariants(SAMPLE, &facts);
    assert_eq!(facts.dialect, Dialect::Python);
    assert_eq!(facts.path, "pkg/sample.py");
    assert_eq!(facts.diagnostics, []);
    assert_eq!(
        names(&facts),
        [
            "pkg/sample.py",
            "MAX",
            "NAME",
            "run",
            "Box",
            "Box::__init__",
            "Box::_hidden",
            "Box::make",
            "nested",
            "nested::inner",
        ]
    );
    let kinds = [
        None,
        Some(SymbolKind::Const),
        Some(SymbolKind::Const),
        Some(SymbolKind::Function),
        Some(SymbolKind::Class),
        Some(SymbolKind::Method),
        Some(SymbolKind::Method),
        Some(SymbolKind::Method),
        Some(SymbolKind::Function),
        Some(SymbolKind::Function),
    ];
    for (item, kind) in facts.nodes.iter().zip(kinds) {
        assert_eq!(item.fact.symbol_kind, kind, "{}", item.fact.name);
        assert_eq!(item.fact.kind, if kind.is_none() { NodeKind::Module } else { NodeKind::Symbol });
    }
    let public = [false, true, true, true, true, true, false, true, true, true];
    for (item, is_public) in facts.nodes.iter().zip(public) {
        assert_eq!(item.fact.public, is_public, "{}", item.fact.name);
    }
    for item in &facts.nodes {
        let precision = if matches!(item.fact.symbol_kind, Some(SymbolKind::Const)) {
            Precision::Convention
        } else {
            Precision::Structural
        };
        assert_eq!(item.precision, precision, "{}", item.fact.name);
    }
    let ids = [
        "m:pkg/sample.py",
        "s:pkg/sample.py#MAX",
        "s:pkg/sample.py#NAME",
        "s:pkg/sample.py#run",
        "s:pkg/sample.py#Box",
        "s:pkg/sample.py#Box::__init__",
        "s:pkg/sample.py#Box::_hidden",
        "s:pkg/sample.py#Box::make",
        "s:pkg/sample.py#nested",
        "s:pkg/sample.py#nested::inner",
    ];
    for (item, id) in facts.nodes.iter().zip(ids) {
        assert_eq!(item.fact.id.as_str(), id);
    }
    let spans = [
        ("pkg/sample.py", 1, u32::try_from(SAMPLE.lines().count()).expect("bounded test coordinate")),
        ("MAX", line_of(SAMPLE, "MAX = 3"), line_of(SAMPLE, "MAX = 3")),
        ("NAME", line_of(SAMPLE, "NAME: str"), line_of(SAMPLE, "NAME: str")),
        ("run", line_of(SAMPLE, "@decorator"), line_of(SAMPLE, "return os.getcwd()")),
        ("Box", line_of(SAMPLE, "class Box"), line_of(SAMPLE, "return run()")),
        ("Box::__init__", line_of(SAMPLE, "def __init__"), line_of(SAMPLE, "self.n = size")),
        ("Box::_hidden", line_of(SAMPLE, "def _hidden"), line_of(SAMPLE, "return self.n")),
        ("Box::make", line_of(SAMPLE, "@staticmethod"), line_of(SAMPLE, "return run()")),
        ("nested", line_of(SAMPLE, "def nested"), last_line_of(SAMPLE, "helper()")),
        ("nested::inner", line_of(SAMPLE, "def inner"), last_line_of(SAMPLE, "helper()")),
    ];
    for (name, start, end) in spans {
        assert_eq!(node(&facts, name).fact.lines, [start, end], "{name}");
    }
    let signatures = [
        ("pkg/sample.py", ""),
        ("MAX", "MAX = 3"),
        ("NAME", "NAME: str = \"py\""),
        ("run", "@decorator @wrap(helper()) async def run(n: int = default()) -> int:"),
        ("Box", "class Box(Base, factory()):"),
        ("Box::__init__", "def __init__(self, size=seed()):"),
        ("Box::_hidden", "def _hidden(self):"),
        ("Box::make", "@staticmethod def make():"),
        ("nested", "def nested():"),
        ("nested::inner", "def inner():"),
    ];
    for (name, signature) in signatures {
        assert_eq!(node(&facts, name).fact.signature, signature, "{name}");
    }
    let summaries = [
        ("pkg/sample.py", SummarySource::Doc, "Package entry."),
        ("MAX", SummarySource::Signature, "MAX = 3"),
        ("NAME", SummarySource::Signature, "NAME: str = \"py\""),
        ("run", SummarySource::Doc, "Runs the job."),
        ("Box", SummarySource::Doc, "A box."),
        ("Box::__init__", SummarySource::Signature, "def __init__(self, size=seed()):"),
        ("Box::_hidden", SummarySource::Signature, "def _hidden(self):"),
        ("Box::make", SummarySource::Signature, "@staticmethod def make():"),
        ("nested", SummarySource::Signature, "def nested():"),
        ("nested::inner", SummarySource::Signature, "def inner():"),
    ];
    for (name, source, text) in summaries {
        let summary = &node(&facts, name).fact.summary;
        assert_eq!(summary.source, source, "{name}");
        assert_eq!(summary.text, text, "{name}");
    }
    let bodies = [
        ("pkg/sample.py", SAMPLE),
        ("MAX", "MAX = 3"),
        ("NAME", "NAME: str = \"py\""),
        ("run", "\"\"\"Runs the job. Returns the count.\"\"\" return os.getcwd()"),
        (
            "Box",
            "\"\"\"A box.\"\"\" def __init__(self, size=seed()): self.n = size def _hidden(self): return self.n @staticmethod def make(): return run()",
        ),
        ("Box::__init__", "self.n = size"),
        ("Box::_hidden", "return self.n"),
        ("Box::make", "return run()"),
        ("nested", "def inner(): helper()"),
        ("nested::inner", "helper()"),
    ];
    for (name, body) in bodies {
        assert_eq!(
            node(&facts, name).fact.fingerprint,
            body_fingerprint(body),
            "{name} {body}"
        );
    }
    let parents = [
        "m:pkg/sample.py",
        "m:pkg/sample.py",
        "m:pkg/sample.py",
        "m:pkg/sample.py",
        "s:pkg/sample.py#Box",
        "s:pkg/sample.py#Box",
        "s:pkg/sample.py#Box",
        "m:pkg/sample.py",
        "s:pkg/sample.py#nested",
    ];
    for (edge, parent) in facts.edges.iter().zip(parents) {
        assert_eq!(edge.fact.src.as_str(), parent);
        assert_eq!(edge.fact.kind, EdgeKind::Contains);
        assert_eq!(edge.fact.conf, Confidence::Exact);
    }
    // Same-start references keep the longer span first (`c` before `a.b`, `other` before `name`).
    let expected = [
        ("m:pkg/sample.py", EdgeKind::Imports, "os", "", None, line_of(SAMPLE, "import os")),
        ("m:pkg/sample.py", EdgeKind::Imports, "os.path", "", Some("osp"), line_of(SAMPLE, "import os.path")),
        ("m:pkg/sample.py", EdgeKind::Imports, ".::sibling", "", None, line_of(SAMPLE, "from . import")),
        ("m:pkg/sample.py", EdgeKind::Imports, "..pkg.mod::other", "", None, line_of(SAMPLE, "from ..pkg.mod")),
        ("m:pkg/sample.py", EdgeKind::Imports, "..pkg.mod::name", "", Some("n"), line_of(SAMPLE, "from ..pkg.mod")),
        ("m:pkg/sample.py", EdgeKind::Imports, "typing::*", "", None, line_of(SAMPLE, "from typing")),
        ("m:pkg/sample.py", EdgeKind::Imports, "c", "", None, line_of(SAMPLE, "import a.b")),
        ("m:pkg/sample.py", EdgeKind::Imports, "a.b", "", None, line_of(SAMPLE, "import a.b")),
        ("m:pkg/sample.py", EdgeKind::Calls, "wrap", "", None, line_of(SAMPLE, "@wrap")),
        ("m:pkg/sample.py", EdgeKind::Calls, "helper", "", None, line_of(SAMPLE, "@wrap")),
        ("m:pkg/sample.py", EdgeKind::Calls, "default", "", None, line_of(SAMPLE, "default()")),
        ("s:pkg/sample.py#run", EdgeKind::Calls, "os.getcwd", "run", None, line_of(SAMPLE, "os.getcwd()")),
        ("m:pkg/sample.py", EdgeKind::Calls, "factory", "", None, line_of(SAMPLE, "factory()")),
        ("s:pkg/sample.py#Box", EdgeKind::Calls, "seed", "Box", None, line_of(SAMPLE, "seed()")),
        ("s:pkg/sample.py#Box::make", EdgeKind::Calls, "run", "Box::make", None, line_of(SAMPLE, "return run()")),
        ("s:pkg/sample.py#nested::inner", EdgeKind::Calls, "helper", "nested::inner", None, last_line_of(SAMPLE, "helper()")),
    ];
    assert_eq!(facts.references.len(), expected.len());
    for (item, (src, kind, name, scope, alias, line)) in facts.references.iter().zip(expected) {
        assert_eq!(item.precision, Precision::Structural, "{name}");
        assert_eq!(item.fact.src.as_str(), src, "{name}");
        assert_eq!(item.fact.kind, kind, "{name}");
        assert_eq!(item.fact.name, name);
        assert_eq!(item.fact.scope, scope, "{name}");
        assert_eq!(item.fact.alias.as_deref(), alias, "{name}");
        assert_eq!(item.fact.lines, [line, line], "{name}");
        assert_eq!(item.fact.owner_path, "pkg/sample.py");
    }
    let stub = extract("pkg/sample.pyi", SAMPLE);
    assert_eq!(stub.dialect, Dialect::Stub);
    assert_eq!(stub.completion, Completion::Complete);
    assert_eq!(stub.nodes.len(), facts.nodes.len());
    let windows = extract("pkg/sample.pyw", SAMPLE);
    assert_eq!(windows.dialect, Dialect::Python);
}

#[test]
fn constants_are_module_level_all_caps_only() {
    let facts = extract("c.py", CONSTS);
    check_invariants(CONSTS, &facts);
    assert_eq!(facts.completion, Completion::Complete);
    let ids: Vec<_> = facts.nodes.iter().skip(1).map(|item| item.fact.id.as_str()).collect();
    assert_eq!(
        ids,
        ["s:c.py#A~1", "s:c.py#B~1", "s:c.py#A~2", "s:c.py#B~2", "s:c.py#NAME"]
    );
    assert_eq!(facts.nodes[1].fact.signature, "(A, B) = pair()");
    assert_eq!(facts.nodes[2].fact.signature, "(A, B) = pair()");
    assert_eq!(facts.nodes[3].fact.signature, "A = B = 1");
    assert_eq!(facts.nodes[4].fact.signature, "A = B = 1");
    assert_eq!(facts.nodes[5].fact.signature, "NAME: int");
    assert!(facts.nodes.iter().skip(1).all(|item| item.precision == Precision::Convention));
    assert_eq!(call_names(&facts), ["pair"]);
    let semi = extract("s.py", SEMI);
    assert_eq!(semi.nodes[1].fact.signature, "MAX = 1");
    assert_eq!(semi.nodes[2].fact.signature, "MIN = 2");
    assert_eq!(semi.completion, Completion::Complete);
    let continued = extract("v.py", CONT);
    assert_eq!(continued.nodes[1].fact.name, "VALUE");
    assert_eq!(continued.nodes[1].precision, Precision::Convention);
    assert_eq!(continued.references[0].fact.name, "helper");
    assert_eq!(continued.references[0].fact.scope, "");
    assert_eq!(continued.completion, Completion::Complete);
}

#[test]
fn imports_cover_relative_star_alias_and_function_scope() {
    let facts = extract("i.py", IMPORTS_SRC);
    check_invariants(IMPORTS_SRC, &facts);
    assert_eq!(facts.completion, Completion::Partial);
    assert!(facts.diagnostics.iter().any(|item| item.reason == DiagnosticReason::UnexpectedToken));
    let mut got: Vec<_> = facts
        .references
        .iter()
        .map(|item| {
            (
                item.fact.name.as_str(),
                item.fact.alias.as_deref(),
                item.fact.scope.as_str(),
                item.fact.kind,
            )
        })
        .collect();
    got.sort();
    let mut expected = vec![
        ("...::*", None, "", EdgeKind::Imports),
        ("a.b", None, "f", EdgeKind::Imports),
        ("os::other", None, "f", EdgeKind::Imports),
        ("os::path", Some("p"), "f", EdgeKind::Imports),
        ("x::a", None, "", EdgeKind::Imports),
        ("x::b", Some("c"), "", EdgeKind::Imports),
    ];
    expected.sort();
    assert_eq!(got, expected);
    assert!(facts.references.iter().all(|item| item.fact.name != "missing"));
    let function = node(&facts, "f").fact.id.as_str();
    assert!(facts.references.iter().filter(|item| item.fact.scope == "f").all(|item| item.fact.src.as_str() == function));
}
