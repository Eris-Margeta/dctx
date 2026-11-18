// SPDX-License-Identifier: Apache-2.0

use dctx_ed25519::{Sha512, VerificationError, verify};
use serde_json::Value;
use std::io::{ErrorKind, Write};
use std::process::{Command, Stdio};

fn hex_field(case: &Value, field: &str) -> Vec<u8> {
    hex::decode(case[field].as_str().unwrap()).unwrap()
}

#[test]
fn verifies_all_rfc8032_section_7_1_cases_and_rejects_bit_flips() {
    let vectors: Value =
        serde_json::from_str(include_str!("data/rfc8032-section-7.1.json")).unwrap();
    let vectors = vectors.as_array().unwrap();
    // Section 7.1 calls the 1023-byte message "TEST 1024".
    let cases = [
        ("1", 0),
        ("2", 1),
        ("3", 2),
        ("1024", 1023),
        ("SHA(abc)", 64),
    ];
    assert_eq!(vectors.len(), cases.len());

    for (vector, (name, length)) in vectors.iter().zip(cases) {
        assert_eq!(vector["name"], name);
        let key = hex_field(vector, "public_key");
        let mut message = hex_field(vector, "message");
        let mut signature = hex_field(vector, "signature");
        assert_eq!(message.len(), length, "RFC 8032 TEST {name}");
        assert_eq!(verify(&key, &message, &signature), Ok(()), "TEST {name}");

        signature[32] ^= 1;
        assert_eq!(
            verify(&key, &message, &signature),
            Err(VerificationError::EquationMismatch),
            "signature bit flip, TEST {name}"
        );
        signature[32] ^= 1;

        let mut changed_key = key.clone();
        changed_key[31] ^= 0x80;
        assert_eq!(
            verify(&changed_key, &message, &signature),
            Err(VerificationError::EquationMismatch),
            "public key bit flip, TEST {name}"
        );

        if let Some(byte) = message.get_mut(length / 2) {
            *byte ^= 1;
            assert_eq!(
                verify(&key, &message, &signature),
                Err(VerificationError::EquationMismatch),
                "message bit flip, TEST {name}"
            );
        }
    }
}

#[test]
fn verifies_independent_signature_on_exactly_1024_bytes() {
    let vector: Value = serde_json::from_str(include_str!("data/ed25519-1024-byte.json")).unwrap();
    let key = hex_field(&vector, "public_key");
    let message = hex_field(&vector, "message");
    let signature = hex_field(&vector, "signature");
    assert_eq!(message.len(), 1024);
    assert_eq!(verify(&key, &message, &signature), Ok(()));
    assert_eq!(
        verify(&key, &message[..1023], &signature),
        Err(VerificationError::EquationMismatch)
    );
}

#[test]
fn rejects_s_at_or_above_the_group_order() {
    let vectors: Value =
        serde_json::from_str(include_str!("data/rfc8032-section-7.1.json")).unwrap();
    let vector = &vectors[1];
    let key = hex_field(vector, "public_key");
    let message = hex_field(vector, "message");
    let mut signature = hex_field(vector, "signature");
    assert_eq!(verify(&key, &message, &signature), Ok(()));

    // Add L to the valid S in little-endian order, preserving R.
    let order =
        hex::decode("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010").unwrap();
    let mut carry = 0;
    for (byte, &order_byte) in signature[32..].iter_mut().zip(&order) {
        let sum = u16::from(*byte) + u16::from(order_byte) + carry;
        *byte = sum.to_le_bytes()[0];
        carry = sum >> 8;
    }
    assert_eq!(carry, 0);
    assert_eq!(
        verify(&key, &message, &signature),
        Err(VerificationError::NonCanonicalScalar),
        "S + L"
    );
    signature[32..].copy_from_slice(&order);
    assert_eq!(
        verify(&key, &message, &signature),
        Err(VerificationError::NonCanonicalScalar),
        "S = L"
    );
}

#[test]
fn rejects_small_order_and_noncanonical_public_keys() {
    let vectors: Value =
        serde_json::from_str(include_str!("data/rfc8032-section-7.1.json")).unwrap();
    let vector = &vectors[1];
    let key = hex_field(vector, "public_key");
    let message = hex_field(vector, "message");
    let signature = hex_field(vector, "signature");
    assert_eq!(verify(&key, &message, &signature), Ok(()));

    let policy: Value = serde_json::from_str(include_str!("data/policy.json")).unwrap();
    let small_order_keys = policy["small_order_points"].as_array().unwrap();
    assert_eq!(small_order_keys.len(), 8);
    for hex in small_order_keys {
        let small_order = hex::decode(hex.as_str().unwrap()).unwrap();
        assert_eq!(
            verify(&small_order, &message, &signature),
            Err(VerificationError::SmallOrderPublicKey),
            "small-order key {hex}"
        );
    }

    // RFC 8032 §5.1.3 rejects y >= p and a sign bit of 1 when x = 0.
    let mut y_equals_p = [0xff; 32];
    y_equals_p[0] = 0xed;
    y_equals_p[31] = 0x7f;
    let mut negative_zero = [0; 32];
    negative_zero[0] = 1;
    negative_zero[31] = 0x80;
    for key in [y_equals_p, negative_zero] {
        assert_eq!(
            verify(&key, &message, &signature),
            Err(VerificationError::PublicKeyEncoding)
        );
    }
}

#[test]
fn sha512_published_known_answers() {
    let records = include_str!("data/sha512-kat.txt")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let mut count = 0;
    for record in records {
        let fields: Vec<_> = record.split_ascii_whitespace().collect();
        let [length, message_hex, digest_hex] = fields.as_slice() else {
            panic!("invalid SHA-512 fixture row: {record}");
        };
        let length = length.parse::<usize>().unwrap();
        let message = match *message_hex {
            "-" => Vec::new(),
            "61*" => vec![b'a'; length],
            hex => hex::decode(hex).unwrap(),
        };
        assert_eq!(message.len(), length);
        let expected: [u8; 64] = hex::decode(digest_hex).unwrap().try_into().unwrap();
        assert_eq!(
            Sha512::digest(&message),
            expected,
            "message length {length}"
        );
        count += 1;
    }
    assert_eq!(count, 129 + 3 + 1);
}

fn split_message() -> Vec<u8> {
    (0..=u8::MAX).cycle().take(300).collect()
}

#[test]
fn sha512_streaming_matches_one_shot_at_every_split() {
    let message = split_message();
    let expected = Sha512::digest(&message);
    for split in 0..=message.len() {
        let (prefix, suffix) = message.split_at(split);
        let mut hasher = Sha512::new();
        hasher.update(prefix);
        hasher.update(b"");
        hasher.update(suffix);
        assert_eq!(hasher.finalize(), expected, "split at byte {split}");
    }
}

#[test]
fn sha512_agrees_with_sha512sum_when_available() {
    let mut child = match Command::new("sha512sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            eprintln!("skipping system cross-check: sha512sum is unavailable");
            return;
        }
        Err(error) => panic!("could not start sha512sum: {error}"),
    };
    let message = split_message();
    child.stdin.take().unwrap().write_all(&message).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "sha512sum: {}", output.status);
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let expected = stdout.split_ascii_whitespace().next().unwrap();
    assert_eq!(hex::encode(Sha512::digest(&message)), expected);
}
