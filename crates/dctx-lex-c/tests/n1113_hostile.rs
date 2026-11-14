// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile inputs through every public operation.
//! Fingerprints and dialects accept `str`, so invalid UTF-8 is tested at extraction.
//! Complete Debug renderings compare all public fields and order byte for byte;
//! these crates expose facts rather than a serialized wire format.
#![forbid(unsafe_code)]
use dctx_lex_c::{CLexer, Completion, Dialect, LexError, LexedFile, Limits, body_fingerprint};
use std::collections::BTreeSet;

const PATH: &str = "hostile.cpp";

fn check(bytes: &[u8], facts: &LexedFile) {
    let source = std::str::from_utf8(bytes).expect("accepted UTF-8");
    let lines = u32::try_from(source.bytes().filter(|b| *b == b'\n').count() + 1).unwrap();
    assert_eq!(facts.path, PATH);
    assert!(!facts.nodes.is_empty());
    let ids: BTreeSet<_> = facts.nodes.iter().map(|n| n.fact.id.as_str()).collect();
    assert_eq!(ids.len(), facts.nodes.len(), "duplicate identity");
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    for node in &facts.nodes {
        assert_eq!(node.fact.path, PATH);
        assert!(node.fact.lines[0] >= 1 && node.fact.lines[0] <= node.fact.lines[1]);
        assert!(node.fact.lines[1] <= lines);
        assert_eq!(node.fact.fingerprint.len(), 64);
    }
    for edge in &facts.edges {
        assert!(ids.contains(edge.fact.src.as_str()));
        assert!(ids.contains(edge.fact.dst.as_str()));
        assert_eq!(edge.fact.owner_path, PATH);
    }
    for reference in &facts.references {
        assert!(ids.contains(reference.fact.src.as_str()));
        assert_eq!(reference.fact.owner_path, PATH);
        assert!(reference.fact.lines[0] >= 1);
        assert!(reference.fact.lines[0] <= reference.fact.lines[1]);
        assert!(reference.fact.lines[1] <= lines);
    }
    assert!(facts.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
    assert!(facts.diagnostics.windows(2).all(|d| d[0].byte <= d[1].byte));
    for diagnostic in &facts.diagnostics {
        assert!(diagnostic.byte as usize <= bytes.len());
        assert!(diagnostic.line >= 1 && diagnostic.line <= lines);
    }
    assert_eq!(
        facts.completion == Completion::Complete,
        facts.diagnostics.is_empty()
    );
}

fn repeat(lexer: &CLexer, bytes: &[u8]) {
    let first = lexer.extract(PATH, bytes);
    let second = lexer.extract(PATH, bytes);
    assert_eq!(
        format!("{first:?}").as_bytes(),
        format!("{second:?}").as_bytes()
    );
    if let Ok(facts) = first {
        check(bytes, &facts);
    }
}

#[test]
fn empty_and_every_one_byte_input() {
    let lexer = CLexer::default();
    let empty = lexer.extract(PATH, b"").unwrap();
    assert_eq!(empty.nodes.len(), 1);
    assert!(empty.references.is_empty());
    repeat(&lexer, b"");
    for byte in 0..=u8::MAX {
        repeat(&lexer, &[byte]);
        assert_eq!(
            matches!(lexer.extract(PATH, &[byte]), Err(LexError::InvalidUtf8(_))),
            byte >= 128
        );
    }
}

#[test]
fn every_truncation_including_inside_utf8_is_repeatable() {
    for source in SAMPLES {
        for offset in 0..=source.len() {
            let bytes = &source.as_bytes()[..offset];
            repeat(&CLexer::default(), bytes);
            assert_eq!(
                matches!(
                    CLexer::default().extract(PATH, bytes),
                    Err(LexError::InvalidUtf8(_))
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
        match CLexer::default().extract(PATH, &bytes) {
            Err(LexError::InvalidUtf8(error)) => assert_eq!(error.valid_up_to(), 6),
            other => panic!("expected UTF-8 refusal: {other:?}"),
        }
        repeat(&CLexer::default(), &bytes);
    }
}

#[test]
fn byte_limits_are_inclusive_and_cannot_be_widened() {
    for limit in [0, 1, 64, Limits::CEILING.max_bytes] {
        let lexer = CLexer::new(Limits {
            max_bytes: limit,
            ..Limits::default()
        });
        let mut bytes = vec![b' '; limit];
        assert_eq!(
            lexer.extract(PATH, &bytes).unwrap().completion,
            Completion::Complete
        );
        bytes.push(0xff);
        assert!(
            matches!(lexer.extract(PATH, &bytes), Err(LexError::TooLarge { bytes, limit: actual }) if bytes == limit + 1 && actual == limit)
        );
    }
    let wide = Limits {
        max_bytes: usize::MAX,
        max_depth: u32::MAX,
        max_facts: usize::MAX,
    };
    assert_eq!(wide.narrowed(), Limits::CEILING);
    assert_eq!(Limits::default(), Limits::CEILING);
    assert_eq!(
        Limits {
            max_bytes: 0,
            max_depth: 0,
            max_facts: 0
        }
        .narrowed(),
        Limits {
            max_bytes: 0,
            max_depth: 0,
            max_facts: 0
        }
    );
    let too_big = vec![b' '; Limits::CEILING.max_bytes + 1];
    assert!(matches!(
        CLexer::new(wide).extract(PATH, &too_big),
        Err(LexError::TooLarge { .. })
    ));
}

#[test]
fn bracket_depth_limits_include_the_ceiling() {
    for depth in [0, 1, 8, Limits::CEILING.max_depth] {
        let lexer = CLexer::new(Limits {
            max_depth: depth,
            ..Limits::default()
        });
        let n = usize::try_from(depth).unwrap();
        let source = format!("{}{}", "(".repeat(n), ")".repeat(n));
        assert!(
            lexer.extract(PATH, source.as_bytes()).is_ok(),
            "depth {depth}"
        );
        let over = format!("({source})");
        assert!(
            matches!(lexer.extract(PATH, over.as_bytes()), Err(LexError::TooDeep { limit, .. }) if limit == depth)
        );
    }
}

#[test]
fn zero_fact_budget_refuses_even_the_module() {
    let lexer = CLexer::new(Limits {
        max_facts: 0,
        ..Limits::default()
    });
    for source in ["", " ", "// comment", "int x;"] {
        assert!(
            matches!(
                lexer.extract(PATH, source.as_bytes()),
                Err(LexError::TooManyFacts { limit: 0 })
            ),
            "{source:?}"
        );
    }
}

#[test]
fn duplicate_and_reordered_items_keep_distinct_owners() {
    for source in [
        "void a() { g(); }\nvoid b() { g(); }\nvoid a() { g(); }",
        "void b() { g(); }\nvoid a() { g(); }\nvoid a() { g(); }",
    ] {
        let facts = CLexer::default().extract(PATH, source.as_bytes()).unwrap();
        check(source.as_bytes(), &facts);
        assert_eq!(facts.nodes.len(), 4);
        let calls: Vec<_> = facts
            .references
            .iter()
            .filter(|r| r.fact.kind == dctx_map_model::EdgeKind::Calls)
            .collect();
        assert_eq!(calls.len(), 3);
        for (call, node) in calls.iter().zip(facts.nodes.iter().skip(1)) {
            assert_eq!(call.fact.src, node.fact.id);
            assert_eq!(call.fact.scope, node.fact.name);
        }

        assert!(
            facts
                .nodes
                .iter()
                .any(|n| n.fact.id.as_str().ends_with("#a~1"))
        );
        assert!(
            facts
                .nodes
                .iter()
                .any(|n| n.fact.id.as_str().ends_with("#a~2"))
        );
        let names: Vec<_> = facts
            .nodes
            .iter()
            .skip(1)
            .map(|n| n.fact.name.as_str())
            .collect();
        assert_eq!(
            names,
            if source.starts_with("void a") {
                ["a", "b", "a"]
            } else {
                ["b", "a", "a"]
            }
        );
        repeat(&CLexer::default(), source.as_bytes());
    }
}

#[test]
fn shared_extractor_and_helpers_are_thread_safe_and_repeatable() {
    let lexer = CLexer::default();
    let expected: Vec<_> = SAMPLES
        .iter()
        .map(|s| {
            (
                format!("{:?}", lexer.extract(PATH, s.as_bytes())),
                body_fingerprint(s),
            )
        })
        .collect();
    let expected_dialect = Dialect::for_path(PATH);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    for (source, (expected, hash)) in SAMPLES.iter().zip(&expected) {
                        assert_eq!(
                            &format!("{:?}", lexer.extract(PATH, source.as_bytes())),
                            expected
                        );
                        assert_eq!(&body_fingerprint(source), hash);
                        assert_eq!(Dialect::for_path(PATH), expected_dialect);
                        assert_eq!(Limits::default().narrowed(), Limits::CEILING);
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
fn fingerprint_public_helper_handles_boundaries_and_order() {
    assert_eq!(
        body_fingerprint(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        body_fingerprint("a"),
        "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"
    );
    assert_eq!(body_fingerprint("a a b"), body_fingerprint(" \ta\na  b "));
    assert_ne!(body_fingerprint("a a b"), body_fingerprint("b a a"));
    // This helper accepts str and has no size limit; extraction's ceiling does not apply.
    for size in [Limits::CEILING.max_bytes, Limits::CEILING.max_bytes + 1] {
        assert_eq!(body_fingerprint(&" ".repeat(size)), body_fingerprint(""));
    }
    for source in SAMPLES {
        for offset in 0..=source.len() {
            if let Some(prefix) = source.get(..offset) {
                let hash = body_fingerprint(prefix);
                assert_eq!(hash.len(), 64);
                assert!(
                    hash.bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                );
                assert_eq!(hash.as_bytes(), body_fingerprint(prefix).as_bytes());
            }
        }
    }
}

#[test]
fn diagnostic_flood_is_bounded_and_repeatable() {
    let source = ")\n".repeat(256);
    let facts = CLexer::default().extract(PATH, source.as_bytes()).unwrap();
    assert_eq!(facts.completion, Completion::Partial);
    assert_eq!(facts.diagnostics.len(), LexedFile::MAX_DIAGNOSTICS);
    check(source.as_bytes(), &facts);
    repeat(&CLexer::default(), source.as_bytes());
}

const SAMPLES: &[&str] = &[
    "/// Café.\n#include \"a.h\"\n#define N 1\\\n+2\nnamespace n { struct S { int f(int x) { return g(x); } }; }\n",
    "const char *s = R\"tag(é\\\"/*{})tag\";\n#if X\nint f();\n#else\nint f() { return 1; }\n#endif\n",
];

#[test]
fn fact_budget_is_inclusive_for_module_symbols_and_references() {
    let source = b"void f() { g(); g(); }";
    let normal = CLexer::default().extract(PATH, source).unwrap();
    let used = normal.nodes.len() + normal.references.len();
    assert!(used > 2);
    let exact = CLexer::new(Limits {
        max_facts: used,
        ..Limits::default()
    });
    assert_eq!(exact.extract(PATH, source).unwrap(), normal);
    let short = CLexer::new(Limits {
        max_facts: used - 1,
        ..Limits::default()
    });
    assert!(
        matches!(short.extract(PATH, source), Err(LexError::TooManyFacts { limit }) if limit == used - 1)
    );
    let module_only = CLexer::new(Limits {
        max_facts: 1,
        ..Limits::default()
    });
    assert!(module_only.extract(PATH, b"").is_ok());
}

#[test]
fn deterministic_token_pairs_and_narrow_limits_never_panic() {
    const TOKENS: &[&str] = &[
        "function", "int", "struct", "class", "import", "export", "type", "R\"", "\"é\"", "'",
        "/*", "*/", "(", ")", "[", "]", "{", "}", "::", "=>", "!", ";", "\n", "数", "#", "<", ">",
        "`", "${", "\\u", "\\",
    ];
    let lexer = CLexer::new(Limits {
        max_bytes: 64,
        max_depth: 8,
        max_facts: 3,
    });
    for left in TOKENS {
        for right in TOKENS {
            let source = format!("{left} {right}");
            repeat(&lexer, source.as_bytes());
        }
    }
}

#[test]
fn dialect_helper_uses_only_the_final_component_extension() {
    for path in [
        "",
        "x",
        ".",
        "dir.cpp/no_extension",
        "dir.ts/no_extension",
        "x.c/",
        "x.js/",
        "x.unknown",
    ] {
        assert_eq!(Dialect::for_path(path), None);
    }
    for path in ["a/b.CPP", "a/b.cpp"] {
        assert!(Dialect::for_path(path).is_some());
        for offset in 0..=path.len() {
            let prefix = &path[..offset];
            assert_eq!(Dialect::for_path(prefix), Dialect::for_path(prefix));
        }
    }
    // Path selection is not path validation and does not have an input-size bound.
    for size in [Limits::CEILING.max_bytes, Limits::CEILING.max_bytes + 1] {
        let path = "x".repeat(size) + ".cpp";
        assert!(Dialect::for_path(&path).is_some());
    }
}

#[test]
fn fact_ceiling_accepts_exactly_maximum_and_refuses_one_more() {
    let limit = Limits::CEILING.max_facts;
    let mut source = "#include <a>\n".repeat(limit - 1);
    let facts = CLexer::default().extract(PATH, source.as_bytes()).unwrap();
    assert_eq!(facts.nodes.len(), 1);
    assert_eq!(facts.references.len(), limit - 1);
    assert_eq!(facts.completion, Completion::Complete);
    drop(facts);
    source.push_str("#include <a>\n");
    assert!(
        matches!(CLexer::default().extract(PATH, source.as_bytes()), Err(LexError::TooManyFacts { limit: actual }) if actual == limit)
    );
}

#[test]
fn public_path_refusals_are_typed_and_repeatable() {
    let lexer = CLexer::default();
    for path in ["", "x", "x.txt"] {
        assert!(matches!(
            lexer.extract(path, b""),
            Err(LexError::UnsupportedDialect)
        ));
    }
    for path in ["../bad.c", "/bad.c", "a/../bad.c", "a//bad.c", "bad\0.c"] {
        assert!(
            matches!(lexer.extract(path, b""), Err(LexError::InvalidId(_))),
            "{path:?}"
        );
        let first = format!("{:?}", lexer.extract(path, b""));
        assert_eq!(
            first.as_bytes(),
            format!("{:?}", lexer.extract(path, b"")).as_bytes()
        );
    }
}

#[test]
fn every_supported_dialect_is_repeatable_at_every_truncation() {
    for (path, dialect) in [
        ("unit.c", Dialect::C),
        ("unit.H", Dialect::C),
        ("unit.C", Dialect::Cpp),
        ("unit.cc", Dialect::Cpp),
        ("unit.cpp", Dialect::Cpp),
        ("unit.CXX", Dialect::Cpp),
        ("unit.c++", Dialect::Cpp),
        ("unit.hpp", Dialect::Cpp),
        ("unit.hh", Dialect::Cpp),
        ("unit.hxx", Dialect::Cpp),
        ("unit.ixx", Dialect::Cpp),
        ("unit.cppm", Dialect::Cpp),
    ] {
        assert_eq!(Dialect::for_path(path), Some(dialect));
        for source in SAMPLES {
            for offset in 0..=source.len() {
                let bytes = &source.as_bytes()[..offset];
                let first = CLexer::default().extract(path, bytes);
                assert_eq!(
                    format!("{first:?}"),
                    format!("{:?}", CLexer::default().extract(path, bytes))
                );
                if let Ok(facts) = first {
                    assert_eq!(facts.dialect, dialect);
                }
            }
        }
    }
}

#[test]
fn every_error_message_names_a_recovery_step() {
    let lexer = CLexer::default();
    let invalid_utf8 = lexer.extract(PATH, &[0xff]).unwrap_err();
    let invalid_id = lexer.extract("../bad.c", b"").unwrap_err();
    for (error, action) in [
        (LexError::UnsupportedDialect, "use a supported extension"),
        (invalid_utf8, "transcode the source to UTF-8"),
        (invalid_id, "use a place-relative path"),
        (LexError::TooLarge { bytes: 9, limit: 8 }, "split the file"),
        (
            LexError::TooDeep { line: 7, limit: 2 },
            "reduce syntactic nesting",
        ),
        (LexError::TooManyFacts { limit: 0 }, "increase max_facts"),
    ] {
        let message = error.to_string();
        assert!(message.contains(action), "{message}");
        assert_eq!(message.as_bytes(), error.to_string().as_bytes());
    }
}

#[test]
fn long_unmatched_template_candidates_do_not_rescan_the_suffix() {
    // One trailing closer matches only the innermost '<'. Earlier candidates
    // used to rescan this entire suffix, once per '<', with quadratic work.
    let source = "< ".repeat(32_768) + ">;\nint kept;";
    let facts = CLexer::default().extract(PATH, source.as_bytes()).unwrap();
    assert!(facts.nodes.iter().any(|n| n.fact.name == "kept"));
    check(source.as_bytes(), &facts);
    repeat(&CLexer::default(), source.as_bytes());
}

#[test]
fn template_lookahead_is_invalidated_when_macros_change_brace_meaning() {
    let source = "#define OPEN {\ntemplate<class T> struct A OPEN T x; };\n#undef OPEN\ntemplate<class T> struct B { T x; };\n";
    let facts = CLexer::default().extract(PATH, source.as_bytes()).unwrap();
    for name in ["A", "A::x", "B", "B::x"] {
        assert!(
            facts.nodes.iter().any(|n| n.fact.name == name),
            "missing {name}"
        );
    }
    repeat(&CLexer::default(), source.as_bytes());
}

#[test]
fn duplicate_and_reordered_includes_preserve_source_order() {
    for (source, expected) in [
        (
            "#include <a>\n#include <b>\n#include <a>\n",
            ["a", "b", "a"],
        ),
        (
            "#include <b>\n#include <a>\n#include <a>\n",
            ["b", "a", "a"],
        ),
    ] {
        let facts = CLexer::default().extract(PATH, source.as_bytes()).unwrap();
        assert_eq!(
            facts
                .references
                .iter()
                .map(|r| r.fact.name.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        repeat(&CLexer::default(), source.as_bytes());
    }
}

#[test]
fn raw_string_delimiters_accept_sixteen_bytes_and_reject_seventeen() {
    for prefix in ["const char *s = ", "#define S "] {
        let delimiter = "a".repeat(16);
        let at = format!("{prefix}R\"{delimiter}(é /* {{ ){delimiter}\";\nint kept;\n");
        let facts = CLexer::default().extract(PATH, at.as_bytes()).unwrap();
        assert_eq!(facts.completion, Completion::Complete);
        assert!(facts.nodes.iter().any(|n| n.fact.name == "kept"));
        let delimiter = "a".repeat(17);
        let over = format!("{prefix}R\"{delimiter}(x){delimiter}\";\nint kept;\n");
        let facts = CLexer::default().extract(PATH, over.as_bytes()).unwrap();
        assert_eq!(facts.completion, Completion::Partial);
        assert!(
            facts
                .diagnostics
                .iter()
                .any(|d| d.reason == dctx_lex_c::DiagnosticReason::UnterminatedRawString)
        );
        repeat(&CLexer::default(), over.as_bytes());
    }
    // Recovery after a delimiter cut in the middle of a multibyte character.
    let source = format!("R\"{}é(x)\";\nint kept;", "a".repeat(15));
    for offset in 0..=source.len() {
        repeat(&CLexer::default(), &source.as_bytes()[..offset]);
    }
}
