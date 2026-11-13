// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile inputs for SHA-256 and every content-identity entry point.
//! Byte inputs have no text or item syntax. Duplicate/reordered items are byte
//! concatenations. The real SHA-256 size cap is checked by private state injection
//! in src; these tests never allocate or forge an astronomical slice.

#![forbid(unsafe_code)]

use dctx_hash::{ContentDigest, HashError, IDENTITY_VERSION, IdentityAlgorithm, Sha256};
use sha2::Digest;
use std::fmt::{self, Write};

fn sample(length: usize) -> Vec<u8> {
    (0..length).map(|i| i.to_le_bytes()[0]).collect()
}

fn reference(message: &[u8]) -> ContentDigest {
    ContentDigest::sha256(sha2::Sha256::digest(message).into(), message.len() as u64)
}

#[test]
fn empty_every_one_byte_and_invalid_utf8_match_reference() {
    let mut messages = vec![vec![], b"\xff\xfe\xc0\xaf\xed\xa0\x80\0".to_vec()];
    messages.extend((0..=u8::MAX).map(|byte| vec![byte]));
    for message in messages {
        let expected = reference(&message);
        assert_eq!(Sha256::digest(&message), Ok(expected));
        assert_eq!(Sha256::digest(&message), Ok(expected));
        let mut state = Sha256::default();
        state.update(&[]).unwrap();
        state.update(&message).unwrap();
        state.update(&[]).unwrap();
        assert_eq!(state.finalize(), expected);
        assert_eq!(
            state.finalize().to_string().as_bytes(),
            expected.to_string().as_bytes()
        );
    }
    assert_eq!(Sha256::new().finalize(), reference(&[]));
}

#[test]
fn every_truncation_and_split_matches_reference_without_panics() {
    let message = sample(145);
    for end in 0..=message.len() {
        let prefix = &message[..end];
        let expected = reference(prefix);
        assert_eq!(Sha256::digest(prefix), Ok(expected));
        for split in 0..=end {
            let mut state = Sha256::new();
            state.update(&prefix[..split]).unwrap();
            state.update(&[]).unwrap();
            state.update(&prefix[split..]).unwrap();
            assert_eq!(state.finalize(), expected, "prefix {end}, split {split}");
        }
    }
}

#[test]
fn padding_blocks_and_large_inputs_match_reference() {
    for length in [
        0, 1, 31, 32, 55, 56, 57, 63, 64, 65, 119, 120, 121, 127, 128, 129, 255, 256, 257, 1023,
        1024, 1025, 65_535, 65_536, 65_537,
    ] {
        let message = sample(length);
        let expected = reference(&message);
        assert_eq!(Sha256::digest(&message), Ok(expected));
        for size in [1, 7, 55, 56, 63, 64, 65, 1024] {
            let mut state = Sha256::new();
            for chunk in message.chunks(size) {
                state.update(chunk).unwrap();
                state.update(&[]).unwrap();
            }
            assert_eq!(state.finalize(), expected, "length {length}, chunk {size}");
        }
    }
}

#[test]
fn duplicates_and_reordering_are_hashed_as_exact_concatenations() {
    let first = b"\xfffirst\0";
    let second = b"\x80second\0";
    let mut digests = Vec::new();
    for chunks in [
        vec![first.as_slice(), second.as_slice()],
        vec![second.as_slice(), first.as_slice()],
        vec![first.as_slice(), first.as_slice(), second.as_slice()],
    ] {
        let bytes = chunks.concat();
        let expected = reference(&bytes);
        let mut state = Sha256::new();
        for chunk in chunks {
            state.update(chunk).unwrap();
        }
        assert_eq!(state.finalize(), expected);
        assert_eq!(Sha256::digest(&bytes), Ok(expected));
        assert!(!digests.contains(&expected));
        digests.push(expected);
    }
}

#[test]
fn copied_states_fork_and_finalization_is_repeatable() {
    let message = sample(145);
    let mut prefix = Sha256::new();
    prefix.update(&message[..63]).unwrap();
    let frozen = prefix.finalize();
    for suffix in [&message[63..], &message[..63], &[]] {
        let mut fork = prefix;
        fork.update(suffix).unwrap();
        let expected = reference(&[&message[..63], suffix].concat());
        assert_eq!(fork.finalize(), expected);
        assert_eq!(fork.finalize(), expected);
        assert_eq!(prefix.finalize(), frozen);
    }
    prefix.update(&[]).unwrap();
    assert_eq!(prefix.finalize(), frozen);
    prefix.update(&message[63..]).unwrap();
    assert_eq!(prefix.finalize(), reference(&message));
}

#[test]
fn every_algorithm_code_is_handled_without_aliases() {
    const ALGORITHM: IdentityAlgorithm = IdentityAlgorithm::Sha256;
    const CODE: u16 = ALGORITHM.code();
    const NAME: &str = ALGORITHM.name();
    const RESTORED: Option<IdentityAlgorithm> = IdentityAlgorithm::from_code(CODE);
    assert_eq!(CODE, 1);
    assert_eq!(NAME, "sha256");
    assert_eq!(RESTORED, Some(ALGORITHM));
    for code in 0..=u16::MAX {
        let expected = if code == IdentityAlgorithm::SHA256 {
            Some(ALGORITHM)
        } else {
            None
        };
        assert_eq!(IdentityAlgorithm::from_code(code), expected);
        assert_eq!(IdentityAlgorithm::from_code(code), expected);
    }
}

#[test]
fn metadata_all_byte_values_and_extreme_lengths_round_trip_exactly() {
    // Metadata construction does not hash or validate a claimed message length.
    // Cover both sides of the hasher cap and the entire metadata u64 range.
    for byte_length in [0, 1, u64::MAX / 8, u64::MAX / 8 + 1, u64::MAX] {
        for byte in 0..=u8::MAX {
            let bytes = [byte; 32];
            let digest = ContentDigest::sha256(bytes, byte_length);
            assert_eq!(digest.version(), IDENTITY_VERSION);
            assert_eq!(digest.version(), 1);
            assert_eq!(digest.algorithm(), IdentityAlgorithm::Sha256);
            assert_eq!(digest.algorithm_code(), 1);
            assert_eq!(digest.as_bytes(), &bytes);
            assert_eq!(digest.byte_length(), byte_length);
            assert_eq!(digest, ContentDigest::Sha256V1 { bytes, byte_length });
            let hex = format!("{byte:02x}").repeat(32);
            let expected = format!("v1:sha256:{hex}:{byte_length}");
            assert_eq!(digest.to_string(), expected);
            assert_eq!(digest.to_string().as_bytes(), expected.as_bytes());
        }
    }
}

#[test]
fn metadata_accessors_are_const_and_identity_includes_length_and_order() {
    const DIGEST: ContentDigest = ContentDigest::sha256([0xff; 32], u64::MAX);
    const VERSION: u8 = DIGEST.version();
    const ALGORITHM: IdentityAlgorithm = DIGEST.algorithm();
    const CODE: u16 = DIGEST.algorithm_code();
    const LENGTH: u64 = DIGEST.byte_length();
    const BYTES: &[u8; 32] = DIGEST.as_bytes();
    assert_eq!(
        (VERSION, ALGORITHM, CODE, LENGTH, BYTES),
        (1, IdentityAlgorithm::Sha256, 1, u64::MAX, &[0xff; 32])
    );
    let bytes: [u8; 32] = sample(32).try_into().unwrap();
    let digest = ContentDigest::sha256(bytes, 32);
    let mut reordered = bytes;
    reordered.reverse();
    assert_ne!(digest, ContentDigest::sha256(reordered, 32));
    assert_ne!(digest, ContentDigest::sha256(bytes, 33));
    assert_ne!(
        digest.to_string(),
        ContentDigest::sha256(reordered, 32).to_string()
    );
}

struct LimitedWriter {
    remaining: usize,
}

impl Write for LimitedWriter {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.remaining = self.remaining.checked_sub(text.len()).ok_or(fmt::Error)?;
        Ok(())
    }
}

#[test]
fn digest_display_propagates_every_output_truncation() {
    let digest = ContentDigest::sha256([0xff; 32], u64::MAX);
    let text = digest.to_string();
    for remaining in 0..text.len() {
        assert!(write!(&mut LimitedWriter { remaining }, "{digest}").is_err());
    }
    assert!(
        write!(
            &mut LimitedWriter {
                remaining: text.len()
            },
            "{digest}"
        )
        .is_ok()
    );
}

#[test]
fn hash_error_names_the_limit_and_next_step_and_propagates_format_errors() {
    let error = HashError::MessageTooLong;
    let text = error.to_string();
    assert!(text.contains("2^64-1 bits"), "missing limit: {text}");
    assert!(
        text.contains("hash fewer bytes"),
        "missing next step: {text}"
    );
    assert_eq!(error.to_string().as_bytes(), text.as_bytes());
    assert!(std::error::Error::source(&error).is_none());
    for remaining in 0..text.len() {
        assert!(write!(&mut LimitedWriter { remaining }, "{error}").is_err());
    }
}

#[test]
fn shared_inputs_and_copied_states_allow_concurrent_use() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Sha256>();
    assert_send_sync::<ContentDigest>();
    assert_send_sync::<IdentityAlgorithm>();
    assert_send_sync::<HashError>();
    let message = sample(1025);
    let expected = reference(&message);
    let error_text = HashError::MessageTooLong.to_string();
    let barrier = std::sync::Barrier::new(4);
    let mut prefix = Sha256::new();
    prefix.update(&message[..63]).unwrap();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    for _ in 0..8 {
                        assert_eq!(Sha256::digest(&message), Ok(expected));
                        let mut state = prefix;
                        state.update(&[]).unwrap();
                        state.update(&message[63..]).unwrap();
                        assert_eq!(state.finalize(), expected);
                        assert_eq!(
                            state.finalize().to_string().as_bytes(),
                            expected.to_string().as_bytes()
                        );
                        assert_eq!(
                            IdentityAlgorithm::from_code(expected.algorithm_code()),
                            Some(expected.algorithm())
                        );
                        assert_eq!(Sha256::default().finalize(), reference(&[]));
                        assert_eq!(HashError::MessageTooLong.to_string(), error_text);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
    });
    assert_eq!(prefix.finalize(), reference(&message[..63]));
}
