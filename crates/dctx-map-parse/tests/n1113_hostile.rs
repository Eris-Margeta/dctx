// SPDX-License-Identifier: Apache-2.0
//! Hostile Parser, Language and fingerprint inputs. Every byte offset is cut,
//! including the middle of UTF-8 scalars. No random input, IO or clock is used.
//! Parser maxima are exercised with whitespace so token/fact limits do not
//! obscure the byte boundary; fingerprints/dispatch have no size ceiling.
#![forbid(unsafe_code)]

use dctx_map_model::{Coverage, EdgeKind, NodeKind};
use dctx_map_parse::{
    Language, ParseError, ParsedFile, Parser, PureParser, TreeSitterParser, body_fingerprint,
};
use std::collections::BTreeSet;

const SAMPLES: &[(&str, &[u8])] = &[
    ("sample.rs", "fn café() { call(); }\n".as_bytes()),
    ("sample.js", "function café() { call(); }\n".as_bytes()),
    ("sample.ts", b"function run(): void { call(); }\n"),
    ("sample.tsx", b"function View() { return <b>hi</b>; }\n"),
    ("sample.md", "# Café\n`call` [link](target)\n".as_bytes()),
    ("sample.py", "def run():\n    return 'é'\n".as_bytes()),
    ("sample.go", b"package demo\nfunc Run() { call() }\n"),
    ("Sample.java", b"class Sample { void run() { call(); } }\n"),
    ("sample.c", b"void run() { call(); }\n"),
    ("sample.cpp", b"class Sample { void run() { call(); } };\n"),
    ("sample.json", "{\"name\": \"é\", \"ok\": true}".as_bytes()),
    ("sample.jsonc", b"{ /* comment */ \"name\": \"x\" }"),
    ("sample.toml", b"[demo]\nname = \"x\"\n"),
    ("sample.yaml", b"demo:\n  name: x\n"),
    ("Sample.cs", b"class Sample { void Run() { Call(); } }\n"),
    ("sample.kt", b"fun run() { call() }\n"),
    ("sample.rb", b"def run\n  call\nend\n"),
    ("sample.php", b"<?php function run() { call(); }\n"),
    ("sample.swift", b"func run() { call() }\n"),
    ("sample.xyz", "// Café.\nopaque\n".as_bytes()),
];

fn parsers() -> [(&'static str, Box<dyn Parser + Sync>); 2] {
    [
        ("pure", Box::new(PureParser)),
        ("oracle", Box::new(TreeSitterParser)),
    ]
}

fn snapshot(parser: &dyn Parser, path: &str, bytes: &[u8]) -> Vec<u8> {
    let result = parser.parse(path, bytes);
    if let Ok(facts) = &result {
        check_facts(facts, path, bytes);
    }
    // ParsedFile has no wire serializer: its ordered public Debug projection
    // includes every field and lets this suite compare exact output bytes.
    format!("{result:?}").into_bytes()
}

fn check_facts(facts: &ParsedFile, path: &str, bytes: &[u8]) {
    assert_eq!(facts.path, path);
    assert_eq!(facts.language, Language::for_path(path));
    assert!(!facts.nodes.is_empty());
    assert_eq!(facts.nodes[0].kind, NodeKind::Module);
    let max_line = u32::try_from(bytes.split(|byte| *byte == b'\n').count()).unwrap();
    let ids: BTreeSet<_> = facts.nodes.iter().map(|node| &node.id).collect();
    assert_eq!(ids.len(), facts.nodes.len());
    for node in &facts.nodes {
        assert_eq!(node.path, path);
        assert!(
            node.lines[0] >= 1 && node.lines[0] <= node.lines[1] && node.lines[1] <= max_line,
            "{path}: prefix={:?}, span={:?}, max={max_line}, name={}",
            &bytes[..bytes.len().min(32)],
            node.lines,
            node.name
        );
        assert_eq!(node.fingerprint.len(), 64);
    }
    for edge in &facts.edges {
        assert!(ids.contains(&edge.src) && ids.contains(&edge.dst));
        assert_eq!(edge.owner_path, path);
        assert_eq!(edge.kind, EdgeKind::Contains);
        assert!(edge.kind.permits(edge.conf));
    }
    for reference in &facts.references {
        assert!(ids.contains(&reference.src));
        assert_eq!(reference.owner_path, path);
        assert!(
            reference.lines[0] >= 1
                && reference.lines[0] <= reference.lines[1]
                && reference.lines[1] <= max_line
        );
    }
}

#[test]
fn every_adapter_handles_empty_and_all_one_byte_inputs_repeatably() {
    for (name, parser) in parsers() {
        for (path, _) in SAMPLES {
            assert_eq!(
                snapshot(parser.as_ref(), path, b""),
                snapshot(parser.as_ref(), path, b"")
            );
            for byte in 0..=u8::MAX {
                let first = snapshot(parser.as_ref(), path, &[byte]);
                assert_eq!(
                    first,
                    snapshot(parser.as_ref(), path, &[byte]),
                    "{name} {path}: {byte}"
                );
                if byte >= 128 {
                    assert!(matches!(
                        parser.parse(path, &[byte]),
                        Err(ParseError::InvalidUtf8(_))
                    ));
                }
            }
        }
    }
}

#[test]
fn every_byte_truncation_of_each_small_sample_never_panics() {
    for (name, parser) in parsers() {
        for (path, sample) in SAMPLES {
            assert!(parser.parse(path, sample).is_ok(), "{name} {path}");
            for cut in 0..=sample.len() {
                let bytes = &sample[..cut];
                assert_eq!(
                    snapshot(parser.as_ref(), path, bytes),
                    snapshot(parser.as_ref(), path, bytes),
                    "{name} {path} cut {cut}"
                );
                if std::str::from_utf8(bytes).is_err() {
                    assert!(matches!(
                        parser.parse(path, bytes),
                        Err(ParseError::InvalidUtf8(_))
                    ));
                }
            }
        }
    }
}

#[test]
fn invalid_utf8_is_refused_at_every_insertion_point() {
    for (_, parser) in parsers() {
        for (path, sample) in SAMPLES {
            for invalid in [
                b"\xff".as_slice(),
                b"\xc3\x28",
                b"\xed\xa0\x80",
                b"\xf4\x90\x80\x80",
            ] {
                for at in 0..=sample.len() {
                    let mut bytes = sample.to_vec();
                    bytes.splice(at..at, invalid.iter().copied());
                    assert!(
                        matches!(parser.parse(path, &bytes), Err(ParseError::InvalidUtf8(_))),
                        "{path} offset {at}"
                    );
                }
            }
        }
    }
}

#[test]
fn adapter_byte_maxima_and_one_over_are_checked_before_extraction() {
    for (name, parser) in parsers() {
        for (path, _) in SAMPLES {
            let max = if Language::for_path(path) == Language::Markdown {
                32 << 20
            } else {
                8 << 20
            };
            // A newline then spaces keeps Markdown's line count and allocation
            // small and is valid input to each whitespace-tolerant adapter.
            let mut bytes = vec![b' '; max];
            bytes[0] = b'\n';
            let result = parser.parse(path, &bytes);
            // Empty JSON is a syntax refusal, not a byte-size refusal.
            assert!(
                matches!(result, Ok(_) | Err(ParseError::Syntax { .. })),
                "{name} {path}: {result:?}"
            );
            bytes.push(b' ');
            assert!(
                matches!(parser.parse(path, &bytes), Err(ParseError::TooLarge)),
                "{name} {path} must refuse max + 1"
            );
        }
    }
}

#[test]
fn duplicate_items_and_reordering_keep_ids_and_reference_ownership_valid() {
    for (_, parser) in parsers() {
        for (path, source) in [
            ("sample.rs", "fn a() { first(); }\nfn a() { second(); }\n"),
            (
                "sample.js",
                "function a() { first(); }\nfunction a() { second(); }\n",
            ),
            ("sample.md", "# Same\n`first`\n# Same\n`second`\n"),
        ] {
            let facts = parser.parse(path, source.as_bytes()).unwrap();
            check_facts(&facts, path, source.as_bytes());
            let duplicates: Vec<_> = facts.nodes.iter().skip(1).collect();
            assert_eq!(duplicates.len(), 2);
            assert!(duplicates[0].id.as_str().ends_with("~1"));
            assert!(duplicates[1].id.as_str().ends_with("~2"));
            assert!(facts.references.iter().any(|r| r.src == duplicates[0].id));
            assert!(facts.references.iter().any(|r| r.src == duplicates[1].id));
            assert_eq!(
                snapshot(parser.as_ref(), path, source.as_bytes()),
                snapshot(parser.as_ref(), path, source.as_bytes())
            );
        }
        for (path, a, b) in [
            ("sample.rs", "fn a() { one(); }\n", "fn b() { two(); }\n"),
            (
                "sample.js",
                "function a() { one(); }\n",
                "function b() { two(); }\n",
            ),
            ("sample.md", "# Alpha\ntext\n", "# Beta\ntext\n"),
        ] {
            let forward = parser.parse(path, format!("{a}{b}").as_bytes()).unwrap();
            let reverse = parser.parse(path, format!("{b}{a}").as_bytes()).unwrap();
            let ids = |file: &ParsedFile| {
                file.nodes
                    .iter()
                    .map(|n| n.id.clone())
                    .collect::<BTreeSet<_>>()
            };
            assert_eq!(ids(&forward), ids(&reverse));
        }
    }
}

#[test]
fn invalid_paths_and_truncated_paths_never_escape_identity_validation() {
    for (_, parser) in parsers() {
        for path in [
            "",
            "/escape.rs",
            "../escape.rs",
            "a/../b.rs",
            "a//b.rs",
            "./a.rs",
            "a\\b.rs",
            "a\0.rs",
            "C:escape.rs",
            "a.rs#name",
            "a~1.rs",
        ] {
            assert!(
                matches!(parser.parse(path, b""), Err(ParseError::InvalidId(_))),
                "{path:?}"
            );
        }
        let path = "src/é.rs";
        for cut in 0..=path.len() {
            if let Some(prefix) = path.get(..cut) {
                assert_eq!(
                    snapshot(parser.as_ref(), prefix, b""),
                    snapshot(parser.as_ref(), prefix, b"")
                );
            }
        }
        for length in [120, 121, 4096, 4097] {
            let path = format!("{}.xyz", "a".repeat(length));
            let facts = parser.parse(&path, b"x").unwrap();
            check_facts(&facts, &path, b"x");
            assert!(facts.nodes[0].id.alias().is_some());
        }
    }
}

#[test]
fn opaque_fallback_retains_recognized_language_and_reports_opaque_precision() {
    for (_, parser) in parsers() {
        for extension in [
            "scala", "dart", "zig", "sol", "gleam", "f90", "pl", "groovy", "ps1", "sv", "txt",
            "ex", "lua", "ml", "R", "jl", "hs", "tf", "sql", "sh", "mk", "proto", "pas", "graphql",
            "vue", "clj", "nix", "erl", "xyz",
        ] {
            let path = format!("sample.{extension}");
            let facts = parser.parse(&path, b"x").unwrap();
            check_facts(&facts, &path, b"x");
            assert_eq!(facts.nodes.len(), 1);
            assert_eq!(facts.precision.coverage(), Coverage::Opaque);
            assert!(facts.edges.is_empty() && facts.references.is_empty());
        }
    }
}

#[test]
fn language_dispatch_all_byte_prefixes_and_large_paths_are_deterministic() {
    for sample in [
        "",
        "x",
        "dir/é.RS",
        "dir\\Rakefile",
        "x.sql.in",
        "Dockerfile",
        "Gemfile.lock",
    ] {
        for cut in 0..=sample.len() {
            let Some(path) = sample.get(..cut) else {
                continue;
            };
            assert_eq!(Language::for_path(path), Language::for_path(path));
        }
    }
    for length in [4096, 4097] {
        assert_eq!(
            Language::for_path(&format!("{}.RS", "é".repeat(length))),
            Language::Rust
        );
    }
    assert_eq!(Language::for_path(""), Language::Unknown);
    assert_eq!(Language::for_path("x"), Language::Unknown);
    assert_eq!(Language::for_path("x.rs\0"), Language::Unknown);
    assert_eq!(Language::for_path("dir.dot/file"), Language::Unknown);
    assert_eq!(Language::for_path("dir\\FILE.RS"), Language::Rust);
}

#[test]
fn fingerprints_cover_empty_one_byte_all_prefixes_and_reordering() {
    let sample = " a\né 🦀\t z ";
    for cut in 0..=sample.len() {
        let Some(body) = sample.get(..cut) else {
            continue;
        };
        let output = body_fingerprint(body);
        assert_eq!(output.len(), 64);
        assert!(
            output
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        assert_eq!(output.as_bytes(), body_fingerprint(body).as_bytes());
    }
    for byte in 0..=127u8 {
        let input = [byte];
        let text = std::str::from_utf8(&input).unwrap();
        assert_eq!(body_fingerprint(text), body_fingerprint(text));
    }
    for length in [8 << 20, (8 << 20) + 1] {
        // Fingerprints are independent of parser source-size limits.
        assert_eq!(body_fingerprint(&" ".repeat(length)), body_fingerprint(""));
    }
    assert_eq!(body_fingerprint("a\t b\nc"), body_fingerprint(" a b c "));
    assert_ne!(body_fingerprint("a b a"), body_fingerprint("a a b"));
}

#[test]
fn shared_parsers_can_be_used_concurrently_with_identical_output() {
    for (_, parser) in parsers() {
        std::thread::scope(|scope| {
            let parser = parser.as_ref();
            let workers: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(move || {
                        SAMPLES
                            .iter()
                            .map(|(path, bytes)| snapshot(parser, path, bytes))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            let expected: Vec<_> = SAMPLES
                .iter()
                .map(|(path, bytes)| snapshot(parser, path, bytes))
                .collect();
            for worker in workers {
                assert_eq!(worker.join().unwrap(), expected);
            }
        });
    }
}

#[test]
fn refusal_messages_name_the_corrective_action_and_preserve_sources() {
    for (_, parser) in parsers() {
        for (path, bytes, action) in [
            ("../escape.rs", b"".as_slice(), "use"),
            ("sample.rs", b"\xff".as_slice(), "decode"),
            ("sample.rs", b"fn broken(".as_slice(), "correct"),
        ] {
            let error = parser.parse(path, bytes).unwrap_err();
            assert!(error.to_string().contains(action), "{error}");
            if matches!(error, ParseError::InvalidUtf8(_)) {
                assert!(std::error::Error::source(&error).is_some());
            }
        }
    }
    for (error, action) in [
        (ParseError::TooLarge, "split"),
        (ParseError::NoTree, "retry"),
        (ParseError::Grammar("incompatible grammar".into()), "use"),
        (ParseError::Label(dctx_map_model::InvalidLabel), "supply"),
    ] {
        let first = error.to_string();
        assert!(first.contains(action), "{first}");
        assert_eq!(first.as_bytes(), error.to_string().as_bytes());
    }
}

#[test]
fn bare_cr_coordinates_match_lf_model_for_kotlin_and_php() {
    for (_, parser) in parsers() {
        for (path, source) in [
            (
                "sample.kt",
                "\rfun run() {\r call()\r}\r\nfun other() {\r call()\r}\r",
            ),
            (
                "sample.php",
                "<?php\rfunction run() {\r call();\r}\r\nfunction other() {\r call();\r}\r",
            ),
        ] {
            let facts = parser.parse(path, source.as_bytes()).unwrap();
            check_facts(&facts, path, source.as_bytes());
            let run = facts.nodes.iter().find(|node| node.name == "run").unwrap();
            let other = facts
                .nodes
                .iter()
                .find(|node| node.name == "other")
                .unwrap();
            assert_eq!(run.lines, [1, 1]);
            assert_eq!(other.lines, [2, 2]);
            assert!(facts.references.iter().any(|r| r.lines == [1, 1]));
            assert!(facts.references.iter().any(|r| r.lines == [2, 2]));
            for invalid in ["\r\r\"", "\r\n\r\""] {
                let input = if path.ends_with("php") {
                    format!("<?php{invalid}")
                } else {
                    invalid.into()
                };
                if let Err(ParseError::Syntax { line, .. }) = parser.parse(path, input.as_bytes()) {
                    assert_eq!(line, if invalid.contains('\n') { 2 } else { 1 });
                }
            }
        }
    }
}

#[test]
fn duplicated_and_reordered_sample_lines_never_panic_in_any_adapter() {
    for (_, parser) in parsers() {
        for (path, sample) in SAMPLES {
            let duplicate: Vec<_> = sample.iter().chain(sample.iter()).copied().collect();
            assert_eq!(
                snapshot(parser.as_ref(), path, &duplicate),
                snapshot(parser.as_ref(), path, &duplicate)
            );
            let text = std::str::from_utf8(sample).unwrap();
            let reversed = text.lines().rev().collect::<Vec<_>>().join("\n");
            assert_eq!(
                snapshot(parser.as_ref(), path, reversed.as_bytes()),
                snapshot(parser.as_ref(), path, reversed.as_bytes())
            );
        }
    }
}

#[test]
fn literal_test_names_cannot_collide_with_generated_duplicate_ordinals() {
    let source = b"it('same', () => {}); it('same', () => {}); it('same~1', () => {});";
    for (_, parser) in parsers() {
        assert!(parser.parse("sample.js", source).is_err());
    }
}
