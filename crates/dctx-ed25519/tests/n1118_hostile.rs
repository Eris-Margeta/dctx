// SPDX-License-Identifier: Apache-2.0

use dctx_ed25519::{VerificationError, verify};
use serde_json::Value;

struct Vector {
    name: String,
    key: [u8; 32],
    message: Vec<u8>,
    signature: [u8; 64],
}

fn vectors() -> Vec<Vector> {
    let fixture: Value =
        serde_json::from_str(include_str!("data/rfc8032-section-7.1.json")).unwrap();
    let bytes = |field: &Value| hex::decode(field.as_str().unwrap()).unwrap();
    fixture
        .as_array()
        .unwrap()
        .iter()
        .map(|record| Vector {
            name: record["name"].as_str().unwrap().to_owned(),
            key: bytes(&record["public_key"]).try_into().unwrap(),
            message: bytes(&record["message"]),
            signature: bytes(&record["signature"]).try_into().unwrap(),
        })
        .collect()
}

#[test]
fn verifies_all_five_rfc8032_ed25519_vectors() {
    let vectors = vectors();
    assert_eq!(vectors.len(), 5);
    for vector in vectors {
        assert_eq!(
            verify(&vector.key, &vector.message, &vector.signature),
            Ok(()),
            "RFC 8032 vector {}",
            vector.name
        );
    }
}

#[test]
fn rfc8032_vectors_reject_single_bit_changes() {
    for Vector {
        name,
        key,
        message,
        signature,
    } in vectors()
    {
        if !message.is_empty() {
            let mut changed = message.clone();
            changed[message.len() / 2] ^= 0x80;
            assert_eq!(
                verify(&key, &changed, &signature),
                Err(VerificationError::EquationMismatch),
                "message, vector {name}"
            );
        }
        let mut changed_key = key;
        changed_key[31] ^= 0x80;
        assert!(
            verify(&changed_key, &message, &signature).is_err(),
            "key, vector {name}"
        );

        // Flip a bit in R and then in S, independently.
        for index in [0, 32] {
            let mut changed_signature = signature;
            changed_signature[index] ^= 1;
            assert!(
                verify(&key, &message, &changed_signature).is_err(),
                "signature byte {index}, vector {name}"
            );
        }
    }
}

#[test]
fn rfc8032_vectors_reject_short_and_long_encodings() {
    for Vector {
        name,
        key,
        message,
        signature,
    } in vectors()
    {
        let mut long_key = key.to_vec();
        long_key.push(0);
        for malformed in [&key[..0], &key[..31], long_key.as_slice()] {
            assert_eq!(
                verify(malformed, &message, &signature),
                Err(VerificationError::PublicKeyLength),
                "key length {}, vector {name}",
                malformed.len()
            );
        }

        let mut long_signature = signature.to_vec();
        long_signature.push(0);
        for malformed in [&signature[..0], &signature[..63], long_signature.as_slice()] {
            assert_eq!(
                verify(&key, &message, malformed),
                Err(VerificationError::SignatureLength),
                "signature length {}, vector {name}",
                malformed.len()
            );
        }
    }
}

#[test]
fn adding_the_group_order_to_rfc8032_scalars_is_rejected() {
    let order =
        hex::decode("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010").unwrap();
    for Vector {
        name,
        key,
        message,
        mut signature,
    } in vectors()
    {
        let mut carry = false;
        for (byte, &order_byte) in signature[32..].iter_mut().zip(&order) {
            let (sum, order_overflow) = byte.overflowing_add(order_byte);
            let (sum, carry_overflow) = sum.overflowing_add(u8::from(carry));
            *byte = sum;
            carry = order_overflow || carry_overflow;
        }
        assert!(!carry);
        assert_eq!(
            verify(&key, &message, &signature),
            Err(VerificationError::NonCanonicalScalar),
            "S + L, vector {name}"
        );
        signature[32..].copy_from_slice(&order);
        assert_eq!(
            verify(&key, &message, &signature),
            Err(VerificationError::NonCanonicalScalar),
            "S = L, vector {name}"
        );
    }
}
