// SPDX-License-Identifier: Apache-2.0
use std::{sync::mpsc, thread, time::Duration};

use dctx_lex_md::{LexError, extract};

fn check(case: &str, bytes: Vec<u8>) {
    let len = bytes.len();
    let (tx, rx) = mpsc::channel();
    // The deadline also catches a scanner that stops making progress.
    let worker = thread::spawn(move || {
        for _ in 0..2 {
            if tx.send(extract(&bytes)).is_err() {
                return;
            }
        }
    });
    let receive = || {
        rx.recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|err| panic!("{case}: extraction failed to finish: {err}"))
    };
    let first = receive();
    let second = receive();
    worker.join().expect("lexer worker panicked");
    assert_eq!(first, second, "{case}: nondeterministic result");
    match first {
        Ok(file) => {
            assert_ne!(case, "invalid UTF-8", "{case}: expected a refusal");
            let records = file.headings.len()
                + file.references.len()
                + file.code_blocks.len()
                + usize::from(file.front_matter.is_some());
            // One fixed file record plus a small number of facts per source byte.
            assert!(
                records <= 1 + 4 * len,
                "{case}: {records} output records for {len} bytes"
            );
        }
        Err(LexError::InvalidUtf8 { valid_up_to: 1 }) if case == "invalid UTF-8" => {}
        Err(LexError::TooDeep { .. }) if case == "nested block quotes" => {}
        Err(err) => panic!("{case}: unexpected refusal: {err}"),
    }
}

#[test]
fn hostile_inputs() {
    let cases: &[(&str, &[u8])] = &[
        ("empty", b""),
        ("NUL bytes", b"# heading\ntext\0more\0text"),
        ("invalid UTF-8", b"x\xff\xc0\x80"),
        (
            "mixed line endings and BOM",
            b"\xef\xbb\xbf# First\r\ntext\r## Second\nmore\r\n",
        ),
        ("unterminated inline code", b"# Heading\ntext `unfinished"),
        ("unterminated HTML comment", b"<!-- unfinished"),
        ("unterminated code fence", b"```rust\nfn unfinished() {}\n"),
        ("trailing angle bracket", b"# Heading\nparagraph <"),
    ];
    for &(case, bytes) in cases {
        check(case, bytes.to_vec());
    }
    check("single five-mebibyte line", vec![b'x'; 5 * 1024 * 1024]);
    let mut brackets = b"# Heading\nparagraph ".to_vec();
    brackets.resize(brackets.len() + 100_000, b'[');
    brackets.extend_from_slice(b"\"value\"");
    brackets.resize(brackets.len() + 100_000, b']');
    check("nested brackets", brackets);
    let mut quotes = b"\"".to_vec();
    quotes.extend(b"\\\"".repeat(100_000));
    check("escaped quotes without a closer", quotes);
    let mut block_quotes = b"> ".repeat(100_000);
    block_quotes.extend_from_slice(b"text");
    check("nested block quotes", block_quotes);
}

#[test]
fn distinct_backtick_runs() {
    let mut text = b"# Heading\nparagraph ".to_vec();
    for width in 1..=1_000 {
        text.resize(text.len() + width, b'`');
        text.push(b'x');
    }
    text.resize(5 * 1024 * 1024, b'x');
    check("distinct backtick runs", text);
}
