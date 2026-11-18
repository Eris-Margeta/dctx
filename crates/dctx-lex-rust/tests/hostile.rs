// SPDX-License-Identifier: Apache-2.0
use std::{sync::mpsc, thread, time::Duration};

use dctx_lex_rust::{Limits, Refusal, extract};

fn check(case: &str, bytes: Vec<u8>) {
    let len = bytes.len();
    let (tx, rx) = mpsc::channel();
    // The deadline also catches a scanner that stops making progress.
    let worker = thread::spawn(move || {
        for _ in 0..2 {
            if tx.send(extract(&bytes, &Limits::default())).is_err() {
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
            let records = file.declarations.len()
                + file.imports.len()
                + file.references.len()
                + usize::from(file.module_doc.is_some());
            // One fixed file record plus a small number of facts per source byte.
            assert!(
                records <= 1 + 4 * len,
                "{case}: {records} output records for {len} bytes"
            );
        }
        Err(Refusal::InvalidUtf8 { valid_up_to: 1 }) if case == "invalid UTF-8" => {}
        Err(err) => panic!("{case}: unexpected refusal: {err}"),
    }
}

#[test]
fn hostile_inputs() {
    let cases: &[(&str, &[u8])] = &[
        ("empty", b""),
        ("NUL bytes", b"fn first() {\0}\nfn second() {}"),
        ("invalid UTF-8", b"x\xff\xc0\x80"),
        (
            "mixed line endings and BOM",
            b"\xef\xbb\xbffn first() {}\r\nfn second() {}\rfn third() {}\n",
        ),
        ("unterminated string", b"const S: &str = \"unfinished"),
        ("unterminated comment", b"/* nested /* unfinished */"),
        (
            "unterminated multiline literal",
            b"const S: &str = r#\"unfinished",
        ),
    ];
    for &(case, bytes) in cases {
        check(case, bytes.to_vec());
    }
    check("single five-mebibyte line", vec![b'x'; 5 * 1024 * 1024]);
    let mut brackets = vec![b'['; 100_000];
    brackets.extend_from_slice(b"\"value\"");
    brackets.resize(brackets.len() + 100_000, b']');
    check("nested brackets", brackets);
    let mut quotes = b"\"".to_vec();
    quotes.extend(b"\\\"".repeat(100_000));
    check("escaped quotes without a closer", quotes);
}
