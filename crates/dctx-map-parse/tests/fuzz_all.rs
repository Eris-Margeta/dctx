// SPDX-License-Identifier: Apache-2.0
//! Cross-language robustness gate for the pure Parser port.
//! Exercises SPEC-MAP-MODE sections 5 and 6.2: refusals, determinism and store rules.
//! Uses local fixtures and seeded, bounded proptest inputs without executing source.
//! Each language/property is separate so a finding can be isolated precisely.
#![forbid(unsafe_code)]

use dctx_map_model::{EdgeKind, NodeKind};
use dctx_map_parse::{Language, ParseError, ParsedFile, Parser, PureParser};
use proptest::prelude::*;
use proptest::test_runner::RngSeed;
use std::collections::BTreeSet;

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 48,
        rng_seed: RngSeed::Fixed(0x4458_4655_5a5a),
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn inputs(fixture: &'static str) -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..2048),
        proptest::collection::vec(any::<char>(), 0..256)
            .prop_map(|chars| chars.into_iter().collect::<String>().into_bytes()),
        "[a-zA-Z0-9_ \n\r\t{}()\\[\\]<>:;#/'\"=é]{0,512}".prop_map(String::into_bytes),
        Just(fixture.as_bytes().to_vec()),
        (0usize..32).prop_map(move |lines| format!("{}{fixture}", "\n".repeat(lines)).into_bytes()),
    ]
}

/// Copied from lexers.rs: store validation plus spans inside the source file.
#[allow(
    clippy::naive_bytecount,
    reason = "Standard iterator newline counts keep fixture and bounded 8 MiB corpus checks dependency-free; this is test validation rather than a parser hot path."
)]
fn assert_store_accepts(facts: &ParsedFile, source: &[u8]) {
    // Line terminators: LF, CRLF (one) and a bare CR (the PHP lexer counts it, D1113-LEXHOSTILE).
    let breaks = source
        .iter()
        .enumerate()
        .filter(|&(i, &b)| b == b'\n' || (b == b'\r' && source.get(i + 1) != Some(&b'\n')))
        .count();
    let file_end = u32::try_from(breaks).unwrap() + 1;
    let mut nodes = facts.nodes.clone();
    let again = facts
        .nodes
        .iter()
        .filter(|node| matches!(node.kind, NodeKind::Place | NodeKind::Package))
        .cloned()
        .collect::<Vec<_>>();
    nodes.extend(again);
    let mut ids = BTreeSet::new();
    assert_eq!(nodes[0].kind, NodeKind::Module, "{}", facts.path);
    for node in &nodes {
        let identity =
            node.id
                .as_str()
                .split_once(':')
                .map(|(_, rest)| match rest.split_once('#') {
                    Some((path, _)) => path,
                    None => rest,
                });
        assert_eq!(node.path, facts.path, "{}", node.name);
        assert_eq!(identity, Some(facts.path.as_str()), "{}", node.id);
        assert!(
            node.id.as_str().starts_with(node.kind.letter()),
            "{}",
            node.id
        );
        assert!(
            node.lines[0] >= 1 && node.lines[1] >= node.lines[0],
            "{}",
            node.name
        );
        assert!(
            node.lines[1] <= file_end,
            "{} spans past the file",
            node.name
        );
        assert!(
            ids.insert(node.id.as_str().to_owned()),
            "duplicate id {}",
            node.id
        );
    }
    let mut owned = BTreeSet::from([facts.nodes[0].id.as_str()]);
    loop {
        let before = owned.len();
        for edge in &facts.edges {
            assert_eq!(edge.owner_path, facts.path);
            assert!(edge.kind.permits(edge.conf));
            assert!(ids.contains(edge.src.as_str()), "{}", edge.src);
            assert!(ids.contains(edge.dst.as_str()), "{}", edge.dst);
            if edge.kind == EdgeKind::Contains && owned.contains(edge.src.as_str()) {
                owned.insert(edge.dst.as_str());
            }
        }
        if owned.len() == before {
            break;
        }
    }
    for node in &facts.nodes {
        assert!(
            owned.contains(node.id.as_str()),
            "{} is not owned by the file node",
            node.name
        );
    }
}

fn checked(path: &str, bytes: &[u8]) -> Result<ParsedFile, ParseError> {
    let result = PureParser.parse(path, bytes);
    if let Ok(facts) = &result {
        assert_eq!(facts.path, path);
        assert_eq!(facts.language, Language::for_path(path));
        assert_store_accepts(facts, bytes);
    }
    result
}

fn assert_repeat(path: &str, bytes: &[u8]) {
    let first = checked(path, bytes);
    let second = checked(path, bytes);
    assert_eq!(
        first.as_ref().map_err(|error| format!("{error:?}")),
        second.as_ref().map_err(|error| format!("{error:?}")),
        "{path}: {bytes:?}"
    );
}

macro_rules! languages {
    ($( $name:ident, $language:ident, $path:literal, $fixture:expr; )*) => {
        $(mod $name {
            use super::*;
            const PATH: &str = $path;
            const FIXTURE: &str = $fixture;

            proptest! {
                #![proptest_config(config())]

                #[test]
                fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
                    let _: Result<ParsedFile, ParseError> = checked(PATH, &bytes);
                }

                #[test]
                fn deterministic(bytes in inputs(FIXTURE)) {
                    assert_repeat(PATH, &bytes);
                }

                #[test]
                fn successful_results_obey_store_invariants(bytes in inputs(FIXTURE)) {
                    let _ = checked(PATH, &bytes);
                }
            }

            #[test]
            fn fixture_prefixes_never_panic() {
                assert_eq!(Language::for_path(PATH), Language::$language);
                assert!(checked(PATH, FIXTURE.as_bytes()).is_ok(), "invalid fixture: {PATH}");
                assert_repeat(PATH, FIXTURE.as_bytes());
                let boundaries: Vec<_> = FIXTURE.char_indices().map(|(index, _)| index)
                    .chain(std::iter::once(FIXTURE.len())).collect();
                let cuts = boundaries.len().min(200);
                for index in 0..cuts {
                    let cut = boundaries[index * (boundaries.len() - 1) / (cuts - 1).max(1)];
                    let _ = checked(PATH, &FIXTURE.as_bytes()[..cut]);
                }
            }

            #[test]
            fn nul_and_invalid_utf8_never_panic() {
                for bytes in [b"\0".as_slice(), b"\xff", b"\xc3\x28", b"\xed\xa0\x80"] {
                    let _ = checked(PATH, bytes);
                }
                for suffix in [b"\0".as_slice(), b"\xff"] {
                    let mut bytes = FIXTURE.as_bytes().to_vec();
                    bytes.splice(bytes.len() / 2..bytes.len() / 2, suffix.iter().copied());
                    let _ = checked(PATH, &bytes);
                }
            }

            #[test]
            fn large_and_deep_inputs_never_panic() {
                std::thread::Builder::new().stack_size(16 * 1024 * 1024)
                    .spawn(|| {
                        for bytes in [vec![b'a'; 2 * 1024 * 1024], vec![b'['; 20_000]] {
                            let result = checked(PATH, &bytes);
                            assert!(matches!(result, Ok(_) | Err(ParseError::TooLarge | ParseError::Syntax { .. })),
                                "{PATH}: unexpected refusal: {result:?}");
                        }
                    }).unwrap().join().unwrap();
            }
        })*
    };
}

languages! {
    rust, Rust, "sample.rs", include_str!("fixtures/rust/src/lib.rs");
    javascript, JavaScript, "sample.js", include_str!("fixtures/js/module.mjs");
    jsx, JavaScript, "sample.jsx", "export function View() { return <div>Hello</div>; }\n";
    typescript, TypeScript, "sample.ts", include_str!("fixtures/ts/module.ts");
    tsx, Tsx, "sample.tsx", "export function View() { return <div>Hello</div>; }\n";
    markdown, Markdown, "sample.md", include_str!("fixtures/headings.md");
    python, Python, "sample.py", include_str!("fixtures/sample.py");
    go, Go, "sample.go", include_str!("fixtures/sample.go");
    java, Java, "Sample.java", include_str!("fixtures/Sample.java");
    c, C, "sample.c", include_str!("fixtures/sample.c");
    cpp, C, "sample.cpp", "namespace demo { class Box { public: int run() { return 1; } }; }\n";
    json, Config, "sample.json", "{\"package\": {\"name\": \"demo\", \"enabled\": true}}\n";
    jsonc, Config, "sample.jsonc", "{\n// package settings\n\"name\": \"demo\"\n}\n";
    toml, Config, "sample.toml", include_str!("fixtures/sample.toml");
    yaml, Config, "sample.yaml", include_str!("fixtures/sample.yaml");
    csharp, CSharp, "Sample.cs", include_str!("fixtures/Sample.cs");
    kotlin, Kotlin, "sample.kt", include_str!("fixtures/sample.kt");
    ruby, Ruby, "sample.rb", include_str!("fixtures/sample.rb");
    php, Php, "sample.php", include_str!("fixtures/sample.php");
    swift, Swift, "sample.swift", include_str!("fixtures/sample.swift");
}
