// SPDX-License-Identifier: Apache-2.0
use std::{sync::mpsc, thread, time::Duration};

use dctx_lex_cfg::{CfgLexer, LexError, Limits};

fn check(path: &'static str, case: &str, bytes: Vec<u8>) {
    let len = bytes.len();
    let (tx, rx) = mpsc::channel();
    // The deadline also catches a scanner that stops making progress.
    let worker = thread::spawn(move || {
        for _ in 0..2 {
            if tx
                .send(CfgLexer::new(Limits::default()).extract(path, &bytes))
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
    for path in [
        "hostile.json",
        "hostile.jsonc",
        "hostile.toml",
        "hostile.yaml",
    ] {
        let (prefix, mixed_endings): (&[u8], &[u8]) = match path {
            "hostile.toml" => (b"value = ", b"one = 1\r\ntwo = 2\rthree = 3\n"),
            "hostile.yaml" => (b"value: ", b"one: 1\r\ntwo: 2\rthree: 3\n"),
            _ => (b"", b"{\r\n\"one\": 1,\r\"two\": 2\n}"),
        };
        check(path, "empty", vec![]);
        check(path, "invalid UTF-8", b"x\xff\xc0\x80".to_vec());
        check(path, "NUL bytes", [prefix, b"\"value\0more\""].concat());
        check(
            path,
            "unterminated string",
            [prefix, b"\"unfinished"].concat(),
        );
        check(
            path,
            "mixed line endings and BOM",
            [b"\xef\xbb\xbf", mixed_endings].concat(),
        );
        let mut line = [prefix, b"\""].concat();
        line.resize(5 * 1024 * 1024, b'x');
        check(path, "single five-mebibyte line", line);
        let mut brackets = prefix.to_vec();
        brackets.resize(brackets.len() + 100_000, b'[');
        brackets.extend_from_slice(b"\"value\"");
        brackets.resize(brackets.len() + 100_000, b']');
        check(path, "nested brackets", brackets);
        let mut quotes = [prefix, b"\""].concat();
        quotes.extend(b"\\\"".repeat(100_000));
        check(path, "escaped quotes without a closer", quotes);
    }
    check(
        "hostile.jsonc",
        "unterminated comment",
        b"{ /* unfinished".to_vec(),
    );
    check(
        "hostile.toml",
        "unterminated multiline literal",
        b"value = \"\"\"\nunfinished".to_vec(),
    );
}
