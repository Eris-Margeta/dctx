// SPDX-License-Identifier: Apache-2.0
//! Hostile byte input exercises the public extractor and its resource bounds.
use dctx_lex_cs::{Completion, CsLexer, DiagnosticReason, LexError, LexedFile, Limits};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static EXTRACTION: Mutex<()> = Mutex::new(());

const PATH: &str = "hostile.cs";
const FIVE_MB: usize = 5 * 1024 * 1024;
const SAMPLE: &str = "using System;\nusing System.Collections.Generic;\nnamespace Sample;\n/// <summary>Cache stores values.</summary>\npublic class Cache<T> {\n    private List<T> values;\n    public T Get(int index) {\n        var label = $\"value {index}\";\n        return values[index];\n    }\n    public void café() { Console.WriteLine(\"你好\"); }\n}\n";

// Time each extraction separately, including every truncation and mutation.
// Panics fail the calling test; the outer test command also has a watchdog.
fn extract(bytes: &[u8]) -> Result<LexedFile, LexError> {
    // Keep parallel test workers from competing for the per-input CPU budget.
    let _guard = EXTRACTION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let start = Instant::now();
    let result = CsLexer::default().extract(PATH, bytes);
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{} bytes took {:?}",
        bytes.len(),
        start.elapsed()
    );
    if let Ok(file) = &result {
        assert!(!file.nodes.is_empty());
        assert!(file.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
        assert!(
            file.diagnostics
                .windows(2)
                .all(|pair| pair[0].byte <= pair[1].byte)
        );
        for diagnostic in &file.diagnostics {
            assert!(diagnostic.byte as usize <= bytes.len());
            assert!(diagnostic.line >= 1);
            if let Ok(source) = std::str::from_utf8(bytes) {
                assert!(source.is_char_boundary(diagnostic.byte as usize));
            }
        }
        for node in &file.nodes {
            assert!(node.fact.lines[0] >= 1);
            assert!(node.fact.lines[1] >= node.fact.lines[0]);
        }
    }
    result
}

fn accepted(source: &str) -> LexedFile {
    extract(source.as_bytes()).expect("malformed source must remain bounded and recoverable")
}

fn sample() -> Vec<u8> {
    // Keep a complete realistic program and pad its final comment to exactly
    // 400 bytes. Cutting at a multibyte identifier also tests invalid UTF-8.
    let mut bytes = SAMPLE.as_bytes().to_vec();
    assert!(bytes.len() <= 397);
    bytes.extend_from_slice(b"// ");
    bytes.resize(400, b'x');
    bytes
}

#[test]
fn empty_file() {
    let file = accepted("");
    assert_eq!(file.completion, Completion::Complete);
    assert_eq!(file.nodes.len(), 1);
    assert!(file.diagnostics.is_empty());
    assert!(file.references.is_empty());
}

#[test]
fn only_comments() {
    for source in [
        "// comment",
        "/* comment */",
        "// 你好\r\n/* café */\n",
        "/** documentation */",
    ] {
        let file = accepted(source);
        assert_eq!(file.completion, Completion::Complete);
        assert_eq!(file.nodes.len(), 1);
        assert!(file.references.is_empty());
        assert!(file.diagnostics.is_empty());
    }
}

#[test]
fn unterminated_literals_and_block_comments() {
    for source in [
        "\"",
        "\"abc\\",
        "@\"",
        "@\"raw\nvalue",
        "$\"{",
        "$@\"{",
        "@$\"{",
        "\"\"\"",
        "\"\"\"\ntext",
        "$$\"\"\"{{",
        "'",
        "'\\",
        "/*",
        "/* *",
    ] {
        let file = accepted(source);
        assert_eq!(file.completion, Completion::Partial, "{source:?}");
        assert!(
            file.diagnostics.iter().any(|d| matches!(
                d.reason,
                DiagnosticReason::UnterminatedComment
                    | DiagnosticReason::UnterminatedString
                    | DiagnosticReason::UnterminatedChar
            )),
            "{source:?}: {:?}",
            file.diagnostics
        );
    }
}

#[test]
fn ten_thousand_nested_brackets() {
    for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
        for balanced in [false, true] {
            let mut source = open.to_string().repeat(10_000);
            if balanced {
                source.push_str(&close.to_string().repeat(10_000));
            }
            assert!(matches!(
                extract(source.as_bytes()),
                Err(LexError::TooDeep { limit: 256, .. })
            ));
        }
    }
    assert!(
        !accepted("([]){}")
            .diagnostics
            .iter()
            .any(|d| d.reason == DiagnosticReason::UnbalancedBracket)
    );
}

#[test]
fn ten_thousand_nested_generics() {
    let source = format!(
        "{}{}T{}{}",
        "class Cache { Box<",
        "Box<".repeat(10_000),
        ">".repeat(10_000),
        "> value; }"
    );
    let result = extract(source.as_bytes());
    assert!(
        result.is_ok() || matches!(result, Err(LexError::TooDeep { .. })),
        "{result:?}"
    );
}

#[test]
fn five_mb_single_line_comment() {
    let source = format!("//{}", "x".repeat(FIVE_MB - 2));
    let file = accepted(&source);
    assert!(file.diagnostics.is_empty());
    assert_eq!(file.nodes.len(), 1);
}

#[test]
fn five_mb_single_line_literal() {
    let source = format!("\"{}\"", "x".repeat(FIVE_MB - 2));
    let file = accepted(&source);
    assert!(
        !file
            .diagnostics
            .iter()
            .any(|d| d.reason == DiagnosticReason::UnterminatedString)
    );
}

#[test]
fn nul_bytes_do_not_end_the_input() {
    for source in [
        "\0",
        "\0\0\0",
        "// \0\n",
        "/* \0 */",
        "\"\0\"",
        "class Before {}\0class After {}",
    ] {
        let _ = accepted(source);
    }
}

#[test]
fn crlf_and_lone_cr() {
    for newline in ["\r\n", "\r"] {
        let source = SAMPLE.replace('\n', newline);
        let _ = accepted(&source);
        let file = accepted(&format!("// comment{newline}/* block */{newline}"));
        assert!(file.diagnostics.is_empty());
    }
}

#[test]
fn bom_before_source_and_at_eof() {
    for source in [
        "\u{feff}".to_string(),
        format!("\u{feff}{SAMPLE}"),
        format!("{SAMPLE}\u{feff}"),
    ] {
        let _ = accepted(&source);
    }
}

#[test]
fn invalid_utf8_is_a_typed_refusal() {
    for bytes in [
        b"\xff".as_slice(),
        b"\xc0\xaf",
        b"\xed\xa0\x80",
        b"\xf4\x90\x80\x80",
        b"\xe2\x82",
        b"//\xff",
        b"\"\xff\"",
        b"\xef\xbb",
    ] {
        assert!(matches!(extract(bytes), Err(LexError::InvalidUtf8(_))));
    }
}

#[test]
fn unicode_identifiers_keep_their_names() {
    let file = accepted("class café {} class 变量 {}");
    assert!(
        file.nodes
            .iter()
            .any(|node| node.fact.name.ends_with("café")),
        "{:?}",
        file.nodes
    );
    assert!(
        file.nodes
            .iter()
            .any(|node| node.fact.name.ends_with("变量")),
        "{:?}",
        file.nodes
    );
}

#[test]
fn every_byte_offset_truncation() {
    let bytes = sample();
    assert_eq!(bytes.len(), 400);
    assert_eq!(extract(&bytes).unwrap().completion, Completion::Complete);
    for offset in 0..=bytes.len() {
        let prefix = &bytes[..offset];
        let result = extract(prefix);
        if std::str::from_utf8(prefix).is_err() {
            assert!(
                matches!(result, Err(LexError::InvalidUtf8(_))),
                "offset {offset}"
            );
        } else {
            assert!(result.is_ok(), "offset {offset}: {result:?}");
        }
    }
}

#[test]
fn deterministic_byte_mutations() {
    let bytes = sample();
    for offset in 0..bytes.len() {
        for byte in [0, b'"', b'\'', b'\\', b'/', b'{', b'}', 0xff] {
            let mut mutated = bytes.clone();
            mutated[offset] = byte;
            let result = extract(&mutated);
            if std::str::from_utf8(&mutated).is_err() {
                assert!(matches!(result, Err(LexError::InvalidUtf8(_))));
            } else {
                assert!(result.is_ok(), "offset {offset}, byte {byte}: {result:?}");
            }
        }
    }
}

#[test]
fn byte_limit_has_an_inclusive_boundary() {
    let lexer = CsLexer::new(Limits {
        max_bytes: 4,
        ..Limits::default()
    });
    assert!(lexer.extract(PATH, b"/**/").is_ok());
    assert!(matches!(
        lexer.extract(PATH, b"/**/ "),
        Err(LexError::TooLarge { bytes: 5, limit: 4 })
    ));
}

#[test]
fn long_dollar_prefix_without_a_string_is_linear() {
    // A failed string-prefix probe must consume the run instead of rescanning
    // every suffix of it. Keep a declaration after it to check recovery.
    for suffix in ["", "; class After {}", "@name; class After {}"] {
        let source = format!("{}{suffix}", "$".repeat(64 * 1024));
        let file = accepted(&source);
        if !suffix.is_empty() {
            assert!(file.nodes.iter().any(|node| node.fact.name == "After"));
        }
    }
}

#[test]
fn short_quote_runs_inside_a_raw_string_are_linear() {
    // Both runs belong to one unterminated literal. Probing for its closer
    // must not count the same short quote run once per byte.
    let source = format!(
        "{}x{}",
        "\"".repeat(FIVE_MB / 2),
        "\"".repeat(FIVE_MB / 2 - 1)
    );
    let file = accepted(&source);
    assert!(
        file.diagnostics
            .iter()
            .any(|d| d.reason == DiagnosticReason::UnterminatedString)
    );
}

#[test]
fn fingerprint_whitespace_semantics_are_preserved() {
    let expected = dctx_lex_cs::body_fingerprint("a b");
    for source in [
        "a b",
        " a\t\r\n b ",
        "a\x0bb",
        "a\x0cb",
        "a\u{0085}b",
        "a\u{2003}b",
    ] {
        assert_eq!(dctx_lex_cs::body_fingerprint(source), expected);
    }
    assert_eq!(
        dctx_lex_cs::body_fingerprint("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn partial_raw_delimiter_keeps_the_following_interpolation() {
    let file = accepted(r#####"class A { string s = $$"""" text """{{Call()}}""""; }"#####);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    assert!(
        file.references
            .iter()
            .any(|reference| reference.fact.name == "Call")
    );
}
