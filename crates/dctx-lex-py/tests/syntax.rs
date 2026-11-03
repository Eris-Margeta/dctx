// SPDX-License-Identifier: Apache-2.0
//! Literal masking, docstrings, indentation and call scopes for the Python extractor.
//! Covers the Python structural extractor contract (decision 077/decision 078).
//! Uses common helpers and inline fixtures through the public lexer API.
#![forbid(unsafe_code)]

mod common;

use common::fixtures::{
    ASYNC_FOR, BOM_SRC, CALLS_SRC, CRLF_SRC, DEDENT_SRC, DOC_SRC, MASKING, MATCH_SRC,
    MIXED_SRC, ORDINALS, SHEBANG, TABS_SRC, UNICODE_SRC,
};
use common::{call_names, check_invariants, extract, names, node};
use dctx_lex_py::{Completion, DiagnosticReason};
use dctx_map_model::{SummarySource, SymbolKind};

#[test]
fn strings_comments_and_t_strings_do_not_invent_declarations() {
    let facts = extract("m.py", MASKING);
    check_invariants(MASKING, &facts);
    assert_eq!(names(&facts), ["m.py", "real"]);
    assert_eq!(facts.completion, Completion::Complete);
    let calls: Vec<_> = facts
        .references
        .iter()
        .map(|item| (item.fact.name.as_str(), item.fact.scope.as_str()))
        .collect();
    assert_eq!(calls, [("compute", "real"), ("compute", ""), ("fmt", "")]);
}

#[test]
fn match_patterns_abstain_and_guards_are_calls() {
    let facts = extract("m.py", MATCH_SRC);
    check_invariants(MATCH_SRC, &facts);
    assert_eq!(facts.completion, Completion::Complete);
    assert_eq!(names(&facts), ["m.py", "outer"]);
    assert_eq!(node(&facts, "outer").fact.symbol_kind, Some(SymbolKind::Function));
    assert_eq!(call_names(&facts), ["subject", "inside", "guard", "taken"]);
    assert!(facts.references.iter().all(|item| item.fact.scope == "outer"));
    assert!(facts.references.iter().all(|item| item.fact.name != "Point"));
}

#[test]
fn dynamic_callees_abstain_and_compound_headers_keep_the_enclosing_scope() {
    let facts = extract("c.py", CALLS_SRC);
    check_invariants(CALLS_SRC, &facts);
    assert_eq!(facts.completion, Completion::Complete);
    assert_eq!(
        call_names(&facts),
        ["obj.method", "open", "h.read", "Error", "recover", "items"]
    );
    assert!(facts.references.iter().all(|item| item.fact.scope == "f"));
    let async_for = extract("a.py", ASYNC_FOR);
    assert_eq!(node(&async_for, "f").fact.symbol_kind, Some(SymbolKind::Function));
    assert_eq!(node(&async_for, "f").fact.signature, "async def f():");
    assert_eq!(call_names(&async_for), ["items", "use"]);
    assert!(async_for.references.iter().all(|item| item.fact.scope == "f"));
    assert_eq!(async_for.completion, Completion::Complete);
}

#[test]
fn docstrings_are_first_sentences_and_comments_skip_the_shebang() {
    let facts = extract("d.py", DOC_SRC);
    check_invariants(DOC_SRC, &facts);
    assert_eq!(facts.completion, Completion::Complete);
    assert_eq!(facts.references, []);
    let expect = [
        ("raw", SummarySource::Doc, "Raw doc."),
        ("binary", SummarySource::Signature, "def binary():"),
        ("escaped", SummarySource::Doc, "Line Two."),
        ("joined", SummarySource::Doc, "Hello."),
        ("wrapped", SummarySource::Doc, "Only one."),
        ("formatted", SummarySource::Signature, "def formatted():"),
    ];
    for (name, source, text) in expect {
        let summary = &node(&facts, name).fact.summary;
        assert_eq!(summary.source, source, "{name}");
        assert_eq!(summary.text, text, "{name}");
    }
    let shebang = extract("s.py", SHEBANG);
    assert_eq!(shebang.nodes[0].fact.summary.text, "First sentence.");
    assert_eq!(shebang.nodes[0].fact.summary.source, SummarySource::Doc);
    assert_eq!(node(&shebang, "f").fact.summary.text, "def f():");
    assert_eq!(shebang.completion, Completion::Complete);
}

#[test]
fn indentation_newlines_unicode_and_ordinals_stay_bounded() {
    let crlf = extract("c.py", CRLF_SRC);
    assert_eq!(names(&crlf), ["c.py", "f"]);
    assert_eq!(call_names(&crlf), ["g"]);
    assert_eq!(crlf.references[0].fact.scope, "f");
    assert_eq!(crlf.completion, Completion::Complete);
    let tabs = extract("t.py", TABS_SRC);
    assert_eq!(names(&tabs), ["t.py", "f"]);
    assert_eq!(call_names(&tabs), ["g"]);
    assert_eq!(tabs.completion, Completion::Complete);
    let mixed = extract("m.py", MIXED_SRC);
    assert_eq!(names(&mixed), ["m.py", "f"]);
    assert_eq!(mixed.completion, Completion::Partial);
    assert!(mixed.diagnostics.iter().any(|item| item.reason == DiagnosticReason::MixedTabs));
    let dedent = extract("d.py", DEDENT_SRC);
    assert_eq!(names(&dedent), ["d.py", "f"]);
    assert_eq!(dedent.completion, Completion::Partial);
    assert!(dedent.diagnostics.iter().any(|item| item.reason == DiagnosticReason::InconsistentIndent));
    let unicode = extract("u.py", UNICODE_SRC);
    assert_eq!(names(&unicode), ["u.py", "café"]);
    assert!(node(&unicode, "café").fact.public);
    assert_eq!(unicode.completion, Completion::Complete);
    let ordinals = extract("o.py", ORDINALS);
    assert_eq!(ordinals.nodes[1].fact.id.as_str(), "s:o.py#f~1");
    assert_eq!(ordinals.nodes[2].fact.id.as_str(), "s:o.py#f~2");
    let bom = extract("b.py", BOM_SRC);
    assert_eq!(names(&bom), ["b.py", "f"]);
    assert_eq!(bom.completion, Completion::Complete);
}
