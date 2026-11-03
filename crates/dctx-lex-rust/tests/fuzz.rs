// SPDX-License-Identifier: Apache-2.0
//! Property tests: arbitrary bytes and Rust-like token soup never panic, and every
//! result keeps the PARSERS-v1 invariants (in-range UTF-8 spans, ordered lines,
//! consistent labels, determinism). Uses proptest and the public API only.
#![forbid(unsafe_code)]
use dctx_lex_rust::{Completion, Coverage, Extraction, Limits, Precision, SourceSpan, extract};
use proptest::prelude::*;

const TOKENS: &[&str] = &[
    "fn", "pub", "struct", "enum", "trait", "impl", "for", "mod", "use", "const", "static", "type",
    "macro_rules!", "let", "match", "if", "else", "x", "Y", "r#type", "'a", "'b'", "\"s\"",
    "r#\"raw\"#", "b'x'", "1", "::", ":", ";", ",", ".", "->", "=>", "=", "<", ">", "(", ")",
    "{", "}", "[", "]", "#[test]", "#!", "///", "//!", "//", "/*", "*/", "\n", "|", "&", "*",
    "-", "!", "?", "é", "\u{feff}",
];

fn in_bounds(span: &SourceSpan, source: &str, lines: u32) -> bool {
    span.start <= span.end
        && span.end <= source.len()
        && source.is_char_boundary(span.start)
        && source.is_char_boundary(span.end)
        && 1 <= span.lines[0]
        && span.lines[0] <= span.lines[1]
        && span.lines[1] <= lines
}

fn check(source: &str, facts: &Extraction) {
    let lines = u32::try_from(source.split('\n').count()).unwrap_or(u32::MAX);
    let spans = facts
        .declarations
        .iter()
        .map(|d| &d.span)
        .chain(facts.imports.iter().map(|i| &i.span))
        .chain(facts.references.iter().map(|r| &r.span));
    for span in spans {
        assert!(in_bounds(span, source, lines), "{span:?} in {source:?}");
    }
    let expected = match facts.coverage {
        Coverage::Syntax => Precision::Syntax,
        Coverage::Opaque => Precision::LineScan,
    };
    assert!(facts.declarations.iter().all(|d| d.precision == expected));
    assert!(facts.imports.iter().all(|i| i.precision == expected));
    if facts.coverage == Coverage::Opaque {
        assert!(matches!(facts.completion, Completion::Partial(_)));
        assert_eq!(facts.references, []);
    }
    for (index, decl) in facts.declarations.iter().enumerate() {
        assert!(decl.parent.is_none_or(|p| p < index));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        if let Ok(facts) = extract(&bytes, &Limits::default()) {
            let source = std::str::from_utf8(&bytes).unwrap_or_default();
            check(source, &facts);
        }
    }

    #[test]
    fn token_soup_keeps_invariants_and_is_deterministic(
        picks in proptest::collection::vec(proptest::sample::select(TOKENS), 0..160)
    ) {
        let source = picks.join(" ");
        let first = extract(source.as_bytes(), &Limits::default());
        prop_assert!(first.is_ok());
        let facts = first.unwrap_or_else(|e| panic!("{e}"));
        check(&source, &facts);
        prop_assert_eq!(extract(source.as_bytes(), &Limits::default()), Ok(facts));
    }

    #[test]
    fn narrow_limits_never_panic(
        picks in proptest::collection::vec(proptest::sample::select(TOKENS), 0..80),
        nesting in 0_u32..12,
        records in 0_usize..6,
    ) {
        let source = picks.join("");
        let limits = Limits::narrowed(source.len(), nesting, records);
        if let Ok(facts) = extract(source.as_bytes(), &limits) {
            check(&source, &facts);
            prop_assert!(facts.declarations.len() + facts.imports.len() + facts.references.len() <= records);
        }
    }
}
