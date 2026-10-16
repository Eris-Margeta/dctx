//! Malformed-input, limit and determinism properties through the public API.
//! Implements PARSERS-v1 "no panic, no unbounded recursion" with proptest fuzzing
//! of arbitrary bytes and JS-shaped text, plus typed refusals with positive controls.
#![forbid(unsafe_code)]
use dctx_lex_js::{Completion, JsLexer, LexError, LexedFile, Limits, Precision};
use proptest::prelude::*;

const PATHS: [&str; 3] = ["f.js", "f.ts", "f.tsx"];

fn check_invariants(source: &str, facts: &LexedFile) {
    let lines = source.lines().count().max(1) as u32;
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    for node in &facts.nodes {
        let [start, end] = node.fact.lines;
        assert!(start >= 1 && start <= end && end <= lines.max(start));
    }
    for reference in &facts.references {
        let [start, end] = reference.fact.lines;
        assert!(start >= 1 && start <= end);
    }
    assert!(facts.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
    if facts.diagnostics.is_empty() {
        assert_eq!(facts.completion, Completion::Complete);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        for path in PATHS {
            if let Ok(facts) = JsLexer::default().extract(path, &bytes) {
                check_invariants(std::str::from_utf8(&bytes).unwrap_or_default(), &facts);
            }
        }
    }

    #[test]
    fn js_shaped_text_is_deterministic_and_bounded(
        source in "([a-z]{1,3}|[ \\n(){}\\[\\]<>/`'\"$=;:,.*@#!?+&|-]|=>|\\$\\{|function|class|const|export|import|type|interface|=> \\{|é|数){0,120}"
    ) {
        for path in PATHS {
            let first = JsLexer::default().extract(path, source.as_bytes());
            let second = JsLexer::default().extract(path, source.as_bytes());
            match (first, second) {
                (Ok(a), Ok(b)) => {
                    check_invariants(&source, &a);
                    prop_assert_eq!(a, b);
                }
                (Err(a), Err(b)) => prop_assert_eq!(a.to_string(), b.to_string()),
                _ => prop_assert!(false, "non-deterministic outcome"),
            }
        }
    }
}

#[test]
fn malformed_fragments_yield_partial_facts_not_errors() {
    for source in [
        "class {",
        "function (",
        "`${",
        "/*",
        "<div",
        "import {",
        "export {",
        "const f = () =>",
        "a = 'unterminated\nb()",
        "}}}))) f()",
        "type T = A<B<",
        "interface I { m(",
        "x = <a b={c}>{</a>",
    ] {
        for path in PATHS {
            let facts = JsLexer::default()
                .extract(path, source.as_bytes())
                .unwrap_or_else(|e| panic!("{path} {source:?}: {e}"));
            check_invariants(source, &facts);
        }
    }
    let facts = JsLexer::default()
        .extract("p.js", b"function ok() {}\nfunction broken() { call(")
        .unwrap();
    assert_eq!(facts.completion, Completion::Partial);
    assert_eq!(facts.nodes[1].precision, Precision::Structural);
    assert_eq!(facts.nodes[2].precision, Precision::Partial);
    assert!(facts.references.iter().all(|r| r.precision == Precision::Partial));
}

#[test]
fn nesting_limits_refuse_with_typed_errors_and_positive_controls() {
    let deep = "(".repeat(5000) + &")".repeat(5000);
    assert!(matches!(
        JsLexer::default().extract("d.js", deep.as_bytes()),
        Err(LexError::TooDeep { line: 1, .. })
    ));
    let generics = format!("type T = {}X{};", "A<".repeat(2000), ">".repeat(2000));
    assert!(matches!(
        JsLexer::default().extract("d.ts", generics.as_bytes()),
        Err(LexError::TooDeep { .. })
    ));
    let templates = "`${".repeat(400) + &"}`".repeat(400);
    assert!(matches!(
        JsLexer::default().extract("d.js", templates.as_bytes()),
        Err(LexError::TooDeep { .. })
    ));
    let shallow = "(".repeat(100) + &")".repeat(100);
    assert!(JsLexer::default().extract("d.js", shallow.as_bytes()).is_ok());
    let narrow = JsLexer::new(Limits {
        max_depth: 8,
        ..Limits::default()
    });
    assert!(narrow.extract("d.js", b"f(g(h(i())))").is_ok());
    assert!(narrow.extract("d.js", "[".repeat(9).as_bytes()).is_err());
}

#[test]
fn unicode_and_private_names_abstain_instead_of_failing() {
    let facts = JsLexer::default()
        .extract("u.js", "class Ünï { #hidden() {} visible() {} }\nconst ñ = () => 1;".as_bytes())
        .unwrap();
    let names: Vec<_> = facts.nodes.iter().skip(1).map(|n| n.fact.name.as_str()).collect();
    assert_eq!(names, ["Ünï", "Ünï::visible", "ñ"]);
    assert_eq!(facts.completion, Completion::Partial);
}
