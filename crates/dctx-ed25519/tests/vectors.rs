//! Release-blocking RFC 8032 §7.1 and pinned Wycheproof Ed25519 corpus tests.
//! Every published result is asserted, independently of the dalek comparison.
//! Embedded fixtures are offline; unknown results/flags and missing cases fail.

#![forbid(unsafe_code)]

mod common;

use common::reference_accepts;
use dctx_ed25519::verify;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

fn bytes(value: &Value) -> Vec<u8> {
    hex::decode(value.as_str().expect("hex string")).expect("valid fixture hex")
}

#[test]
fn all_rfc8032_section_7_1_vectors() {
    let vectors: Value =
        serde_json::from_str(include_str!("data/rfc8032-section-7.1.json")).unwrap();
    let vectors = vectors.as_array().unwrap();
    assert_eq!(vectors.len(), 5);
    let mut lengths = Vec::new();
    for vector in vectors {
        let key = bytes(&vector["public_key"]);
        let message = bytes(&vector["message"]);
        let signature = bytes(&vector["signature"]);
        lengths.push(message.len());
        assert!(
            reference_accepts(&key, &message, &signature),
            "RFC {} reference",
            vector["name"]
        );
        assert_eq!(
            verify(&key, &message, &signature),
            Ok(()),
            "RFC {}",
            vector["name"]
        );
        let mut changed = signature;
        changed[0] ^= 1;
        assert!(!reference_accepts(&key, &message, &changed));
        assert!(verify(&key, &message, &changed).is_err());
    }
    assert_eq!(lengths, [0, 1, 2, 1023, 64]);
}

#[test]
fn every_wycheproof_case_matches_published_result_and_reference() {
    let corpus_bytes = include_bytes!("data/ed25519_test.json");
    assert_eq!(
        hex::encode(Sha256::digest(corpus_bytes)),
        "752d2ea7d7c6cf4736381b6cbacb61f8182b126ab7cd9b058f00c50084975536"
    );
    let corpus: Value = serde_json::from_slice(corpus_bytes).unwrap();
    assert_eq!(corpus["algorithm"], "EDDSA");
    assert_eq!(corpus["schema"], "eddsa_verify_schema_v1.json");
    assert_eq!(corpus["numberOfTests"], 151);
    let expected_flags = BTreeMap::from([
        ("CompressedSignature", 4),
        ("InvalidEncoding", 22),
        ("InvalidKtv", 1),
        ("InvalidSignature", 20),
        ("Ktv", 38),
        ("SignatureMalleability", 8),
        ("SignatureWithGarbage", 5),
        ("TinkOverflow", 32),
        ("TruncatedSignature", 3),
        ("Valid", 18),
    ]);
    let mut flags = BTreeMap::new();
    let mut ids = BTreeSet::new();
    let (mut valid, mut invalid) = (0, 0);
    for group in corpus["testGroups"].as_array().unwrap() {
        assert_eq!(group["type"], "EddsaVerify");
        assert_eq!(group["publicKey"]["curve"], "edwards25519");
        let key = bytes(&group["publicKey"]["pk"]);
        for case in group["tests"].as_array().unwrap() {
            let id = case["tcId"].as_u64().unwrap();
            assert!(ids.insert(id), "duplicate tcId {id}");
            let expected = match case["result"].as_str().unwrap() {
                "valid" => {
                    valid += 1;
                    true
                }
                "invalid" => {
                    invalid += 1;
                    false
                }
                result => panic!("unreviewed Wycheproof result {result} in tcId {id}"),
            };
            for flag in case["flags"].as_array().unwrap() {
                let flag = flag.as_str().unwrap();
                assert!(
                    expected_flags.contains_key(flag),
                    "unreviewed edge class {flag}"
                );
                *flags.entry(flag).or_insert(0) += 1;
            }
            let message = bytes(&case["msg"]);
            let signature = bytes(&case["sig"]);
            let ours = verify(&key, &message, &signature);
            let reference = reference_accepts(&key, &message, &signature);
            assert_eq!(
                ours.is_ok(),
                expected,
                "tcId {id}: {} ({ours:?})",
                case["comment"]
            );
            assert_eq!(
                reference, expected,
                "reference disagrees with corpus tcId {id}"
            );
            assert_eq!(
                ours.is_ok(),
                reference,
                "release blocked: differential tcId {id}"
            );
        }
    }
    assert_eq!((valid, invalid), (88, 63));
    assert_eq!(ids, (1..=151).collect());
    assert_eq!(flags, expected_flags);
    assert_eq!(
        corpus["notes"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        expected_flags.keys().copied().collect()
    );
}
