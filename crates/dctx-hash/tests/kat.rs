// SPDX-License-Identifier: Apache-2.0

use dctx_hash::Sha256;
use std::io::{ErrorKind, Write};
use std::process::{Command, Stdio};

fn decode_hex(hex: &str) -> Vec<u8> {
    let (pairs, leftover) = hex.as_bytes().as_chunks::<2>();
    assert!(leftover.is_empty(), "incomplete hex byte");
    pairs
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn sha256_published_known_answers() {
    let records = include_str!("data/sha256-kat.txt")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let mut count = 0;
    for record in records {
        let fields: Vec<_> = record.split_ascii_whitespace().collect();
        let [length, message_hex, digest_hex] = fields.as_slice() else {
            panic!("invalid SHA-256 fixture row: {record}");
        };
        let length = length.parse::<usize>().unwrap();
        let message = match *message_hex {
            "-" => Vec::new(),
            "61*" => vec![b'a'; length],
            hex => decode_hex(hex),
        };
        assert_eq!(message.len(), length);
        let expected: [u8; 32] = decode_hex(digest_hex).try_into().unwrap();
        let digest = Sha256::digest(&message).unwrap();
        assert_eq!(*digest.as_bytes(), expected, "message length {length}");
        assert_eq!(digest.byte_length(), u64::try_from(length).unwrap());
        count += 1;
    }
    assert_eq!(count, 65 + 3 + 4 + 1);
}

fn split_message() -> Vec<u8> {
    (0..=u8::MAX).cycle().take(300).collect()
}

#[test]
fn sha256_streaming_matches_one_shot_at_every_split() {
    let message = split_message();
    let expected = Sha256::digest(&message).unwrap();
    for split in 0..=message.len() {
        let (prefix, suffix) = message.split_at(split);
        let mut hasher = Sha256::new();
        hasher.update(prefix).unwrap();
        hasher.update(b"").unwrap();
        hasher.update(suffix).unwrap();
        assert_eq!(hasher.finalize(), expected, "split at byte {split}");
    }
}

#[test]
fn sha256_agrees_with_sha256sum_when_available() {
    let mut child = match Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            eprintln!("skipping system cross-check: sha256sum is unavailable");
            return;
        }
        Err(error) => panic!("could not start sha256sum: {error}"),
    };
    let message = split_message();
    child.stdin.take().unwrap().write_all(&message).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "sha256sum: {}", output.status);
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let expected = decode_hex(stdout.split_ascii_whitespace().next().unwrap());
    assert_eq!(
        Sha256::digest(&message).unwrap().as_bytes().as_slice(),
        expected
    );
}
