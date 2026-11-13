// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile inputs for every public verification and SHA-512 entry point.
//! Fixed-size inputs exercise their exact maxima and suffix rejection. SHA-512's
//! astronomical length limit is exercised with private counter injection in src.

#![forbid(unsafe_code)]

use dctx_ed25519::{Sha512, VerificationError, verify};
use ed25519_dalek::{Signer, SigningKey};
use sha2::Digest;
use std::fmt::{self, Write};

fn signer() -> SigningKey {
    SigningKey::from_bytes(&[0x31; 32])
}

fn sample(length: usize) -> Vec<u8> {
    (0..length).map(|i| i.to_le_bytes()[0]).collect()
}

fn reference(message: &[u8]) -> [u8; 64] {
    sha2::Sha512::digest(message).into()
}

#[test]
fn verify_all_key_and_signature_truncations_and_overlong_sizes() {
    let signer = signer();
    let key = signer.verifying_key().to_bytes();
    let message = b"\xff\0signed bytes\x80";
    let signature = signer.sign(message).to_bytes();
    assert_eq!(verify(&key, message, &signature), Ok(()));
    for end in 0..key.len() {
        assert_eq!(
            verify(&key[..end], message, &signature),
            Err(VerificationError::PublicKeyLength),
            "key prefix {end}"
        );
    }
    for end in 0..signature.len() {
        assert_eq!(
            verify(&key, message, &signature[..end]),
            Err(VerificationError::SignatureLength),
            "signature prefix {end}"
        );
    }
    for length in [33, 64, 65, 4096] {
        let mut overlong = key.to_vec();
        overlong.resize(length, 0);
        assert_eq!(
            verify(&overlong, message, &signature),
            Err(VerificationError::PublicKeyLength)
        );
    }
    for length in [65, 128, 129, 4096] {
        let mut overlong = signature.to_vec();
        overlong.resize(length, 0);
        assert_eq!(
            verify(&key, message, &overlong),
            Err(VerificationError::SignatureLength)
        );
    }
    assert_eq!(verify(&key, message, &signature), Ok(()));
}

#[test]
fn verify_empty_every_one_byte_and_invalid_utf8_messages() {
    let signer = signer();
    let key = signer.verifying_key().to_bytes();
    let mut messages = vec![vec![], b"\xff\xfe\xc0\xaf\xed\xa0\x80\0".to_vec()];
    messages.extend((0..=u8::MAX).map(|byte| vec![byte]));
    for message in messages {
        let signature = signer.sign(&message).to_bytes();
        assert_eq!(verify(&key, &message, &signature), Ok(()));
        let mut changed = message.clone();
        changed.push(0);
        assert_eq!(
            verify(&key, &changed, &signature),
            Err(VerificationError::EquationMismatch)
        );
    }
}

#[test]
fn verify_authenticates_every_message_prefix_including_empty() {
    let signer = signer();
    let key = signer.verifying_key().to_bytes();
    let message = sample(129);
    let signature = signer.sign(&message).to_bytes();
    for end in 0..=message.len() {
        let prefix = &message[..end];
        let expected = if end == message.len() {
            Ok(())
        } else {
            Err(VerificationError::EquationMismatch)
        };
        assert_eq!(verify(&key, prefix, &signature), expected, "prefix {end}");
        // A truncated message is still legal when signed as those exact bytes.
        let signature = signer.sign(prefix).to_bytes();
        assert_eq!(verify(&key, prefix, &signature), Ok(()));
    }
}

#[test]
fn verify_duplicate_reordered_and_large_messages_bind_exact_bytes() {
    let signer = signer();
    let key = signer.verifying_key().to_bytes();
    let message = b"first\0second\xff";
    let signature = signer.sign(message).to_bytes();
    for changed in [
        [message.as_slice(), message.as_slice()].concat(),
        b"second\xfffirst\0".to_vec(),
        b"first\0second\xffsecond\xff".to_vec(),
        b"first\0second".to_vec(),
    ] {
        assert_eq!(
            verify(&key, &changed, &signature),
            Err(VerificationError::EquationMismatch)
        );
        assert_eq!(
            verify(&key, &changed, &signer.sign(&changed).to_bytes()),
            Ok(())
        );
    }
    assert_eq!(
        verify(&[key, key].concat(), message, &signature),
        Err(VerificationError::PublicKeyLength)
    );
    assert_eq!(
        verify(&key, message, &[signature, signature].concat()),
        Err(VerificationError::SignatureLength)
    );
    let mut swapped = signature;
    swapped.rotate_left(32);
    assert!(verify(&key, message, &swapped).is_err());
    let mut reordered_key = key;
    reordered_key.reverse();
    assert!(verify(&reordered_key, message, &signature).is_err());
    // Messages have no application size cap; cross several hash-block boundaries.
    for length in [111, 112, 127, 128, 129, 65_535, 65_536, 65_537] {
        let message = sample(length);
        let signature = signer.sign(&message).to_bytes();
        assert_eq!(verify(&key, &message, &signature), Ok(()));
    }
}

#[test]
fn verify_scalar_boundary_and_invalid_points_have_typed_errors() {
    let signer = signer();
    let key = signer.verifying_key().to_bytes();
    let message = b"scalar boundary";
    let signature = signer.sign(message).to_bytes();
    let order: [u8; 32] =
        hex::decode("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010")
            .unwrap()
            .try_into()
            .unwrap();
    let mut below_order = order;
    below_order[0] -= 1;
    let mut above_order = order;
    above_order[0] += 1;
    for scalar in [[0; 32], below_order, order, above_order, [0xff; 32]] {
        let mut changed = signature;
        changed[32..].copy_from_slice(&scalar);
        let expected = if scalar == below_order || scalar == [0; 32] {
            VerificationError::EquationMismatch
        } else {
            VerificationError::NonCanonicalScalar
        };
        assert_eq!(verify(&key, message, &changed), Err(expected));
    }
    let mut identity = [0; 32];
    identity[0] = 1;
    let mut negative_zero = identity;
    negative_zero[31] = 0x80;
    for (point, key_error, r_error) in [
        (
            [0; 32],
            VerificationError::SmallOrderPublicKey,
            VerificationError::SmallOrderSignaturePoint,
        ),
        (
            identity,
            VerificationError::SmallOrderPublicKey,
            VerificationError::SmallOrderSignaturePoint,
        ),
        (
            negative_zero,
            VerificationError::PublicKeyEncoding,
            VerificationError::SignaturePointEncoding,
        ),
        (
            [0xff; 32],
            VerificationError::PublicKeyEncoding,
            VerificationError::SignaturePointEncoding,
        ),
    ] {
        assert_eq!(verify(&point, message, &signature), Err(key_error));
        let mut changed = signature;
        changed[..32].copy_from_slice(&point);
        assert_eq!(verify(&key, message, &changed), Err(r_error));
    }
    assert_eq!(verify(&key, message, &signature), Ok(()));
}

#[test]
fn verify_repeat_calls_and_error_precedence_are_stable() {
    let signer = signer();
    let key = signer.verifying_key().to_bytes();
    let message = b"repeat";
    let signature = signer.sign(message).to_bytes();
    for _ in 0..8 {
        assert_eq!(
            verify(&[], &[], &[]),
            Err(VerificationError::PublicKeyLength)
        );
        assert_eq!(
            verify(&key, message, &[]),
            Err(VerificationError::SignatureLength)
        );
        assert_eq!(
            verify(&[0xff; 32], message, &[0xff; 64]),
            Err(VerificationError::NonCanonicalScalar)
        );
        assert_eq!(
            verify(&key, b"changed", &signature),
            Err(VerificationError::EquationMismatch)
        );
        assert_eq!(verify(&key, message, &signature), Ok(()));
    }
}

#[test]
fn sha512_empty_one_byte_and_binary_inputs_match_reference() {
    let mut messages = vec![vec![], b"\xff\xfe\xc0\xaf\xed\xa0\x80\0".to_vec()];
    messages.extend((0..=u8::MAX).map(|byte| vec![byte]));
    for message in messages {
        let expected = reference(&message);
        assert_eq!(Sha512::digest(&message), expected);
        assert_eq!(Sha512::digest(&message), expected);
        let mut state = Sha512::default();
        state.update(&[]);
        state.update(&message);
        state.update(&[]);
        assert_eq!(state.clone().finalize(), expected);
        assert_eq!(state.finalize(), expected);
    }
    assert_eq!(Sha512::new().finalize(), reference(&[]));
}

#[test]
fn sha512_every_truncation_and_split_matches_reference_without_panics() {
    let message = sample(145);
    for end in 0..=message.len() {
        let prefix = &message[..end];
        let expected = reference(prefix);
        assert_eq!(Sha512::digest(prefix), expected);
        for split in 0..=end {
            let mut state = Sha512::new();
            state.update(&prefix[..split]);
            state.update(&[]);
            state.update(&prefix[split..]);
            assert_eq!(state.finalize(), expected, "prefix {end}, split {split}");
        }
    }
}

#[test]
fn sha512_padding_blocks_and_large_inputs_match_reference() {
    for length in [
        0, 1, 63, 64, 65, 111, 112, 113, 127, 128, 129, 239, 240, 241, 255, 256, 257, 1023, 1024,
        1025, 65_535, 65_536, 65_537,
    ] {
        let message = sample(length);
        let expected = reference(&message);
        assert_eq!(Sha512::digest(&message), expected);
        for size in [1, 7, 111, 112, 127, 128, 129, 1024] {
            let mut state = Sha512::new();
            for chunk in message.chunks(size) {
                state.update(chunk);
                state.update(&[]);
            }
            assert_eq!(state.finalize(), expected, "length {length}, chunk {size}");
        }
    }
}

#[test]
fn sha512_duplicates_order_and_clone_forks_preserve_exact_bytes() {
    let first = b"\xfffirst\0";
    let second = b"\x80second\0";
    let mut prefix = Sha512::new();
    prefix.update(first);
    let frozen = prefix.clone().finalize();
    for chunks in [
        vec![first.as_slice(), second.as_slice()],
        vec![second.as_slice(), first.as_slice()],
        vec![first.as_slice(), first.as_slice(), second.as_slice()],
    ] {
        let bytes = chunks.concat();
        let mut state = Sha512::new();
        for chunk in chunks {
            state.update(chunk);
        }
        assert_eq!(state.finalize(), reference(&bytes));
    }
    for suffix in [first.as_slice(), second.as_slice(), &[]] {
        let mut fork = prefix.clone();
        fork.update(suffix);
        assert_eq!(
            fork.finalize(),
            reference(&[first.as_slice(), suffix].concat())
        );
        assert_eq!(prefix.clone().finalize(), frozen);
    }
    assert_ne!(
        reference(&[first.as_slice(), second.as_slice()].concat()),
        reference(&[second.as_slice(), first.as_slice()].concat())
    );
}

#[test]
fn shared_inputs_and_cloned_sha512_states_allow_concurrent_use() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Sha512>();
    assert_send_sync::<VerificationError>();
    let signer = signer();
    let key = signer.verifying_key().to_bytes();
    let message = sample(1025);
    let signature = signer.sign(&message).to_bytes();
    let mut prefix = Sha512::new();
    prefix.update(&message[..129]);
    let expected = reference(&message);
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    for _ in 0..4 {
                        assert_eq!(verify(&key, &message, &signature), Ok(()));
                        assert_eq!(
                            verify(&key[..1], &message, &signature),
                            Err(VerificationError::PublicKeyLength)
                        );
                        assert_eq!(
                            verify(&key, &message[..1], &signature),
                            Err(VerificationError::EquationMismatch)
                        );
                        let mut state = prefix.clone();
                        state.update(&message[129..]);
                        assert_eq!(state.finalize(), expected);
                        assert_eq!(Sha512::digest(&message), expected);
                        assert_eq!(Sha512::default().finalize(), reference(&[]));
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
    });
    assert_eq!(prefix.finalize(), reference(&message[..129]));
}

struct RefuseWrites;

impl Write for RefuseWrites {
    fn write_str(&mut self, _: &str) -> fmt::Result {
        Err(fmt::Error)
    }
}

#[test]
fn every_verification_error_is_stable_actionable_and_propagates_format_errors() {
    use VerificationError::*;
    let errors = [
        (PublicKeyLength, "provide"),
        (SignatureLength, "provide"),
        (PublicKeyEncoding, "obtain"),
        (SmallOrderPublicKey, "obtain"),
        (SignaturePointEncoding, "obtain"),
        (SmallOrderSignaturePoint, "obtain"),
        (NonCanonicalScalar, "obtain"),
        (EquationMismatch, "check"),
    ];
    let mut rendered = Vec::new();
    for (error, action) in errors {
        let text = error.to_string();
        assert!(text.contains(action), "missing next step: {text}");
        assert_eq!(error.to_string().as_bytes(), text.as_bytes());
        assert!(std::error::Error::source(&error).is_none());
        assert!(write!(&mut RefuseWrites, "{error}").is_err());
        assert!(
            !rendered.contains(&text),
            "errors must remain distinguishable"
        );
        rendered.push(text);
    }
}
