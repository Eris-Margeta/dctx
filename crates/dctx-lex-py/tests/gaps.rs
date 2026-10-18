//! Gap closures PY1–PY3 for the Python extractor.
//! Covers f-string call ownership, indent composition and resource refusals.
//! Uses the public lexer API; the oracles in this file are the controls.
#![forbid(unsafe_code)]

mod common;

use common::{check_invariants, extract, names, node};
use dctx_lex_py::{Completion, DiagnosticReason, LexError, LexedFile, Limits, PyLexer};
use dctx_map_model::{EdgeKind, StableId};

const SENTINEL: &str = "SENTINEL_DXG08";

fn call_rows(facts: &LexedFile) -> Vec<(String, String, StableId)> {
    facts
        .references
        .iter()
        .filter(|item| item.fact.kind == EdgeKind::Calls)
        .map(|item| (item.fact.scope.clone(), item.fact.name.clone(), item.fact.src.clone()))
        .collect()
}

fn assert_calls(facts: &LexedFile, rows: &[(&str, &str)]) {
    let found = call_rows(facts);
    assert_eq!(found.len(), rows.len(), "{found:?}");
    for (found, expect) in found.iter().zip(rows) {
        assert_eq!(found.0, expect.0);
        assert_eq!(found.1, expect.1);
        let owner = if expect.0.is_empty() {
            &facts.nodes[0]
        } else {
            node(facts, expect.0)
        };
        assert_eq!(found.2, owner.fact.id);
    }
    assert!(found.iter().all(|row| row.1 != "ghost"));
}

fn fstring_source(raw: bool, triple: bool, escaped: bool, spec: bool, nested: bool) -> (String, Vec<(&'static str, &'static str)>) {
    let prefix = if raw { "rf" } else { "f" };
    let quote = if triple { "\"\"\"" } else { "\"" };
    let open_lit = if triple { "ghost()\n" } else { "ghost()" };
    let close_lit = if triple { "\nghost()" } else { "" };
    let escaped = if escaped { " {{ghost()}}" } else { "" };
    let field = if spec { "{live()!r:{width()}s}" } else { "{live()}" };
    let nested_field = if nested { "{f'{nested()}'}" } else { "" };
    let source = format!(
        "anchor()\ndef owner():\n    text = {prefix}{quote}{open_lit}{escaped}{field}{nested_field}{close_lit}{quote}\n    keep()\ndef other():\n    later()\n"
    );
    let mut rows = vec![("", "anchor"), ("owner", "live")];
    if spec {
        rows.push(("owner", "width"));
    }
    if nested {
        rows.push(("owner", "nested"));
    }
    rows.push(("owner", "keep"));
    rows.push(("other", "later"));
    (source, rows)
}

/// PY1. Nested fields, format specs and escaped braces keep `live()` and drop literal `ghost()`.
/// Not a defect: the scanner already assigns those calls and refuses only past the depth ceiling.
#[test]
fn py1_formatted_string_semantic_properties() {
    for raw in [false, true] {
        for triple in [false, true] {
            for escaped in [false, true] {
                for spec in [false, true] {
                    for nested in [false, true] {
                        let (source, rows) = fstring_source(raw, triple, escaped, spec, nested);
                        let facts = extract("m.py", &source);
                        check_invariants(&source, &facts);
                        assert_eq!(facts.completion, Completion::Complete, "{source}");
                        assert_calls(&facts, &rows);
                    }
                }
            }
        }
    }
    let extras = [
        "anchor()\ndef owner():\n    text = f\"{live():{{ghost()}}}\"\n    keep()\ndef other():\n    later()\n",
        "anchor()\ndef owner():\n    text = f\"{live()[0:1]}\"\n    keep()\ndef other():\n    later()\n",
        "anchor()\ndef owner():\n    text = f\"{(lambda: live())}\"\n    keep()\ndef other():\n    later()\n",
        "anchor()\ndef owner():\n    text = f\"{ {live()} }\"\n    keep()\ndef other():\n    later()\n",
    ];
    let rows = [("", "anchor"), ("owner", "live"), ("owner", "keep"), ("other", "later")];
    for source in extras {
        let facts = extract("m.py", source);
        check_invariants(source, &facts);
        assert_eq!(facts.completion, Completion::Complete, "{source}");
        assert_calls(&facts, &rows);
    }
    let deep = concat!(
        "def owner():\n",
        "    x = f\"{f'{f\"{live()}\"}'}\"\n",
        "    keep()\n",
    );
    let narrow = PyLexer::new(Limits { max_depth: 2, ..Limits::default() });
    assert!(matches!(
        narrow.extract("m.py", deep.as_bytes()),
        Err(LexError::TooDeep { limit: 2, .. })
    ));
    let accepted = PyLexer::new(Limits { max_depth: 3, ..Limits::default() })
        .extract("m.py", deep.as_bytes())
        .expect("shallower f-string nest");
    check_invariants(deep, &accepted);
    assert_calls(&accepted, &[("owner", "live"), ("owner", "keep")]);
}

fn suite(body: &str, inner: &str, inside: &str) -> String {
    format!(
        "anchor()\r\ndef outer():\r\n{body}def inner():\r\n{inner}x = (\r\n{inside}live()\r\n{inner})\r\n\r\n{inner}# note\r\n{inner}keep()\r\n{body}return outer_call()\r\n"
    )
}

fn indent_rows() -> Vec<(&'static str, &'static str)> {
    vec![
        ("", "anchor"),
        ("outer::inner", "live"),
        ("outer::inner", "keep"),
        ("outer", "outer_call"),
    ]
}

/// PY2. Space and tab layouts match; a bad dedent is Partial at that line.
/// Not a defect: logical indents already compose continuations, blanks, comments and CRLF.
#[test]
fn py2_indentation_composition() {
    let spaces = suite("        ", "                ", "");
    let tabs = suite("\t", "\t\t", "\t");
    let spaced = extract("nest.py", &spaces);
    let tabbed = extract("nest.py", &tabs);
    check_invariants(&spaces, &spaced);
    check_invariants(&tabs, &tabbed);
    assert_eq!(spaced.completion, Completion::Complete);
    assert_eq!(tabbed.completion, Completion::Complete);
    assert_eq!(names(&spaced), ["nest.py", "outer", "outer::inner"]);
    assert_eq!(names(&tabbed), names(&spaced));
    assert_calls(&spaced, &indent_rows());
    assert_eq!(call_rows(&spaced), call_rows(&tabbed));

    let bad = "anchor()\r\ndef outer():\r\n        def inner():\r\n                x = (\r\nlive()\r\n                )\r\n\r\n                # note\r\n          bad()\r\n        keep()\r\n";
    let facts = extract("nest.py", bad);
    check_invariants(bad, &facts);
    assert_eq!(facts.completion, Completion::Partial);
    let byte = bad.find("          bad()").expect("bad indent") as u32;
    let line = bad[..byte as usize].bytes().filter(|item| *item == b'\n').count() as u32 + 1;
    assert_eq!(facts.diagnostics.len(), 1);
    assert_eq!(facts.diagnostics[0].reason, DiagnosticReason::InconsistentIndent);
    assert_eq!(facts.diagnostics[0].byte, byte);
    assert_eq!(facts.diagnostics[0].line, line);
    assert_eq!(names(&facts), ["nest.py", "outer", "outer::inner"]);
    let live = facts.references.iter().find(|item| item.fact.name == "live").expect("live");
    assert_eq!(live.fact.scope, "outer::inner");
    assert_eq!(live.fact.src, node(&facts, "outer::inner").fact.id);
    assert_eq!(spaced.completion, Completion::Complete);
}

fn assert_redacted(error: &LexError) {
    let display = error.to_string();
    let debug = format!("{error:?}");
    assert!(!display.contains(SENTINEL), "{display}");
    assert!(!debug.contains(SENTINEL), "{debug}");
}

/// PY3. Byte and fact ceilings refuse exactly, and errors omit source sentinels.
/// Not a defect: ceilings already refuse on the exact boundary and errors do not echo source.
#[test]
fn py3_wide_input_budget_and_error_redaction() {
    let limit = Limits::CEILING.max_bytes;
    let head = "def live():\n    return 1\nx = '''";
    let tail = "'''\n";
    let fill = limit - head.len() - tail.len();
    let mut source = String::with_capacity(limit);
    source.push_str(head);
    source.extend(std::iter::repeat('a').take(fill));
    source.push_str(tail);
    assert_eq!(source.len(), limit);
    let wide = PyLexer::default().extract("w.py", source.as_bytes()).expect("ceiling file");
    assert_eq!(wide.completion, Completion::Complete);
    assert_eq!(names(&wide), ["w.py", "live"]);
    let mut over = source.into_bytes();
    over.push(b'\n');
    match PyLexer::default().extract("w.py", &over) {
        Err(error @ LexError::TooLarge { bytes, limit: got }) => {
            assert_eq!(bytes, limit + 1);
            assert_eq!(got, limit);
            assert_redacted(&error);
        }
        other => panic!("expected TooLarge, got {other:?}"),
    }

    let body = "def f():\n    live()\n";
    let enough = PyLexer::new(Limits { max_facts: 3, ..Limits::default() });
    let tight = PyLexer::new(Limits { max_facts: 2, ..Limits::default() });
    let accepted = enough.extract("f.py", body.as_bytes()).expect("fact ceiling");
    assert_calls(&accepted, &[("f", "live")]);
    match tight.extract("f.py", body.as_bytes()) {
        Err(error @ LexError::TooManyFacts { limit: 2 }) => assert_redacted(&error),
        other => panic!("expected TooManyFacts, got {other:?}"),
    }
    let one = PyLexer::new(Limits { max_facts: 1, ..Limits::default() });
    assert!(one.extract("f.py", b"pass\n").is_ok());
    match PyLexer::new(Limits { max_facts: 0, ..Limits::default() }).extract("f.py", b"pass\n") {
        Err(error @ LexError::TooManyFacts { limit: 0 }) => assert_redacted(&error),
        other => panic!("expected zero-fact refusal, got {other:?}"),
    }

    let mut malformed = SENTINEL.as_bytes().to_vec();
    malformed.push(0xff);
    malformed.extend(SENTINEL.as_bytes());
    match PyLexer::default().extract("../bad.py", &malformed) {
        Err(error @ LexError::InvalidUtf8(_)) => assert_redacted(&error),
        other => panic!("expected InvalidUtf8, got {other:?}"),
    }
    let text = format!("{SENTINEL} = 1\n");
    match PyLexer::default().extract("../bad.py", text.as_bytes()) {
        Err(error @ LexError::InvalidId(_)) => assert_redacted(&error),
        other => panic!("expected InvalidId, got {other:?}"),
    }
    match PyLexer::default().extract("notes.md", text.as_bytes()) {
        Err(error @ LexError::UnsupportedDialect) => assert_redacted(&error),
        other => panic!("expected UnsupportedDialect, got {other:?}"),
    }
    let control = extract("ok.py", "def live():\n    pass\n");
    assert_eq!(control.completion, Completion::Complete);
    assert_eq!(names(&control), ["ok.py", "live"]);
}
