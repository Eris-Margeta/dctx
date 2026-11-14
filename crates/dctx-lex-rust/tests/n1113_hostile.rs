// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile inputs through the public extraction and limits APIs.
//! Debug bytes include every public field and traversal order; there is no wire encoder.
#![forbid(unsafe_code)]
use dctx_lex_rust::{
    Completion, Coverage, Extraction, Limits, Precision, Refusal, SourceSpan, extract,
};

const SAMPLES: &[&str] = &[
    "//! Café.\nuse crate::a::{b as c, d};\npub struct S;\nimpl S { pub fn f(&self) { g(); self.h(); } }\n",
    "\u{feff}#!run\nconst S: &str = r##\"é } /*\"##;\nmacro_rules! m { () => { g(); } }\n#[test]\nfn check() { m!(); }\n",
    "pub mod n {\n/// Kept.\npub fn f<'a>(s: &'a str) { let _ = '\\u{e9}'; }\n}\n/* broken\n",
];

fn check(bytes: &[u8], facts: &Extraction, limits: &Limits) {
    let source = std::str::from_utf8(bytes).unwrap();
    let lines = u32::try_from(source.bytes().filter(|b| *b == b'\n').count() + 1).unwrap();
    let span = |span: SourceSpan| {
        assert!(span.start <= span.end && span.end <= bytes.len());
        assert!(source.is_char_boundary(span.start) && source.is_char_boundary(span.end));
        assert!(span.lines[0] >= 1 && span.lines[0] <= span.lines[1] && span.lines[1] <= lines);
    };
    assert!(
        facts.declarations.len() + facts.imports.len() + facts.references.len()
            <= limits.max_records()
    );
    for (index, declaration) in facts.declarations.iter().enumerate() {
        span(declaration.span);
        assert!(declaration.parent.is_none_or(|p| p < index));
    }
    for import in &facts.imports {
        span(import.span);
    }
    for reference in &facts.references {
        span(reference.span);
        assert!(reference.from.is_none_or(|p| p < facts.declarations.len()));
    }
    if facts.coverage == Coverage::Opaque {
        assert!(matches!(facts.completion, Completion::Partial(_)));
        assert!(
            facts
                .declarations
                .iter()
                .all(|d| d.precision == Precision::LineScan)
        );
        assert!(
            facts
                .imports
                .iter()
                .all(|i| i.precision == Precision::LineScan)
        );
        assert!(facts.references.is_empty());
    }
}

fn repeat(bytes: &[u8], limits: &Limits) {
    let first = extract(bytes, limits);
    let second = extract(bytes, limits);
    assert_eq!(
        format!("{first:?}").as_bytes(),
        format!("{second:?}").as_bytes()
    );
    if let Ok(facts) = first {
        check(bytes, &facts, limits);
    }
}

#[test]
fn empty_and_every_one_byte_input() {
    let limits = Limits::default();
    let empty = extract(b"", &limits).unwrap();
    assert_eq!(empty.completion, Completion::Complete);
    assert!(
        empty.declarations.is_empty() && empty.imports.is_empty() && empty.references.is_empty()
    );
    repeat(b"", &limits);
    for byte in 0..=u8::MAX {
        repeat(&[byte], &limits);
        assert_eq!(
            matches!(extract(&[byte], &limits), Err(Refusal::InvalidUtf8 { .. })),
            byte >= 128
        );
    }
}

#[test]
fn every_truncation_including_inside_utf8_is_repeatable() {
    for source in SAMPLES {
        for offset in 0..=source.len() {
            let bytes = &source.as_bytes()[..offset];
            repeat(bytes, &Limits::default());
            assert_eq!(
                matches!(
                    extract(bytes, &Limits::default()),
                    Err(Refusal::InvalidUtf8 { .. })
                ),
                std::str::from_utf8(bytes).is_err()
            );
        }
    }
}

#[test]
fn malformed_utf8_reports_the_valid_prefix() {
    for bad in [
        &b"\xff"[..],
        &b"\xc0\x80"[..],
        &b"\xed\xa0\x80"[..],
        &b"\xf4\x90\x80\x80"[..],
        &b"\xe2\x82"[..],
    ] {
        let mut bytes = b"// ok\n".to_vec();
        bytes.extend_from_slice(bad);
        assert_eq!(
            extract(&bytes, &Limits::default()),
            Err(Refusal::InvalidUtf8 { valid_up_to: 6 })
        );
        repeat(&bytes, &Limits::default());
    }
}

#[test]
fn byte_limits_are_inclusive_and_cannot_be_widened() {
    for limit in [0, 1, 64, Limits::CEILING.max_bytes()] {
        let limits = Limits::narrowed(limit, u32::MAX, usize::MAX);
        let mut bytes = vec![b' '; limit];
        assert_eq!(
            extract(&bytes, &limits).unwrap().completion,
            Completion::Complete
        );
        bytes.push(0xff);
        assert_eq!(
            extract(&bytes, &limits),
            Err(Refusal::TooLarge {
                bytes: limit + 1,
                limit
            })
        );
    }
    assert_eq!(
        Limits::narrowed(usize::MAX, u32::MAX, usize::MAX),
        Limits::CEILING
    );
    assert_eq!(Limits::default(), Limits::CEILING);
    let zero = Limits::narrowed(0, 0, 0);
    assert_eq!(
        (zero.max_bytes(), zero.max_nesting(), zero.max_records()),
        (0, 0, 0)
    );
    assert_eq!(
        extract(b"", &zero).unwrap().completion,
        Completion::Complete
    );
    let over = vec![b' '; Limits::CEILING.max_bytes() + 1];
    assert!(matches!(
        extract(&over, &Limits::narrowed(usize::MAX, u32::MAX, usize::MAX)),
        Err(Refusal::TooLarge { .. })
    ));
}

#[test]
fn nesting_cost_limits_are_inclusive_even_at_the_ceiling() {
    // Each unary '-' and the final identifier costs one token.
    for limit in [0, 1, 8, Limits::CEILING.max_nesting()] {
        let limits = Limits::narrowed(1 << 20, limit, 100);
        let source = "-".repeat(usize::try_from(limit).unwrap());
        let at = extract(source.as_bytes(), &limits).unwrap();
        assert!(!matches!(
            at.completion,
            Completion::Partial(Refusal::TooDeep { .. })
        ));
        let over = source + "x";
        let facts = extract(over.as_bytes(), &limits).unwrap();
        assert_eq!(
            facts.completion,
            Completion::Partial(Refusal::TooDeep { line: 1, limit })
        );
        check(over.as_bytes(), &facts, &limits);
    }
}

#[test]
fn record_budgets_cover_declarations_imports_and_references_together() {
    let source = b"use a; fn f() { g(); }";
    for limit in 0..=4 {
        let limits = Limits::narrowed(source.len(), 64, limit);
        let facts = extract(source, &limits).unwrap();
        let used = facts.declarations.len() + facts.imports.len() + facts.references.len();
        assert_eq!(used, limit.min(3));
        assert_eq!(
            facts.completion,
            if limit < 3 {
                Completion::Partial(Refusal::TooManyRecords { limit })
            } else {
                Completion::Complete
            }
        );
        check(source, &facts, &limits);
        repeat(source, &limits);
    }
    // A pre-existing syntax/lex refusal remains the primary reason in fallback.
    let malformed = b"pub struct Kept;\nfn broken(\n";
    for limit in [0, 1, 2] {
        let limits = Limits::narrowed(malformed.len(), 64, limit);
        let facts = extract(malformed, &limits).unwrap();
        assert!(matches!(
            facts.completion,
            Completion::Partial(Refusal::Lex { .. })
        ));
        check(malformed, &facts, &limits);
        repeat(malformed, &limits);
    }
}

#[test]
fn record_ceiling_accepts_exactly_maximum_and_marks_one_more_partial() {
    let limit = Limits::CEILING.max_records();
    let mut source = "use a;\n".repeat(limit);
    let facts = extract(source.as_bytes(), &Limits::default()).unwrap();
    assert_eq!(facts.completion, Completion::Complete);
    assert_eq!(facts.imports.len(), limit);
    drop(facts);
    source.push_str("use a;\n");
    let facts = extract(source.as_bytes(), &Limits::default()).unwrap();
    assert_eq!(
        facts.completion,
        Completion::Partial(Refusal::TooManyRecords { limit })
    );
    assert_eq!(facts.imports.len(), limit);
}

#[test]
fn duplicate_and_reordered_items_preserve_sites_and_owners() {
    for (source, names) in [
        (
            "fn a() { g(); } fn b() { g(); } fn a() { g(); }",
            ["a", "b", "a"],
        ),
        (
            "fn b() { g(); } fn a() { g(); } fn a() { g(); }",
            ["b", "a", "a"],
        ),
    ] {
        let facts = extract(source.as_bytes(), &Limits::default()).unwrap();
        assert_eq!(
            facts
                .declarations
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>(),
            names
        );
        assert_eq!(facts.references.len(), 3);
        assert_eq!(
            facts.references.iter().map(|r| r.from).collect::<Vec<_>>(),
            [Some(0), Some(1), Some(2)]
        );
        repeat(source.as_bytes(), &Limits::default());
    }
    let imports = b"use a; use b; use a;";
    let facts = extract(imports, &Limits::default()).unwrap();
    assert_eq!(
        facts
            .imports
            .iter()
            .map(|i| i.path.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "a"]
    );
}

#[test]
fn shared_limits_and_extraction_are_concurrently_repeatable() {
    let limits = Limits::default();
    let expected: Vec<_> = SAMPLES
        .iter()
        .map(|s| format!("{:?}", extract(s.as_bytes(), &limits)))
        .collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    for (source, expected) in SAMPLES.iter().zip(&expected) {
                        for _ in 0..3 {
                            assert_eq!(
                                &format!("{:?}", extract(source.as_bytes(), &limits)),
                                expected
                            );
                        }
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
    });
}

#[test]
fn deterministic_token_pairs_and_narrow_limits_never_panic() {
    const TOKENS: &[&str] = &[
        "fn", "pub", "struct", "use", "r#", "\"é\"", "'", "/*", "*/", "(", ")", "[", "]", "{", "}",
        "::", "=>", "!", ";", "\n", "数", "#", "<", ">",
    ];
    for left in TOKENS {
        for right in TOKENS {
            let source = format!("{left} {right}");
            repeat(source.as_bytes(), &Limits::narrowed(64, 8, 2));
        }
    }
}

#[test]
fn every_refusal_message_names_a_recovery_step() {
    for (error, action) in [
        (Refusal::TooLarge { bytes: 9, limit: 8 }, "split the file"),
        (
            Refusal::InvalidUtf8 { valid_up_to: 2 },
            "transcode the source to UTF-8",
        ),
        (
            Refusal::TooDeep { line: 7, limit: 2 },
            "reduce syntactic nesting",
        ),
        (
            Refusal::Lex { line: 7 },
            "close literals, comments and delimiters",
        ),
        (Refusal::Syntax { line: 7 }, "repair Rust syntax"),
        (Refusal::TooManyRecords { limit: 0 }, "increase max_records"),
        (Refusal::Worker, "check OS thread/process and memory limits"),
    ] {
        let message = error.to_string();
        assert!(message.contains(action), "{message}");
        assert_eq!(message.as_bytes(), error.to_string().as_bytes());
    }
}
