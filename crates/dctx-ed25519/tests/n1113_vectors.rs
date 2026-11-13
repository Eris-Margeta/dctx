// SPDX-License-Identifier: Apache-2.0
//! RFC 8032 section 7.1, TEST 1-3:
//! https://www.rfc-editor.org/rfc/rfc8032#section-7.1
//! The public crate verifies only. Its existing dalek dependency reproduces
//! signatures from the published seeds; dctx verifies the pinned signatures.

use dctx_ed25519::{VerificationError, verify};
use ed25519_dalek::{Signer, SigningKey};

struct Vector {
    seed: &'static str,
    key: &'static str,
    message: &'static str,
    signature: &'static str,
}

const VECTORS: [Vector; 3] = [
    Vector {
        seed: "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        key: "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        message: "",
        signature: concat!(
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155",
            "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
        ),
    },
    Vector {
        seed: "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
        key: "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        message: "72",
        signature: concat!(
            "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da",
            "085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00"
        ),
    },
    Vector {
        seed: "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
        key: "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
        message: "af82",
        signature: concat!(
            "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac",
            "18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a"
        ),
    },
];

fn decode<const N: usize>(value: &str) -> [u8; N] {
    hex::decode(value).unwrap().try_into().unwrap()
}

fn assert_sign_and_verify(vector: &Vector) {
    let seed = decode::<32>(vector.seed);
    let key = decode::<32>(vector.key);
    let message = hex::decode(vector.message).unwrap();
    let signature = decode::<64>(vector.signature);
    let signing_key = SigningKey::from_bytes(&seed);
    assert_eq!(signing_key.verifying_key().to_bytes(), key);
    let signed = signing_key.sign(&message).to_bytes();
    assert_eq!(signed, signature);
    assert_eq!(verify(&key, &message, &signature), Ok(()));
    assert_eq!(verify(&key, &message, &signed), Ok(()));
}

#[test]
fn rfc8032_test_1_sign_and_verify() {
    assert_sign_and_verify(&VECTORS[0]);
}

#[test]
fn rfc8032_test_2_sign_and_verify() {
    assert_sign_and_verify(&VECTORS[1]);
}

#[test]
fn rfc8032_test_3_sign_and_verify() {
    assert_sign_and_verify(&VECTORS[2]);
}

#[test]
fn tampered_messages_are_rejected() {
    for vector in &VECTORS {
        let key = decode::<32>(vector.key);
        let signature = decode::<64>(vector.signature);
        let message = hex::decode(vector.message).unwrap();
        assert_eq!(verify(&key, &message, &signature), Ok(()));
        let mut changed = message;
        if changed.is_empty() {
            changed.push(0);
        } else {
            changed[0] ^= 1;
        }
        assert!(verify(&key, &changed, &signature).is_err());
    }
}

#[test]
fn every_single_bit_signature_mutation_is_rejected() {
    for vector in &VECTORS {
        let key = decode::<32>(vector.key);
        let signature = decode::<64>(vector.signature);
        let message = hex::decode(vector.message).unwrap();
        assert_eq!(verify(&key, &message, &signature), Ok(()));
        for bit in 0..512 {
            let mut changed = signature;
            changed[bit / 8] ^= 1 << (bit % 8);
            assert!(
                verify(&key, &message, &changed).is_err(),
                "signature bit {bit}"
            );
        }
    }
}

#[test]
fn every_single_bit_key_mutation_is_rejected() {
    for vector in &VECTORS {
        let key = decode::<32>(vector.key);
        let signature = decode::<64>(vector.signature);
        let message = hex::decode(vector.message).unwrap();
        assert_eq!(verify(&key, &message, &signature), Ok(()));
        for bit in 0..256 {
            let mut changed = key;
            changed[bit / 8] ^= 1 << (bit % 8);
            assert!(
                verify(&changed, &message, &signature).is_err(),
                "key bit {bit}"
            );
        }
    }
}

#[test]
fn noncanonical_s_is_rejected() {
    let order = decode::<32>("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");
    for vector in &VECTORS {
        let key = decode::<32>(vector.key);
        let signature = decode::<64>(vector.signature);
        let message = hex::decode(vector.message).unwrap();
        assert_eq!(verify(&key, &message, &signature), Ok(()));
        let mut added_order = signature;
        let mut carry = 0u16;
        for (byte, modulus) in added_order[32..].iter_mut().zip(order) {
            let sum = u16::from(*byte) + u16::from(modulus) + carry;
            *byte = sum.to_le_bytes()[0];
            carry = sum >> 8;
        }
        assert_eq!(carry, 0);
        let mut equal_order = signature;
        equal_order[32..].copy_from_slice(&order);
        let mut maximal = signature;
        maximal[32..].fill(0xff);
        for changed in [equal_order, added_order, maximal] {
            assert_eq!(
                verify(&key, &message, &changed),
                Err(VerificationError::NonCanonicalScalar)
            );
        }
    }
}
