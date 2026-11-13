// SPDX-License-Identifier: Apache-2.0
//! Hostile byte inputs exercised through the public extraction API.
#![forbid(unsafe_code)]

use dctx_lex_rb::{Completion, LexError, LexedFile, Limits, RubyLexer};
use std::{
    sync::{Mutex, mpsc},
    thread,
    time::Duration,
};

static EXTRACTION_LOCK: Mutex<()> = Mutex::new(());

const PATH: &str = "hostile.rb";
const HEAD: &str = "";
const DECLARATION: &str = "def real\n  1\nend\n";

// There is no public step counter. A receive deadline also catches a stalled
// scanner; an elapsed-time check after a synchronous call could never do that.
fn bounded_with(bytes: &[u8], limits: Limits) -> Result<LexedFile, LexError> {
    let _guard = EXTRACTION_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let bytes = bytes.to_vec();
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let result = RubyLexer::new(limits).extract(PATH, &bytes);
        sender.send(result).expect("test receiver alive");
    });
    let result = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("extraction must finish without panicking within two seconds");
    worker.join().expect("extractor worker must not panic");
    result
}

fn bounded(bytes: &[u8]) -> Result<LexedFile, LexError> {
    let result = bounded_with(bytes, Limits::default());
    if let Ok(facts) = &result {
        assert!(!facts.nodes.is_empty());
        assert!(facts.nodes.len() + facts.references.len() <= Limits::CEILING.max_facts);
        assert!(facts.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
        let source = std::str::from_utf8(bytes).expect("accepted input is UTF-8");
        for diagnostic in &facts.diagnostics {
            assert!(diagnostic.line > 0);
            assert!(source.is_char_boundary(diagnostic.byte as usize));
        }
        assert!(
            facts
                .diagnostics
                .windows(2)
                .all(|pair| pair[0].byte <= pair[1].byte)
        );
        for node in &facts.nodes {
            assert!(node.fact.lines[0] > 0);
            assert!(node.fact.lines[0] <= node.fact.lines[1]);
        }
        for reference in &facts.references {
            assert!(reference.fact.lines[0] > 0);
            assert!(reference.fact.lines[0] <= reference.fact.lines[1]);
        }
    }
    result
}

fn has_real(facts: &LexedFile) {
    assert!(
        facts
            .nodes
            .iter()
            .any(|node| node.fact.name.rsplit("::").next() == Some("real")),
        "{facts:?}"
    );
}

#[test]
fn empty_file() {
    let facts = bounded(b"").expect("empty file");
    assert_eq!(facts.nodes.len(), 1);
    assert!(facts.references.is_empty());
    assert!(facts.diagnostics.is_empty());
    assert_eq!(facts.completion, Completion::Complete);
}

#[test]
fn only_comments() {
    let facts = bounded("# def ghost; end\n=begin\nclass Hidden; end\n=end\n".as_bytes())
        .expect("comments");
    assert_eq!(facts.nodes.len(), 1);
    assert!(facts.references.is_empty());
    assert!(facts.diagnostics.is_empty());
}

#[test]
fn unterminated_literals_and_documentation() {
    for tail in &[
        "x = \"open",
        "x = 'open\\",
        "x = %q{open",
        "x = /open",
        "x = <<~DOC\nbody\n",
        "x = <<'DOC'\nbody\n",
        "x = \"#{call(",
        "=begin\nopen documentation",
    ] {
        let source = format!("{HEAD}{DECLARATION}{tail}");
        let facts = bounded(source.as_bytes()).expect("damaged source returns partial facts");
        has_real(&facts);
        assert_eq!(facts.completion, Completion::Partial, "{tail:?}");
        assert!(!facts.diagnostics.is_empty(), "{tail:?}");
    }
}

#[test]
fn ten_thousand_nested_brackets() {
    for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
        let source = format!(
            "{HEAD}{}0{}",
            open.to_string().repeat(10_000),
            close.to_string().repeat(10_000)
        );
        assert!(matches!(
            bounded(source.as_bytes()),
            Err(LexError::TooDeep { limit: 256, .. })
        ));
        let control = format!(
            "{HEAD}{}0{}",
            open.to_string().repeat(8),
            close.to_string().repeat(8)
        );
        assert!(bounded(control.as_bytes()).is_ok());
    }
}

#[test]
fn five_megabyte_single_line() {
    let mut source = format!("{HEAD}\"");
    source.push_str(&"x".repeat(5 * 1024 * 1024 - source.len() - 1));
    source.push('"');
    assert_eq!(source.len(), 5 * 1024 * 1024);
    assert!(!source.as_bytes().contains(&b'\n'));
    assert!(!source.as_bytes().contains(&b'\r'));
    assert!(bounded(source.as_bytes()).is_ok());
}

#[test]
fn nul_bytes() {
    for tail in ["\0", "\0\0\0", "\"a\0b\""] {
        let source = format!("{HEAD}{DECLARATION}{tail}");
        let facts = bounded(source.as_bytes()).expect("NUL recovery");
        has_real(&facts);
    }
}

#[test]
fn crlf_and_lone_cr() {
    for newline in ["\r\n", "\r"] {
        let source = format!("{HEAD}{}", DECLARATION.replace('\n', newline));
        has_real(&bounded(source.as_bytes()).expect("newline recovery"));
    }
}

#[test]
fn utf8_bom() {
    let source = format!("\u{feff}{HEAD}{DECLARATION}");
    let facts = bounded(source.as_bytes()).expect("UTF-8 BOM");
    has_real(&facts);
    assert!(facts.diagnostics.is_empty());
}

#[test]
fn invalid_utf8() {
    for invalid in [
        &b"\xff"[..],
        &b"\xc0\xaf"[..],
        &b"\xed\xa0\x80"[..],
        &b"\xf4\x90\x80\x80"[..],
        &b"\xe2\x82"[..],
    ] {
        let mut bytes = format!("{HEAD}{DECLARATION}").into_bytes();
        bytes.extend_from_slice(invalid);
        assert!(matches!(bounded(&bytes), Err(LexError::InvalidUtf8(_))));
    }
}

#[test]
fn mixed_tabs() {
    let source = format!("{HEAD}{}", "def real\n\tvalue = 1\n \tvalue\nend\n");
    has_real(&bounded(source.as_bytes()).expect("mixed indentation recovery"));
}

#[test]
fn unicode_identifiers() {
    let source = format!("{HEAD}{}", "def café(数)\n  数\nend\n");
    let facts = bounded(source.as_bytes()).expect("Unicode identifiers");
    assert!(facts.nodes.iter().any(|node| node.fact.name == "café"));
    assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
}

fn sample() -> Vec<u8> {
    let mut source = "# Inventory service: preserve structure during incomplete edits.\nrequire 'json'\nmodule Inventory\n  class Worker\n    attr_reader :rows\n    def real(café = \"数😀\")\n      @rows = JSON.parse(File.read(café))\n      puts <<~REPORT\n        Loaded records: #{@rows.size}\n      REPORT\n      { count: @rows.size, rows: @rows }\n    end\n  end\nend\nInventory::Worker.new.real(\"records.json\")\n".to_owned();
    assert!(source.len() < 398);
    source.push('#');
    source.extend(std::iter::repeat_n('x', 400 - source.len()));
    assert_eq!(source.len(), 400);
    source.into_bytes()
}

fn deterministic(bytes: &[u8]) {
    let first = bounded(bytes);
    let second = bounded(bytes);
    assert_eq!(
        first.as_ref().map_err(ToString::to_string),
        second.as_ref().map_err(ToString::to_string)
    );
    if std::str::from_utf8(bytes).is_err() {
        assert!(matches!(first, Err(LexError::InvalidUtf8(_))));
    } else {
        assert!(first.is_ok(), "unexpected refusal: {first:?}");
    }
}

#[test]
fn every_byte_offset_truncation_of_realistic_sample() {
    let sample = sample();
    has_real(&bounded(&sample).expect("complete sample"));
    for end in 0..=sample.len() {
        deterministic(&sample[..end]);
    }
}

#[test]
fn deterministic_byte_mutations() {
    let sample = sample();
    for offset in 0..sample.len() {
        let mut bytes = sample.clone();
        bytes[offset] = [0, b'\\', b'"', b'\r', 0xff][offset % 5];
        deterministic(&bytes);
    }
}

#[test]
fn fingerprint_normalization_preserves_all_whitespace() {
    for separator in [
        ' ', '\t', '\n', '\r', '\u{b}', '\u{c}', '\u{85}', '\u{a0}', '\u{1680}', '\u{2003}',
        '\u{2028}', '\u{2029}', '\u{202f}', '\u{205f}', '\u{3000}',
    ] {
        assert_eq!(
            dctx_lex_rb::body_fingerprint(&format!(
                "{separator}a{separator}{separator}b{separator}"
            )),
            dctx_lex_rb::body_fingerprint("a b")
        );
    }
}

#[test]
fn bom_preserves_column_zero_comment_and_data_markers() {
    for bom in ["", "\u{feff}"] {
        let source = format!("{bom}=begin\ndef ghost; end\n=end\n{DECLARATION}");
        let facts = bounded(source.as_bytes()).expect("block comment after BOM");
        has_real(&facts);
        assert!(!facts.nodes.iter().any(|node| node.fact.name == "ghost"));
        assert!(facts.diagnostics.is_empty());
        let source = format!("{bom}__END__\ndef ghost; end\n");
        let facts = bounded(source.as_bytes()).expect("data marker after BOM");
        assert_eq!(facts.nodes.len(), 1);
        assert!(facts.references.is_empty());
    }
}

#[test]
fn string_chunk_boundaries_preserve_unicode_escapes_and_following_code() {
    for escaped in ["n", "é", "€", "😀"] {
        let source = format!("{HEAD}value = \"\\{escaped}\";\n{DECLARATION}");
        let facts = bounded(source.as_bytes()).expect("escaped Unicode scalar");
        has_real(&facts);
        assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
    }

    for edge in [4093, 4094, 4095, 4096, 8191] {
        let padding = "x".repeat(edge);
        let source = format!("{HEAD}value = \"{padding}é€😀\\\"ghost()\";\n{DECLARATION}");
        let facts = bounded(source.as_bytes()).expect("string chunk boundary");
        has_real(&facts);
        assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
        assert!(
            !facts
                .references
                .iter()
                .any(|reference| reference.fact.name == "ghost")
        );
    }
}
