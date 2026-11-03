// SPDX-License-Identifier: Apache-2.0
//! Every refusal path of the public extract API, each next to its positive control.
//! Checks PARSERS-v1 limits (bytes, nesting, records) and malformed-input behaviour;
//! deep inputs at the nesting ceiling must parse on the worker without exhausting its stack.
//! Uses only the public API.
#![forbid(unsafe_code)]
use dctx_lex_rust::{Completion, Coverage, Limits, Precision, Refusal, extract};

fn complete(source: &str) -> bool {
    extract(source.as_bytes(), &Limits::default())
        .is_ok_and(|f| f.completion == Completion::Complete && f.coverage == Coverage::Syntax)
}

fn too_deep(source: &str) -> bool {
    extract(source.as_bytes(), &Limits::default()).is_ok_and(|f| {
        matches!(f.completion, Completion::Partial(Refusal::TooDeep { limit: 2048, .. }))
    })
}

fn body(expr: &str) -> String {
    format!("fn deep() {{ let _ = {expr}; }}\n")
}

#[test]
fn oversized_input_is_refused_before_parsing() {
    let source = "fn a() {}";
    let exact = Limits::narrowed(source.len(), 64, 100);
    assert!(extract(source.as_bytes(), &exact).is_ok());
    let tight = Limits::narrowed(source.len() - 1, 64, 100);
    assert_eq!(
        extract(source.as_bytes(), &tight),
        Err(Refusal::TooLarge { bytes: 9, limit: 8 })
    );
}

#[test]
fn non_utf8_is_refused_without_facts() {
    assert_eq!(
        extract(b"fn a() {}\xc3", &Limits::default()),
        Err(Refusal::InvalidUtf8 { valid_up_to: 9 })
    );
    assert!(complete("fn a() { \"é\"; }"));
}

#[test]
fn deep_inputs_at_the_ceiling_parse_and_beyond_it_are_refused() {
    type Shape = (&'static str, fn(usize) -> String, usize, usize);
    let shapes: [Shape; 7] = [
        ("unary", |n| format!("{}x", "-".repeat(n)), 2000, 2100),
        ("parens", |n| format!("{}x{}", "(".repeat(n), ")".repeat(n)), 2000, 2100),
        ("chain", |n| format!("x{}", ".a()".repeat(n)), 670, 700),
        ("binary", |n| format!("{}b", "a + ".repeat(n)), 1000, 1100),
        ("closures", |n| format!("{}a", "|a| ".repeat(n)), 670, 700),
        (
            "generics",
            |n| format!("Vec::<{}u8{}::new()", "Vec<".repeat(n - 1), ">".repeat(n)),
            670,
            700,
        ),
        ("else-if", |n| format!("{}{{ 2 }}", "if c { 1 } else ".repeat(n)), 500, 600),
    ];
    for (shape, nested, fits, exceeds) in shapes {
        assert!(complete(&body(&nested(fits))), "{shape} x {fits} should parse");
        assert!(too_deep(&body(&nested(exceeds))), "{shape} x {exceeds} should be refused");
    }
}

#[test]
fn refused_nesting_still_recovers_declarations() {
    let source = format!("/// Kept.\npub fn kept() {{}}\n{}", body(&"-".repeat(3000)));
    let facts = extract(source.as_bytes(), &Limits::default()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(facts.coverage, Coverage::Opaque);
    let names: Vec<&str> = facts.declarations.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["kept", "deep"]);
    assert_eq!(facts.declarations[0].doc.as_deref(), Some("Kept."));
    assert!(facts.declarations[0].public);
}

#[test]
fn syntax_and_lex_errors_return_partial_line_scan_facts() {
    let broken = "pub struct Kept;\nfn half(x: u8 -> ) {\n}\n";
    let facts = extract(broken.as_bytes(), &Limits::default()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(facts.completion, Completion::Partial(Refusal::Syntax { line: 2 }));
    assert!(facts.declarations.iter().all(|d| d.precision == Precision::LineScan));
    assert_eq!(facts.declarations.len(), 2);
    let unterminated = "fn kept() {}\nconst S: &str = \"open;\n";
    let facts = extract(unterminated.as_bytes(), &Limits::default()).unwrap_or_else(|e| panic!("{e}"));
    assert!(matches!(facts.completion, Completion::Partial(Refusal::Lex { .. })));
    assert_eq!(facts.declarations[0].name, "kept");
    assert!(complete("pub struct Kept;\nfn whole(x: u8) -> u8 { x }\n"));
}

#[test]
fn record_limit_truncates_and_reports() {
    let source = "fn a() {} fn b() {} fn c() {}";
    let two = Limits::narrowed(1 << 20, 64, 2);
    let facts = extract(source.as_bytes(), &two).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(facts.declarations.len(), 2);
    assert_eq!(facts.completion, Completion::Partial(Refusal::TooManyRecords { limit: 2 }));
    let three = Limits::narrowed(1 << 20, 64, 3);
    let facts = extract(source.as_bytes(), &three).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(facts.completion, Completion::Complete);
}

#[test]
fn narrowed_nesting_refuses_earlier() {
    let source = body(&format!("{}x", "-".repeat(40)));
    let narrow = Limits::narrowed(1 << 20, 16, 100);
    let facts = extract(source.as_bytes(), &narrow).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(facts.completion, Completion::Partial(Refusal::TooDeep { line: 1, limit: 16 }));
    assert!(complete(&source));
}
