// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile inputs exercised only through the public API.
use dctx_lex_cfg::{
    CfgLexer, Completion, DiagnosticReason, Dialect, LexError, LexedFile, Limits, body_fingerprint,
};
use std::collections::BTreeSet;

const SAMPLES: [(&str, &str); 4] = [
    (
        "sample.json",
        "{\"path\":\"./café\",\"a\":[1,{\"b\":true}],\"u\":\"\\uD83D\\uDE00\"}",
    ),
    (
        "sample.jsonc",
        "/* lead */ {\"path\":\"./src\",// note\n\"a\":[1,2,],}",
    ),
    (
        "sample.toml",
        "# lead\n[a]\npath = './src'\nb = [{c = \"é\"}, 2]\n",
    ),
    (
        "sample.yaml",
        "---\na: &base\n  path: './src'\nb: [1, {c: \"é\"}]\nuse: *base\n",
    ),
];

fn check(source: &[u8], file: &LexedFile) {
    let lines = u32::try_from(source.iter().filter(|&&b| b == b'\n').count() + 1).unwrap();
    let ids: BTreeSet<_> = file.nodes.iter().map(|n| n.fact.id.as_str()).collect();
    assert_eq!(ids.len(), file.nodes.len());
    assert_eq!(file.edges.len() + 1, file.nodes.len());
    assert!(file.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
    assert!(file.diagnostics.windows(2).all(|w| w[0].byte <= w[1].byte));
    for diagnostic in &file.diagnostics {
        assert!(diagnostic.byte as usize <= source.len());
        assert!((1..=lines).contains(&diagnostic.line));
    }
    for node in &file.nodes {
        assert_eq!(node.fact.path, file.path);
        assert!((1..=lines).contains(&node.fact.lines[0]));
        assert!((node.fact.lines[0]..=lines).contains(&node.fact.lines[1]));
        assert_eq!(node.fact.fingerprint.len(), 64);
    }
    for edge in &file.edges {
        assert!(ids.contains(edge.fact.src.as_str()));
        assert!(ids.contains(edge.fact.dst.as_str()));
    }
    for reference in &file.references {
        assert!(ids.contains(reference.fact.src.as_str()));
    }
}

#[test]
fn empty_and_every_one_byte_input_in_every_dialect() {
    for (path, _) in SAMPLES {
        let lexer = CfgLexer::default();
        check(b"", &lexer.extract(path, b"").unwrap());
        for byte in 0..=u8::MAX {
            let bytes = [byte];
            let result = lexer.extract(path, &bytes);
            if byte.is_ascii() {
                check(&bytes, &result.unwrap());
            } else {
                assert!(matches!(result, Err(LexError::InvalidUtf8(_))));
            }
        }
    }
}

#[test]
fn every_truncation_and_invalid_utf8_is_repeatable() {
    for (path, sample) in SAMPLES {
        let lexer = CfgLexer::default();
        for end in 0..=sample.len() {
            let bytes = &sample.as_bytes()[..end];
            let first = lexer.extract(path, bytes);
            assert_eq!(first, lexer.extract(path, bytes), "{path} cutoff {end}");
            assert_eq!(
                format!("{first:?}").as_bytes(),
                format!("{:?}", lexer.extract(path, bytes)).as_bytes()
            );
            match first {
                Ok(file) => check(bytes, &file),
                Err(error) => assert!(matches!(error, LexError::InvalidUtf8(_))),
            }
        }
        for bytes in [
            &b"\xff"[..],
            &b"\xc0\xaf"[..],
            &b"a\xed\xa0\x80"[..],
            &b"\xf4\x90\x80\x80"[..],
        ] {
            assert!(matches!(
                lexer.extract(path, bytes),
                Err(LexError::InvalidUtf8(_))
            ));
        }
    }
}

#[test]
fn exact_byte_limits_zero_limits_and_clamped_ceiling() {
    for (path, _) in SAMPLES {
        for max_bytes in [0, 1, 32, Limits::CEILING.max_bytes] {
            let lexer = CfgLexer::new(Limits {
                max_bytes,
                ..Limits::default()
            });
            let mut bytes = vec![b' '; max_bytes];
            check(&bytes, &lexer.extract(path, &bytes).unwrap());
            bytes.push(b' ');
            assert_eq!(
                lexer.extract(path, &bytes),
                Err(LexError::TooLarge {
                    bytes: max_bytes + 1,
                    limit: max_bytes
                })
            );
        }
        let no_facts = CfgLexer::new(Limits {
            max_facts: 0,
            ..Limits::default()
        });
        assert_eq!(
            no_facts.extract(path, b""),
            Err(LexError::TooManyFacts { limit: 0 })
        );
    }
    let wide = Limits {
        max_bytes: usize::MAX,
        max_depth: u32::MAX,
        max_facts: usize::MAX,
    };
    assert_eq!(wide.narrowed(), Limits::CEILING);
    assert_eq!(Limits::default().narrowed(), Limits::CEILING);
    let bytes = vec![b' '; Limits::CEILING.max_bytes + 1];
    assert!(matches!(
        CfgLexer::new(wide).extract("a.json", &bytes),
        Err(LexError::TooLarge { .. })
    ));
}

#[test]
fn depth_and_fact_boundaries_include_discarded_duplicate_values() {
    for (path, at, over) in [
        ("a.json", "{\"a\":1}", "{\"a\":{\"b\":1}}"),
        ("a.jsonc", "{\"a\":1}", "{\"a\":{\"b\":1}}"),
        ("a.toml", "a = 1", "a = {b = 1}"),
        ("a.yaml", "a: 1", "a:\n  b: 1"),
    ] {
        let lexer = CfgLexer::new(Limits {
            max_depth: 1,
            max_facts: 2,
            ..Limits::default()
        });
        assert!(lexer.extract(path, at.as_bytes()).is_ok());
        assert!(matches!(
            lexer.extract(path, over.as_bytes()),
            Err(LexError::TooDeep { .. })
        ));
        let lexer = CfgLexer::new(Limits {
            max_facts: 1,
            ..Limits::default()
        });
        assert!(matches!(
            lexer.extract(path, at.as_bytes()),
            Err(LexError::TooManyFacts { limit: 1 })
        ));
    }
    for path in ["a.json", "a.jsonc"] {
        let lexer = CfgLexer::new(Limits {
            max_depth: 3,
            ..Limits::default()
        });
        assert!(lexer.extract(path, b"{\"a\":1,\"a\":[[0]]}").is_ok());
        assert!(matches!(
            lexer.extract(path, b"{\"a\":1,\"a\":[[[0]]]}"),
            Err(LexError::TooDeep { .. })
        ));
    }
    // Exercise the actual nesting ceiling, without depending on process stack size.
    let at = format!("{}{}", "[".repeat(256), "]".repeat(256));
    assert!(CfgLexer::default().extract("a.json", at.as_bytes()).is_ok());
    let over = format!("[{at}]");
    assert!(matches!(
        CfgLexer::default().extract("a.json", over.as_bytes()),
        Err(LexError::TooDeep { limit: 256, .. })
    ));
}

#[test]
fn duplicates_keep_first_value_and_reordering_preserves_named_facts() {
    for (path, duplicate, reordered) in [
        ("a.json", "{\"a\":1,\"a\":2,\"b\":3}", "{\"b\":3,\"a\":1}"),
        ("a.jsonc", "{\"a\":1,\"a\":2,\"b\":3}", "{\"b\":3,\"a\":1}"),
        ("a.toml", "a = 1\na = 2\nb = 3\n", "b = 3\na = 1\n"),
        ("a.yaml", "a: 1\na: 2\nb: 3\n", "b: 3\na: 1\n"),
    ] {
        let lexer = CfgLexer::default();
        let first = lexer.extract(path, duplicate.as_bytes()).unwrap();
        assert_eq!(first.completion, Completion::Partial);
        assert!(
            first
                .diagnostics
                .iter()
                .any(|d| d.reason == DiagnosticReason::DuplicateKey)
        );
        let other = lexer.extract(path, reordered.as_bytes()).unwrap();
        for name in ["a", "b"] {
            let a = &first
                .nodes
                .iter()
                .find(|n| n.fact.name == name)
                .unwrap()
                .fact;
            let b = &other
                .nodes
                .iter()
                .find(|n| n.fact.name == name)
                .unwrap()
                .fact;
            assert_eq!((&a.id, &a.signature), (&b.id, &b.signature));
        }
    }
}

#[test]
fn shared_lexer_concurrently_emits_identical_bytes_even_for_parallel_finish() {
    let lexer = CfgLexer::default();
    let large = format!(
        "{{{}}}",
        (0..1100)
            .map(|i| format!("\"k{i}\":{i}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    for (path, source) in SAMPLES.into_iter().chain([("large.json", large.as_str())]) {
        let expected = format!("{:?}", lexer.extract(path, source.as_bytes()));
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    let lexer = &lexer;
                    let expected = &expected;
                    scope.spawn(move || {
                        let result = lexer.extract(path, source.as_bytes());
                        assert_eq!(format!("{result:?}").as_bytes(), expected.as_bytes());
                        assert_eq!(Dialect::for_path(path), Some(result.unwrap().dialect));
                        assert_eq!(body_fingerprint(source), body_fingerprint(source));
                        assert_eq!(Limits::default().narrowed(), Limits::CEILING);
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap();
            }
        });
    }
}

#[test]
fn public_helpers_handle_hostile_strings_and_paths() {
    // These helpers accept str, so invalid UTF-8 is excluded by their types.
    for byte in 0..=127_u8 {
        let input = [byte];
        let text = std::str::from_utf8(&input).unwrap();
        assert_eq!(body_fingerprint(text), body_fingerprint(text));
        assert_eq!(Dialect::for_path(text), None);
    }
    for size in [Limits::CEILING.max_bytes, Limits::CEILING.max_bytes + 1] {
        let path = "a".repeat(size) + ".JSON";
        assert_eq!(Dialect::for_path(&path), Some(Dialect::Json));
    }
    for end in 0..="cfg/a.jsonc".len() {
        let path = &"cfg/a.jsonc"[..end];
        assert_eq!(Dialect::for_path(path), Dialect::for_path(path));
    }
    for source in ["", "x", "é", "\0", " a\u{b}\u{a0} b ", "a a", "b a"] {
        for end in 0..=source.len() {
            if let Some(prefix) = source.get(..end) {
                assert_eq!(
                    body_fingerprint(prefix).as_bytes(),
                    body_fingerprint(prefix).as_bytes()
                );
                assert_eq!(body_fingerprint(prefix).len(), 64);
            }
        }
    }
    for source in [
        "a".repeat(Limits::CEILING.max_bytes),
        "a".repeat(Limits::CEILING.max_bytes + 1),
    ] {
        assert_eq!(body_fingerprint(&source), body_fingerprint(&source));
    }
    assert_eq!(body_fingerprint(" a\n\t b "), body_fingerprint("a b"));
    for path in ["", "x", "x.json/child", "x.json\0", "x.json "] {
        assert_eq!(Dialect::for_path(path), None);
    }
    for (path, _) in SAMPLES {
        assert_eq!(Dialect::for_path(path), Dialect::for_path(path));
    }
    for path in ["../a.json", "/a.json", "a/../b.toml", "a\0.yaml"] {
        assert!(matches!(
            CfgLexer::default().extract(path, b""),
            Err(LexError::InvalidId(_))
        ));
    }
}

#[test]
fn malformed_json_numbers_are_diagnosed_in_both_dialects() {
    for path in ["a.json", "a.jsonc"] {
        for number in ["01", "-01", "1.", "1e", "1e+", "1e-", "-", ".1"] {
            let input = format!("{{\"a\":{number}}}");
            let file = CfgLexer::default().extract(path, input.as_bytes()).unwrap();
            assert_eq!(file.completion, Completion::Partial, "{input}");
        }
        for number in ["0", "-0", "10", "0.1", "-1.2e+3", "1E-9"] {
            let input = format!("{{\"a\":{number}}}");
            assert_eq!(
                CfgLexer::default()
                    .extract(path, input.as_bytes())
                    .unwrap()
                    .completion,
                Completion::Complete,
                "{input}"
            );
        }
    }
}

#[test]
fn json_raw_control_characters_are_diagnosed_with_and_without_escapes() {
    for path in ["a.json", "a.jsonc"] {
        for control in 0..=31_u8 {
            for prefix in ["", "\\t"] {
                let input = format!("{{\"a\":\"{prefix}x{}y\"}}", char::from(control));
                assert_eq!(
                    CfgLexer::default()
                        .extract(path, input.as_bytes())
                        .unwrap()
                        .completion,
                    Completion::Partial,
                    "{input:?}"
                );
            }
        }
        assert_eq!(
            CfgLexer::default()
                .extract(path, br#"{"a":"x\ty\u0000"}"#)
                .unwrap()
                .completion,
            Completion::Complete
        );
    }
}

#[test]
fn duplicate_toml_inline_tables_do_not_consume_following_assignments() {
    for discarded in ["{b = 2}", "{b = {c = 2}}", "[{b = 2}, {c = 3}]"] {
        let input = format!("a = 1\na = {discarded}\nkeep = 3\nlast = 4\n");
        let file = CfgLexer::default()
            .extract("a.toml", input.as_bytes())
            .unwrap();
        for (name, value) in [("a", "1"), ("keep", "3"), ("last", "4")] {
            assert_eq!(
                file.nodes
                    .iter()
                    .find(|n| n.fact.name == name)
                    .unwrap()
                    .fact
                    .signature,
                value
            );
        }
        assert_eq!(file.diagnostics.len(), 1, "{input}: {:?}", file.diagnostics);
    }
}

#[test]
fn yaml_escapes_decode_unicode_and_refuse_invalid_sequences() {
    for (encoded, decoded) in [
        (r"\x41", "A"),
        (r"\u00e9", "é"),
        (r"\U0001f600", "😀"),
        (r"\r", ""),
        (r".\/src", "./src"),
    ] {
        let input = format!("path: \"{encoded}\"\n");
        let file = CfgLexer::default()
            .extract("a.yaml", input.as_bytes())
            .unwrap();
        assert_eq!(file.completion, Completion::Complete);
        assert_eq!(file.nodes[1].fact.signature, decoded);
    }
    for encoded in [r"\q", r"\é", r"\uD800", r"\U00110000", r"\xG0", r"\u123"] {
        let input = format!("a: \"{encoded}\"\nkeep: 3\n");
        assert_eq!(
            CfgLexer::default()
                .extract("a.yaml", input.as_bytes())
                .unwrap()
                .completion,
            Completion::Partial,
            "{input}"
        );
    }
}

#[test]
fn every_refusal_names_a_next_step() {
    let invalid_utf8 = CfgLexer::default().extract("a.json", b"\xff").unwrap_err();
    let invalid_id = CfgLexer::default().extract("../a.json", b"").unwrap_err();
    for (error, next) in [
        (LexError::UnsupportedDialect, "use .json"),
        (invalid_id, "use a place-relative path"),
        (invalid_utf8, "re-encode"),
        (LexError::TooLarge { bytes: 2, limit: 1 }, "split or reduce"),
        (LexError::TooDeep { line: 1, limit: 0 }, "reduce nesting"),
        (
            LexError::TooManyFacts { limit: 0 },
            "split the configuration",
        ),
    ] {
        assert!(error.to_string().contains(next), "{error}");
    }
}

#[test]
fn bom_diagnostic_offsets_index_the_original_input() {
    for (path, input) in [
        ("a.json", "\u{feff}{\"a\":1,\"a\":2}"),
        ("a.jsonc", "\u{feff}{\"a\":1,\"a\":2}"),
        ("a.toml", "\u{feff}a = 1\na = 2\n"),
        ("a.yaml", "\u{feff}a: 1\na: 2\n"),
    ] {
        let file = CfgLexer::default().extract(path, input.as_bytes()).unwrap();
        let duplicate = file
            .diagnostics
            .iter()
            .find(|d| d.reason == DiagnosticReason::DuplicateKey)
            .unwrap();
        let expected = if path.ends_with("json") || path.ends_with("jsonc") {
            input.rfind("\"a\"").unwrap()
        } else {
            input.rfind('a').unwrap()
        };
        assert_eq!(duplicate.byte as usize, expected, "{path}");
    }
}

#[test]
fn discarded_toml_dotted_keys_cannot_bypass_the_depth_limit() {
    let lexer = CfgLexer::new(Limits {
        max_depth: 3,
        ..Limits::default()
    });
    for input in [
        "a = 1\na = {b.c.d = 2}\n",
        "a = 1\na.b.c.d = 2\n",
        "a = {b = 1, b.c.d = 2}\n",
    ] {
        assert!(
            matches!(
                lexer.extract("a.toml", input.as_bytes()),
                Err(LexError::TooDeep { limit: 3, .. })
            ),
            "{input}"
        );
        assert!(
            CfgLexer::new(Limits {
                max_depth: 4,
                ..Limits::default()
            })
            .extract("a.toml", input.as_bytes())
            .is_ok()
        );
    }
}

#[test]
fn yaml_multiline_quotes_keep_following_keys_after_many_breaks() {
    for quote in ['\'', '"'] {
        let input = format!("a: {quote}{}end{quote}\nkeep: 3\n", "word\n  ".repeat(4096));
        let file = CfgLexer::default()
            .extract("a.yaml", input.as_bytes())
            .unwrap();
        assert_eq!(file.completion, Completion::Complete);
        assert_eq!(
            file.nodes
                .iter()
                .find(|n| n.fact.name == "keep")
                .unwrap()
                .fact
                .signature,
            "3"
        );
    }
}

#[test]
fn fact_reference_and_diagnostic_budgets_are_enforced() {
    for (path, input) in [
        ("a.json", "{\"path\":\"./src\"}"),
        ("a.jsonc", "{\"path\":\"./src\"}"),
        ("a.toml", "path = './src'"),
        ("a.yaml", "path: './src'"),
    ] {
        let exact = CfgLexer::new(Limits {
            max_facts: 3,
            ..Limits::default()
        })
        .extract(path, input.as_bytes())
        .unwrap();
        assert_eq!(exact.nodes.len(), 2);
        assert_eq!(exact.references.len(), 1);
        assert_eq!(
            CfgLexer::new(Limits {
                max_facts: 2,
                ..Limits::default()
            })
            .extract(path, input.as_bytes()),
            Err(LexError::TooManyFacts { limit: 2 })
        );
    }
    for (path, input) in [
        ("a.json", format!("{{{}}}", vec!["\"a\":1"; 1000].join(","))),
        (
            "a.jsonc",
            format!("{{{}}}", vec!["\"a\":1"; 1000].join(",")),
        ),
        ("a.toml", "a = 1\n".repeat(1000)),
        ("a.yaml", "a: 1\n".repeat(1000)),
    ] {
        let lexer = CfgLexer::default();
        let file = lexer.extract(path, input.as_bytes()).unwrap();
        assert_eq!(file.diagnostics.len(), LexedFile::MAX_DIAGNOSTICS);
        assert_eq!(file.completion, Completion::Partial);
        assert_eq!(file.nodes.len(), 2);
        check(input.as_bytes(), &file);
        assert_eq!(file, lexer.extract(path, input.as_bytes()).unwrap());
    }
}

#[test]
fn maximum_key_length_and_abstention_keep_following_siblings() {
    for size in [512, 513] {
        let key = "k".repeat(size);
        for (path, input) in [
            ("a.json", format!("{{\"{key}\":1,\"keep\":2}}")),
            ("a.jsonc", format!("{{\"{key}\":1,\"keep\":2}}")),
            ("a.toml", format!("{key} = 1\nkeep = 2\n")),
            ("a.yaml", format!("{key}: 1\nkeep: 2\n")),
        ] {
            let file = CfgLexer::default().extract(path, input.as_bytes()).unwrap();
            assert!(file.nodes.iter().any(|n| n.fact.name == "keep"));
            assert_eq!(file.nodes.iter().any(|n| n.fact.name == key), size == 512);
            assert_eq!(file.completion == Completion::Complete, size == 512);
        }
    }
}

#[test]
fn rejected_toml_headers_do_not_attach_keys_to_the_previous_table() {
    for rejected in ["[a]", "[broken", "[\"#unsupported\"]"] {
        let input = format!("[a]\nx = 1\n[b]\ny = 2\n{rejected}\nleaked = 3\n[c]\nkeep = 4\n");
        let file = CfgLexer::default()
            .extract("a.toml", input.as_bytes())
            .unwrap();
        assert!(
            file.nodes.iter().all(|n| !n.fact.name.contains("leaked")),
            "{input}: {:?}",
            file.nodes
        );
        assert!(file.nodes.iter().any(|n| n.fact.name == "c.keep"));
    }
}
