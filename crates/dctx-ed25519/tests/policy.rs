// SPDX-License-Identifier: Apache-2.0
//! Adversarial checks for the documented PL-ED encoding and torsion policy.
//! Public algebra fixtures distinguish strict/cofactorless from cofactored checks.
//! Each assertion has a valid control and dalek strict verification as reference.

#![forbid(unsafe_code)]

mod common;

use common::reference_accepts;
use dctx_ed25519::{VerificationError, verify};
use serde_json::Value;

fn fixtures() -> Value {
    serde_json::from_str(include_str!("data/policy.json")).unwrap()
}

fn bytes(value: &Value) -> Vec<u8> {
    hex::decode(value.as_str().unwrap()).unwrap()
}

fn valid_control() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let fixtures = fixtures();
    let case = &fixtures["cofactorless"][0];
    let key = bytes(&case["public_key"]);
    let message = bytes(&case["message"]);
    let signature = bytes(&case["signature"]);
    assert!(reference_accepts(&key, &message, &signature));
    assert_eq!(verify(&key, &message, &signature), Ok(()));
    (key, message, signature)
}

#[test]
fn all_eight_small_order_points_are_rejected_as_key_and_r() {
    let (key, message, signature) = valid_control();
    let fixtures = fixtures();
    let points = fixtures["small_order_points"].as_array().unwrap();
    assert_eq!(points.len(), 8);
    for encoded in points {
        let point = bytes(encoded);
        assert!(!reference_accepts(&point, &message, &signature));
        assert_eq!(
            verify(&point, &message, &signature),
            Err(VerificationError::SmallOrderPublicKey)
        );
        let mut changed = signature.clone();
        changed[..32].copy_from_slice(&point);
        assert!(!reference_accepts(&key, &message, &changed));
        assert_eq!(
            verify(&key, &message, &changed),
            Err(VerificationError::SmallOrderSignaturePoint)
        );
    }
}

#[test]
fn cofactorless_equation_and_reduced_challenge_match_dalek() {
    let fixtures = fixtures();
    let cases = fixtures["cofactorless"].as_array().unwrap();
    assert_eq!(cases.len(), 6);
    let mut accepted = 0;
    for case in cases {
        let key = bytes(&case["public_key"]);
        let message = bytes(&case["message"]);
        let signature = bytes(&case["signature"]);
        let expected = case["valid"].as_bool().unwrap();
        let reference = reference_accepts(&key, &message, &signature);
        assert_eq!(reference, expected, "{} reference", case["name"]);
        assert_eq!(
            verify(&key, &message, &signature).is_ok(),
            reference,
            "{}",
            case["name"]
        );
        accepted += usize::from(expected);
    }
    assert_eq!(accepted, 3);
}

#[test]
fn noncanonical_coordinates_and_negative_zero_are_rejected() {
    let (key, message, signature) = valid_control();
    for low in 0xed..=0xff {
        for sign in [0, 0x80] {
            let mut encoded = [0xff; 32];
            encoded[0] = low;
            encoded[31] = 0x7f | sign;
            assert_eq!(
                verify(&encoded, &message, &signature),
                Err(VerificationError::PublicKeyEncoding)
            );
            let mut changed = signature.clone();
            changed[..32].copy_from_slice(&encoded);
            assert!(!reference_accepts(&key, &message, &changed));
            assert_eq!(
                verify(&key, &message, &changed),
                Err(VerificationError::SignaturePointEncoding)
            );
        }
    }
    // x=0 has only sign zero, for both y=1 and y=-1.
    let mut minus_one = [0xff; 32];
    minus_one[0] = 0xec;
    let mut identity = [0; 32];
    identity[0] = 1;
    identity[31] = 0x80;
    for encoded in [minus_one, identity] {
        assert!(!reference_accepts(&encoded, &message, &signature));
        assert_eq!(
            verify(&encoded, &message, &signature),
            Err(VerificationError::PublicKeyEncoding)
        );
        let mut changed = signature.clone();
        changed[..32].copy_from_slice(&encoded);
        assert!(!reference_accepts(&key, &message, &changed));
        assert_eq!(
            verify(&key, &message, &changed),
            Err(VerificationError::SignaturePointEncoding)
        );
    }
}

#[test]
fn scalar_malleability_and_all_neighboring_lengths_fail() {
    let (key, message, signature) = valid_control();
    let order =
        hex::decode("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010").unwrap();
    let mut changed = signature.clone();
    let mut carry = 0u16;
    for (byte, modulus) in changed[32..].iter_mut().zip(&order) {
        let sum = u16::from(*byte) + u16::from(*modulus) + carry;
        *byte = sum.to_le_bytes()[0];
        carry = sum >> 8;
    }
    assert!(!reference_accepts(&key, &message, &changed));
    assert_eq!(
        verify(&key, &message, &changed),
        Err(VerificationError::NonCanonicalScalar)
    );
    for increment in 0..=18 {
        changed[32..].copy_from_slice(&order);
        changed[32] += increment;
        assert!(!reference_accepts(&key, &message, &changed));
        assert_eq!(
            verify(&key, &message, &changed),
            Err(VerificationError::NonCanonicalScalar)
        );
    }
    for length in (0..=96).filter(|length| *length != 64) {
        let mut changed = signature.clone();
        changed.resize(length, 0);
        assert!(!reference_accepts(&key, &message, &changed));
        assert_eq!(
            verify(&key, &message, &changed),
            Err(VerificationError::SignatureLength)
        );
    }
    for length in (0..=64).filter(|length| *length != 32) {
        let mut changed = key.clone();
        changed.resize(length, 0);
        assert!(!reference_accepts(&changed, &message, &signature));
        assert_eq!(
            verify(&changed, &message, &signature),
            Err(VerificationError::PublicKeyLength)
        );
    }
}
