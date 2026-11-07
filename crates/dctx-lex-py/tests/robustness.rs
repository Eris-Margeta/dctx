// SPDX-License-Identifier: Apache-2.0
//! Refusals, damaged input, limits and deterministic fuzz checks for the Python extractor.
//! Covers the Python structural extractor contract (decision 077/decision 078).
//! Uses common fact invariants and the complete inline fixture corpus with proptest.
#![forbid(unsafe_code)]

mod common;

use common::fixtures::{
    FIXTURES, FNEST, ONELINER, PAREN_SRC, PARTIAL_SRC, TRIPLE_CLOSED, TRIPLE_OPEN,
};
use common::{call_names, check_invariants, extract, names, node};
use dctx_lex_py::{
    Completion, DiagnosticReason, LexError, LexedFile, Limits, PARSER_ID, PARSER_REVISION,
    Precision, PyLexer,
};
use proptest::prelude::*;

#[test]
fn parser_identity_and_refusal_strings() {
    assert_eq!(PARSER_ID, "dctx-lex-py");
    assert_eq!(PARSER_REVISION, 1);
    assert_eq!(
        LexError::UnsupportedDialect.to_string(),
        "path is not a Python file"
    );
    assert_eq!(
        LexError::TooLarge { bytes: 9, limit: 4 }.to_string(),
        "file of 9 bytes exceeds the 4-byte limit"
    );
    assert_eq!(
        LexError::TooManyFacts { limit: 3 }.to_string(),
        "file yields more than 3 facts"
    );
    assert_eq!(
        LexError::TooDeep { line: 4, limit: 8 }.to_string(),
        "nesting deeper than 8 at line 4"
    );
    let lexer = PyLexer::default();
    assert!(matches!(
        lexer.extract("x.js", b""),
        Err(LexError::UnsupportedDialect)
    ));
    assert!(matches!(
        lexer.extract("../x.py", b""),
        Err(LexError::InvalidId(_))
    ));
    assert!(matches!(
        lexer.extract("/abs.py", b""),
        Err(LexError::InvalidId(_))
    ));
    assert!(matches!(
        lexer.extract("a\\b.py", b""),
        Err(LexError::InvalidId(_))
    ));
    assert!(matches!(
        lexer.extract("x.py", &[0xff]),
        Err(LexError::InvalidUtf8(_))
    ));
    let small = PyLexer::new(Limits {
        max_bytes: 4,
        ..Limits::default()
    });
    assert!(matches!(
        small.extract("a.py", b"hello"),
        Err(LexError::TooLarge { bytes: 5, limit: 4 })
    ));
    assert!(small.extract("a.py", b"a=1").is_ok());
    let empty = extract("m.py", "");
    assert_eq!(empty.nodes.len(), 1);
    assert_eq!(empty.nodes[0].fact.lines, [1, 1]);
    assert_eq!(empty.edges, []);
    assert_eq!(empty.references, []);
    assert_eq!(empty.completion, Completion::Complete);
}

#[test]
fn damaged_files_keep_earlier_facts_and_label_later_ones_partial() {
    let facts = extract("p.py", PARTIAL_SRC);
    check_invariants(PARTIAL_SRC, &facts);
    assert_eq!(names(&facts), ["p.py", "kept", "also"]);
    assert_eq!(node(&facts, "kept").precision, Precision::Structural);
    assert_eq!(node(&facts, "also").precision, Precision::Partial);
    assert_eq!(facts.nodes[0].precision, Precision::Structural);
    assert_eq!(facts.references.len(), 1);
    assert_eq!(facts.references[0].fact.name, "call");
    assert_eq!(facts.references[0].fact.scope, "also");
    assert_eq!(facts.references[0].precision, Precision::Partial);
    assert!(
        facts
            .diagnostics
            .iter()
            .any(|item| item.reason == DiagnosticReason::UnterminatedString)
    );
    let open = extract("t.py", TRIPLE_OPEN);
    assert_eq!(names(&open), ["t.py", "kept"]);
    assert_eq!(open.completion, Completion::Partial);
    assert!(
        open.diagnostics
            .iter()
            .any(|item| item.reason == DiagnosticReason::UnterminatedString)
    );
    assert!(open.nodes.iter().all(|item| item.fact.name != "swallowed"));
    let closed = extract("c.py", TRIPLE_CLOSED);
    assert_eq!(names(&closed), ["c.py"]);
    assert_eq!(call_names(&closed), ["real"]);
    assert_eq!(closed.completion, Completion::Complete);
    let paren = extract("b.py", PAREN_SRC);
    assert_eq!(names(&paren), ["b.py", "kept"]);
    assert_eq!(node(&paren, "kept").precision, Precision::Structural);
    assert_eq!(paren.completion, Completion::Partial);
    assert!(
        paren
            .diagnostics
            .iter()
            .any(|item| item.reason == DiagnosticReason::UnbalancedBracket)
    );
    assert!(paren.nodes.iter().all(|item| item.fact.name != "later"));
    let many = extract("e.py", &"else\n".repeat(80));
    assert_eq!(many.diagnostics.len(), LexedFile::MAX_DIAGNOSTICS);
    assert_eq!(many.completion, Completion::Partial);
    assert!(
        many.diagnostics
            .iter()
            .all(|item| item.reason == DiagnosticReason::UnexpectedToken)
    );
}

#[test]
fn depth_and_fact_limits_refuse_with_a_positive_control() {
    let deep = PyLexer::new(Limits {
        max_depth: 2,
        ..Limits::default()
    });
    let bound = PyLexer::new(Limits {
        max_depth: 3,
        ..Limits::default()
    });
    assert!(matches!(
        deep.extract("d.py", ONELINER.as_bytes()),
        Err(LexError::TooDeep { limit: 2, .. })
    ));
    let accepted = bound.extract("d.py", ONELINER.as_bytes()).unwrap();
    assert_eq!(names(&accepted), ["d.py", "a", "a::b", "a::b::c"]);
    assert!(matches!(
        deep.extract("f.py", FNEST.as_bytes()),
        Err(LexError::TooDeep { limit: 2, .. })
    ));
    assert!(bound.extract("f.py", FNEST.as_bytes()).is_ok());
    let one = PyLexer::new(Limits {
        max_facts: 1,
        ..Limits::default()
    });
    let two = PyLexer::new(Limits {
        max_facts: 2,
        ..Limits::default()
    });
    let zero = PyLexer::new(Limits {
        max_facts: 0,
        ..Limits::default()
    });
    assert!(matches!(
        one.extract("i.py", b"import os\n"),
        Err(LexError::TooManyFacts { limit: 1 })
    ));
    assert!(two.extract("i.py", b"import os\n").is_ok());
    assert!(one.extract("e.py", b"").is_ok());
    assert!(matches!(
        zero.extract("e.py", b""),
        Err(LexError::TooManyFacts { limit: 0 })
    ));
}

#[test]
fn every_fixture_prefix_is_deterministic_and_bounded() {
    for source in FIXTURES {
        for end in 0..=source.len() {
            let bytes = &source.as_bytes()[..end];
            let first = PyLexer::default().extract("pkg/sample.py", bytes);
            let second = PyLexer::default().extract("pkg/sample.py", bytes);
            match (&first, &second) {
                (Ok(left), Ok(right)) => {
                    let text = std::str::from_utf8(bytes).unwrap();
                    check_invariants(text, left);
                    assert_eq!(left, right);
                }
                (Err(left), Err(right)) => assert_eq!(left.to_string(), right.to_string()),
                _ => panic!("non-deterministic prefix of length {end}"),
            }
            match first {
                Err(LexError::InvalidUtf8(_)) => assert!(std::str::from_utf8(bytes).is_err()),
                Ok(_)
                | Err(
                    LexError::TooDeep { .. }
                    | LexError::TooManyFacts { .. }
                    | LexError::TooLarge { .. },
                ) => {}
                Err(error) => panic!("unexpected {error}"),
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        for path in ["f.py", "f.pyw", "f.pyi"] {
            let first = PyLexer::default().extract(path, &bytes);
            let second = PyLexer::default().extract(path, &bytes);
            match (first, second) {
                (Ok(left), Ok(right)) => {
                    let text = std::str::from_utf8(&bytes).unwrap();
                    check_invariants(text, &left);
                    prop_assert_eq!(left, right);
                }
                (Err(left), Err(right)) => prop_assert_eq!(left.to_string(), right.to_string()),
                _ => prop_assert!(false, "non-deterministic outcome"),
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn truncated_fixture_prefixes_never_panic(index in 0..FIXTURES.len(), cut in 0usize..8_000) {
        let source = FIXTURES[index];
        let end = cut % (source.len() + 1);
        let bytes = &source.as_bytes()[..end];
        let first = PyLexer::default().extract("pkg/sample.py", bytes);
        let second = PyLexer::default().extract("pkg/sample.py", bytes);
        match (first, second) {
            (Ok(left), Ok(right)) => {
                let text = std::str::from_utf8(bytes).unwrap();
                check_invariants(text, &left);
                prop_assert_eq!(left, right);
            }
            (Err(left), Err(right)) => prop_assert_eq!(left.to_string(), right.to_string()),
            _ => prop_assert!(false, "non-deterministic outcome"),
        }
    }

    #[test]
    fn python_shaped_text_is_deterministic(
        source in r##"([A-Za-z_]{1,3}|def |class |import |from |async |return |[ \n\t()\[\]{}:.,=*'\"#@]|é|数){0,60}"##
    ) {
        let first = PyLexer::default().extract("s.py", source.as_bytes());
        let second = PyLexer::default().extract("s.py", source.as_bytes());
        match (first, second) {
            (Ok(left), Ok(right)) => {
                check_invariants(&source, &left);
                prop_assert_eq!(left, right);
            }
            (Err(left), Err(right)) => prop_assert_eq!(left.to_string(), right.to_string()),
            _ => prop_assert!(false, "non-deterministic outcome"),
        }
    }
}
