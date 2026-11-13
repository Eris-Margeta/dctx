// SPDX-License-Identifier: Apache-2.0
//! Exhaustive small and boundary inputs; no random state or external resources.
use dctx_lex_md::{Document, Heading, HeadingStyle, LexError, Limits, Span, extract, extract_with};

const SAMPLES: [&str; 6] = [
    "\u{feff}---\ntitle: é\n---\n# Top\ntext `café` [x](./src)\n## Child\n",
    "> - # Hé\n>   [x](<a b> \"title\") and `` a ` b ``\n",
    "Title\n=====\n\n```rust\n# hidden\n```\n\n    # code\n",
    "# A\r\n[x](a(b)c 't') ![i](b) [x](#a)\r\n",
    "# A\n<!--\n# hidden\n-->\n\n[x](é)\n",
    "# A\n[unterminated](a(\n`unclosed\n~~~\n",
];

fn check(source: &str, document: &Document) {
    let lines = u32::try_from(source.lines().count().max(1)).unwrap();
    assert_eq!(document.lines, lines);
    let span = |s: Span| {
        assert!(s.start <= s.end);
        assert!(source.get(s.start..s.end).is_some(), "{source:?}: {s:?}");
        assert_eq!(s.text(source), &source[s.start..s.end]);
    };
    for (i, heading) in document.headings.iter().enumerate() {
        span(heading.span);
        span(heading.section);
        span(heading.body());
        assert!((1..=6).contains(&heading.level));
        assert!(heading.span.end <= heading.section.end);
        assert!((1..=lines).contains(&heading.lines[0]));
        assert!((heading.lines[0]..=lines).contains(&heading.lines[1]));
        assert!(heading.parent.is_none_or(|parent| parent < i));
        assert!(document.stable_name(i).is_some());
    }
    for reference in &document.references {
        span(reference.span);
        assert!(reference.span.start < reference.span.end);
        assert!(!reference.name.is_empty());
        assert!((1..=lines).contains(&reference.lines[0]));
        assert!((reference.lines[0]..=lines).contains(&reference.lines[1]));
        assert!(
            reference
                .owner
                .is_none_or(|owner| owner < document.headings.len())
        );
    }
    for block in &document.code_blocks {
        span(block.span);
        assert!((1..=lines).contains(&block.lines[0]));
        assert!((block.lines[0]..=lines).contains(&block.lines[1]));
    }
    if let Some(front) = document.front_matter {
        span(front);
    }
    assert_eq!(document.stable_name(document.headings.len()), None);
    assert_eq!(document.stable_name(usize::MAX), None);
}

#[test]
fn empty_and_every_one_byte_input_through_both_extractors() {
    check("", &extract(b"").unwrap());
    check("", &extract_with(b"", &Limits::default()).unwrap());
    for byte in 0..=u8::MAX {
        let bytes = [byte];
        let first = extract(&bytes);
        assert_eq!(first, extract_with(&bytes, &Limits::default()));
        if byte.is_ascii() {
            check(std::str::from_utf8(&bytes).unwrap(), &first.unwrap());
        } else {
            assert_eq!(first, Err(LexError::InvalidUtf8 { valid_up_to: 0 }));
        }
    }
}

#[test]
fn every_sample_truncation_preserves_invariants_and_identical_output_bytes() {
    for sample in SAMPLES {
        for end in 0..=sample.len() {
            let bytes = &sample.as_bytes()[..end];
            let first = extract(bytes);
            let repeated = extract_with(bytes, &Limits::default());
            assert_eq!(first, repeated, "cutoff {end} of {sample:?}");
            assert_eq!(
                format!("{first:?}").as_bytes(),
                format!("{repeated:?}").as_bytes()
            );
            match first {
                Ok(document) => check(std::str::from_utf8(bytes).unwrap(), &document),
                Err(error) => assert!(matches!(error, LexError::InvalidUtf8 { .. })),
            }
        }
    }
}

#[test]
fn invalid_utf8_reports_exact_valid_prefix_and_size_refusal_precedes_decoding() {
    for invalid in [
        &b"\xff"[..],
        &b"\xc0\xaf"[..],
        &b"\xed\xa0\x80"[..],
        &b"\xf4\x90\x80\x80"[..],
        &b"\xe2\x82"[..],
    ] {
        let mut bytes = b"# A\n".to_vec();
        bytes.extend_from_slice(invalid);
        let expected = Err(LexError::InvalidUtf8 { valid_up_to: 4 });
        assert_eq!(extract(&bytes), expected);
        assert_eq!(extract_with(&bytes, &Limits::default()), expected);
        assert_eq!(
            extract_with(
                &bytes,
                &Limits {
                    max_bytes: 4,
                    ..Limits::default()
                }
            ),
            Err(LexError::TooLarge {
                len: bytes.len(),
                max: 4
            })
        );
    }
}

#[test]
fn exact_and_over_default_and_custom_byte_limits() {
    for max in [0, 1, 64, Limits::default().max_bytes] {
        let limits = Limits {
            max_bytes: max,
            ..Limits::default()
        };
        let mut bytes = vec![b' '; max];
        assert!(extract_with(&bytes, &limits).is_ok());
        if max == Limits::default().max_bytes {
            assert_eq!(extract(&bytes), extract_with(&bytes, &limits));
        }
        bytes.push(b' ');
        let refusal = Err(LexError::TooLarge { len: max + 1, max });
        assert_eq!(extract_with(&bytes, &limits), refusal);
        if max == Limits::default().max_bytes {
            assert_eq!(extract(&bytes), refusal);
        }
    }
}

#[test]
fn container_and_bracket_limits_have_exact_positive_controls() {
    for depth in [0, 1, 4, Limits::default().max_depth] {
        let limits = Limits {
            max_depth: depth,
            ..Limits::default()
        };
        for marker in ["> ", "- "] {
            let at = marker.repeat(depth) + "# A";
            assert!(extract_with(at.as_bytes(), &limits).is_ok());
            let over = format!("{marker}{at}");
            assert_eq!(
                extract_with(over.as_bytes(), &limits),
                Err(LexError::TooDeep {
                    line: 1,
                    max: depth
                })
            );
        }
    }
    for max_brackets in [0, 1, 2, Limits::default().max_brackets] {
        let limits = Limits {
            max_brackets,
            ..Limits::default()
        };
        let at = "[".repeat(max_brackets) + "x](a)";
        let over = "[".repeat(max_brackets + 1) + "x](a)";
        for source in [at, over] {
            let result = extract_with(source.as_bytes(), &limits).unwrap();
            assert_eq!(result.references.len(), usize::from(max_brackets > 0));
            check(&source, &result);
        }
    }
}

#[test]
fn duplicate_and_reordered_headings_and_references_keep_source_order() {
    for sample in [
        "# A\n[x](p) [x](p)\n# B\n# A\n",
        "# B\n# A\n[x](p) [x](p)\n# A\n",
    ] {
        let document = extract(sample.as_bytes()).unwrap();
        check(sample, &document);
        let names: Vec<_> = (0..document.headings.len())
            .map(|i| document.stable_name(i).unwrap())
            .collect();
        assert_eq!(names.iter().filter(|n| n.starts_with("a~")).count(), 2);
        assert!(names.contains(&"a~1".to_string()));
        assert!(names.contains(&"a~2".to_string()));
        assert_eq!(document.references.len(), 2);
        assert_eq!(document.references[0].name, document.references[1].name);
        assert!(document.references[0].span.start < document.references[1].span.start);
        assert_eq!(
            document,
            extract_with(sample.as_bytes(), &Limits::default()).unwrap()
        );
    }
}

#[test]
fn simultaneous_calls_share_inputs_and_limits_without_output_changes() {
    let limits = Limits::default();
    for source in SAMPLES {
        let expected = format!("{:?}", extract(source.as_bytes()));
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|i| {
                    let limits = &limits;
                    let expected = &expected;
                    scope.spawn(move || {
                        let result = if i % 2 == 0 {
                            extract(source.as_bytes())
                        } else {
                            extract_with(source.as_bytes(), limits)
                        };
                        assert_eq!(format!("{result:?}").as_bytes(), expected.as_bytes());
                        check(source, &result.unwrap());
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
fn span_and_document_helpers_are_total_for_caller_constructed_values() {
    for source in ["", "x", "é", "é😀", "a a", "b a"] {
        for start in [0, 1, 2, source.len(), source.len() + 1, usize::MAX] {
            for end in [0, 1, 2, source.len(), source.len() + 1, usize::MAX] {
                let span = Span { start, end };
                let expected = source.get(start..end).unwrap_or_default();
                assert_eq!(span.text(source).as_bytes(), expected.as_bytes());
            }
        }
    }
    let document = Document {
        headings: Vec::new(),
        references: Vec::new(),
        code_blocks: Vec::new(),
        front_matter: None,
        lines: 1,
    };
    assert_eq!(document.stable_name(0), None);
    let heading = Heading {
        level: 1,
        style: HeadingStyle::Atx,
        title: String::new(),
        name: String::new(),
        ordinal: None,
        parent: None,
        span: Span {
            start: 0,
            end: usize::MAX,
        },
        section: Span { start: 0, end: 0 },
        lines: [1, 1],
        summary: String::new(),
    };
    assert_eq!(heading.body().text(""), "");
}

#[test]
fn heading_and_inline_delimiter_floods_are_repeatable() {
    for source in [
        format!("# Root\n{}", "###### Child\n[x](a) `b`\n".repeat(2000)),
        format!("# A\n{}", "[a](b \"".repeat(2000)),
        format!("# A\n{}", "[".repeat(10_000)),
    ] {
        let document = extract(source.as_bytes()).unwrap();
        check(&source, &document);
        assert_eq!(
            document,
            extract_with(source.as_bytes(), &Limits::default()).unwrap()
        );
    }
}

#[test]
fn every_refusal_names_a_next_step() {
    for (error, next) in [
        (LexError::TooLarge { len: 2, max: 1 }, "split or reduce"),
        (
            LexError::TooDeep { line: 1, max: 0 },
            "reduce container nesting",
        ),
        (LexError::InvalidUtf8 { valid_up_to: 0 }, "re-encode"),
    ] {
        assert!(error.to_string().contains(next), "{error}");
    }
}
