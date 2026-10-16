//! Property tests against dev-only dalek and sha2 reference implementations.
//! Tests random signed messages, bit mutations, arbitrary encodings and hashes.
//! Every disagreement fails; test seeds are synthetic and never production keys.

#![forbid(unsafe_code)]

mod common;

use common::reference_accepts;
use dctx_ed25519::{Sha512, verify};
use ed25519_dalek::{Signer, SigningKey};
use proptest::prelude::*;
use sha2::Digest;

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn random_signatures_and_single_bit_mutations(
        seed in any::<[u8; 32]>(),
        message in prop::collection::vec(any::<u8>(), 1..4097),
        signature_bit in 0usize..512,
        key_bit in 0usize..256,
        message_bit in any::<usize>(),
    ) {
        let signing_key = SigningKey::from_bytes(&seed);
        let key = signing_key.verifying_key().to_bytes();
        let signature = signing_key.sign(&message).to_bytes();
        prop_assert!(reference_accepts(&key, &message, &signature));
        prop_assert_eq!(verify(&key, &message, &signature), Ok(()));

        let mut changed_signature = signature;
        changed_signature[signature_bit / 8] ^= 1 << (signature_bit % 8);
        prop_assert!(!reference_accepts(&key, &message, &changed_signature));
        prop_assert!(verify(&key, &message, &changed_signature).is_err());

        let mut changed_key = key;
        changed_key[key_bit / 8] ^= 1 << (key_bit % 8);
        prop_assert!(!reference_accepts(&changed_key, &message, &signature));
        prop_assert!(verify(&changed_key, &message, &signature).is_err());

        let mut changed_message = message.clone();
        let bit = message_bit % (changed_message.len() * 8);
        changed_message[bit / 8] ^= 1 << (bit % 8);
        prop_assert!(!reference_accepts(&key, &changed_message, &signature));
        prop_assert!(verify(&key, &changed_message, &signature).is_err());
    }

    #[test]
    fn arbitrary_exact_size_inputs_obey_reference_rejections(
        key in any::<[u8; 32]>(),
        signature in any::<[u8; 64]>(),
        message in prop::collection::vec(any::<u8>(), 0..1025),
    ) {
        let ours = verify(&key, &message, &signature).is_ok();
        let reference = reference_accepts(&key, &message, &signature);
        prop_assert_eq!(ours, reference, "release blocked: arbitrary input differential");
    }

    #[test]
    fn arbitrary_lengths_obey_reference_rejections(
        key in prop::collection::vec(any::<u8>(), 0..66),
        signature in prop::collection::vec(any::<u8>(), 0..130),
        message in prop::collection::vec(any::<u8>(), 0..257),
    ) {
        prop_assert_eq!(verify(&key, &message, &signature).is_ok(),
            reference_accepts(&key, &message, &signature));
    }

    #[test]
    fn sha512_matches_sha2_for_random_messages_and_chunking(
        message in prop::collection::vec(any::<u8>(), 0..16385),
        chunk_size in 1usize..513,
    ) {
        let expected: [u8; 64] = sha2::Sha512::digest(&message).into();
        prop_assert_eq!(Sha512::digest(&message), expected);
        let mut streaming = Sha512::new();
        for chunk in message.chunks(chunk_size) {
            streaming.update(&[]);
            streaming.update(chunk);
        }
        streaming.update(&[]);
        prop_assert_eq!(streaming.finalize(), expected);
    }
}

#[test]
fn every_single_bit_of_a_signed_sample_is_authenticated() {
    let signing_key = SigningKey::from_bytes(&[0x42; 32]);
    let key = signing_key.verifying_key().to_bytes();
    let message = b"dctx-plugin-manifest-v1";
    let signature = signing_key.sign(message).to_bytes();
    assert!(reference_accepts(&key, message, &signature));
    assert_eq!(verify(&key, message, &signature), Ok(()));
    for bit in 0..512 {
        let mut changed = signature;
        changed[bit / 8] ^= 1 << (bit % 8);
        assert!(
            !reference_accepts(&key, message, &changed),
            "signature bit {bit}"
        );
        assert!(
            verify(&key, message, &changed).is_err(),
            "signature bit {bit}"
        );
    }
    for bit in 0..256 {
        let mut changed = key;
        changed[bit / 8] ^= 1 << (bit % 8);
        assert!(
            !reference_accepts(&changed, message, &signature),
            "key bit {bit}"
        );
        assert!(
            verify(&changed, message, &signature).is_err(),
            "key bit {bit}"
        );
    }
    for bit in 0..message.len() * 8 {
        let mut changed = *message;
        changed[bit / 8] ^= 1 << (bit % 8);
        assert!(
            !reference_accepts(&key, &changed, &signature),
            "message bit {bit}"
        );
        assert!(
            verify(&key, &changed, &signature).is_err(),
            "message bit {bit}"
        );
    }
}

#[test]
fn sha512_padding_boundaries_at_every_split() {
    for length in [
        0, 1, 31, 32, 63, 64, 65, 111, 112, 113, 127, 128, 129, 239, 240, 255, 256, 257,
    ] {
        let message: Vec<_> = (0..length).map(|i| i as u8).collect();
        let expected: [u8; 64] = sha2::Sha512::digest(&message).into();
        for split in 0..=length {
            let mut hash = Sha512::default();
            hash.update(&message[..split]);
            hash.update(&[]);
            hash.update(&message[split..]);
            assert_eq!(
                hash.clone().finalize(),
                expected,
                "length {length}, split {split}"
            );
            assert_eq!(hash.finalize(), expected);
        }
    }
}
