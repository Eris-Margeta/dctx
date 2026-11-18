// SPDX-License-Identifier: Apache-2.0
use std::{sync::mpsc, thread, time::Duration};

use dctx_lex_rb::{LexError, Limits, RubyLexer};

fn check(path: &'static str, case: &str, bytes: Vec<u8>) {
    let len = bytes.len();
    let (tx, rx) = mpsc::channel();
    // The deadline also catches a scanner that stops making progress.
    let worker = thread::spawn(move || {
        for _ in 0..2 {
            if tx
                .send(RubyLexer::new(Limits::default()).extract(path, &bytes))
                .is_err()
            {
                return;
            }
        }
    });
    let receive = || {
        rx.recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|err| panic!("{path}: {case}: extraction failed to finish: {err}"))
    };
    let first = receive();
    let second = receive();
    worker.join().expect("lexer worker panicked");
    assert_eq!(first, second, "{path}: {case}: nondeterministic result");
    match first {
        Ok(file) => {
            assert_ne!(case, "invalid UTF-8", "{path}: {case}: expected a refusal");
            let records = file.nodes.len()
                + file.edges.len()
                + file.references.len()
                + file.diagnostics.len();
            // One fixed file record plus a small number of facts per source byte.
            assert!(
                records <= 1 + 4 * len,
                "{path}: {case}: {records} output records for {len} bytes"
            );
        }
        Err(LexError::InvalidUtf8(err)) if case == "invalid UTF-8" => {
            assert_eq!(err.valid_up_to(), 1);
        }
        Err(LexError::TooDeep { .. }) if case == "nested brackets" => {}
        Err(err) => panic!("{path}: {case}: unexpected refusal: {err}"),
    }
}

#[test]
fn hostile_inputs() {
    let cases: &[(&str, &[u8])] = &[
        ("empty", b""),
        ("NUL bytes", b"value = 0\0\nother = 1"),
        ("invalid UTF-8", b"x\xff\xc0\x80"),
        (
            "mixed line endings and BOM",
            b"\xef\xbb\xbfdef first\r\nend\rdef second\nend\r\n",
        ),
        ("unterminated string", b"value = \"unfinished"),
        ("unterminated comment", b"=begin\nunfinished\n"),
        ("unterminated heredoc", b"value = <<~END\n  unfinished\n"),
    ];
    let path = "hostile.rb";
    for &(case, bytes) in cases {
        check(path, case, bytes.to_vec());
    }
    check(
        path,
        "single five-mebibyte line",
        vec![b'x'; 5 * 1024 * 1024],
    );
    let mut brackets = vec![b'['; 100_000];
    brackets.extend_from_slice(b"\"value\"");
    brackets.resize(brackets.len() + 100_000, b']');
    check(path, "nested brackets", brackets);
    let mut quotes = b"\"".to_vec();
    quotes.extend(b"\\\"".repeat(100_000));
    check(path, "escaped quotes without a closer", quotes);
}
