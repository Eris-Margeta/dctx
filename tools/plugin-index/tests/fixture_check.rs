// SPDX-License-Identifier: Apache-2.0
//! Signed local fixtures for the public index page.
//! Implements PLUGINS.md §7.2 PL-X: the git page verifies, and the named refusals stay refusals.
//! Depends on dev-only ed25519-dalek to mint a signature from the RFC 8032 test secret.
//! That secret is a published test vector. It is not a publisher key and never authorizes install.

#![forbid(unsafe_code)]

use ed25519_dalek::{Signer, SigningKey};
use plugin_index::{
    IndexError, PublisherPins, check_signed_index, decode_index_key, execute, index_message,
    validate_index,
};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn index_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/index")
}

fn contract_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/design/plugin-api/fixtures")
}

fn read(path: PathBuf) -> Vec<u8> {
    fs::read(&path).unwrap_or_else(|_| panic!("missing {}", path.display()))
}

/// RFC 8032 section 7.1 test 1 secret. Public test data only.
fn rfc_secret() -> [u8; 32] {
    decode_hex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
}

fn sign(index: &[u8]) -> ([u8; 32], [u8; 64]) {
    let signing = SigningKey::from_bytes(&rfc_secret());
    let key = signing.verifying_key().to_bytes();
    let signature = signing.sign(&index_message(index)).to_bytes();
    (key, signature)
}

#[test]
fn git_page_verifies_under_the_test_key_and_pins() {
    let index = read(index_dir().join("index.json"));
    let pins = PublisherPins::parse(&read(index_dir().join("publisher-pins.json"))).unwrap();
    let key = decode_index_key(&read(index_dir().join("test-index-key.hex"))).unwrap();
    let (signed_key, signature) = sign(&index);
    assert_eq!(signed_key, key);
    let page = check_signed_index(&index, &signature, &key, &pins).unwrap();
    assert_eq!(page.plugins[0].id, "dctx/git");
    assert_eq!(page.plugins[0].repository, "fixture/dctx-plugin-git");

    let mut files = HashMap::new();
    files.insert("index.json".to_owned(), index);
    files.insert("sig".to_owned(), Vec::from(signature));
    files.insert(
        "key".to_owned(),
        read(index_dir().join("test-index-key.hex")),
    );
    files.insert("pins".to_owned(), read(index_dir().join("publisher-pins.json")));
    execute(
        &[
            "check".to_owned(),
            "index.json".to_owned(),
            "sig".to_owned(),
            "key".to_owned(),
            "pins".to_owned(),
        ],
        |path| {
            files.get(path).cloned().ok_or_else(|| IndexError::Io {
                path: path.to_owned(),
            })
        },
    )
    .unwrap();
}

#[test]
fn named_refusals_survive_a_valid_test_signature() {
    let pins = PublisherPins::parse(&read(index_dir().join("publisher-pins.json"))).unwrap();
    let cases = [
        ("invalid/duplicate-id.json", IndexError::DuplicateId),
        ("invalid/alias-claim.json", IndexError::AliasClaim),
        ("invalid/reserved-alias.json", IndexError::AliasClaim),
        ("invalid/same-repository.json", IndexError::AliasClaim),
        ("invalid/unsigned-metadata.json", IndexError::UnsignedMetadata),
        ("invalid/changed-publisher.json", IndexError::ChangedPublisher),
        ("invalid/hostile-display.json", IndexError::HostileDisplay),
        ("invalid/userinfo-url.json", IndexError::Origin),
        ("invalid/http-url.json", IndexError::Schema("manifest_url")),
    ];
    for (name, expected) in cases {
        let bytes = read(index_dir().join(name));
        let (key, signature) = sign(&bytes);
        assert_eq!(
            check_signed_index(&bytes, &signature, &key, &pins),
            Err(expected),
            "{name}"
        );
    }
}

#[test]
fn missing_signature_changed_bytes_and_other_key_are_refused() {
    let index = read(index_dir().join("index.json"));
    let pins = PublisherPins::parse(&read(index_dir().join("publisher-pins.json"))).unwrap();
    let (key, signature) = sign(&index);
    assert_eq!(
        check_signed_index(&index, b"", &key, &pins),
        Err(IndexError::UnsignedMetadata)
    );
    let mut flipped = signature;
    flipped[0] ^= 0x01;
    assert_eq!(
        check_signed_index(&index, &flipped, &key, &pins),
        Err(IndexError::Signature)
    );
    let mut changed = index.clone();
    let last = changed.len() - 1;
    changed[last] ^= 0x01;
    assert_eq!(
        check_signed_index(&changed, &signature, &key, &pins),
        Err(IndexError::Signature)
    );
    let other = SigningKey::from_bytes(&[7u8; 32]).verifying_key().to_bytes();
    let other_sig = SigningKey::from_bytes(&[7u8; 32])
        .sign(&index_message(&index))
        .to_bytes();
    assert_eq!(
        check_signed_index(&index, &other_sig, &other, &pins),
        Err(IndexError::UnsignedMetadata)
    );
}

#[test]
fn contract_index_is_accepted_and_its_duplicate_is_not() {
    let valid = read(contract_dir().join("valid/index.json"));
    let index = validate_index(&valid).unwrap();
    assert_eq!(index.plugins[0].id, "dctx/git");
    let duplicate = read(contract_dir().join("invalid/index-duplicate-id.json"));
    assert_eq!(validate_index(&duplicate), Err(IndexError::DuplicateId));
    let userinfo = read(contract_dir().join("invalid/index-userinfo-url.json"));
    assert_eq!(validate_index(&userinfo), Err(IndexError::Origin));
    let http = read(contract_dir().join("invalid/index-http-url.json"));
    assert_eq!(validate_index(&http), Err(IndexError::Schema("manifest_url")));
}

#[test]
fn validate_command_accepts_the_page_and_publish_does_not_read() {
    let index = read(index_dir().join("index.json"));
    execute(&["validate".to_owned(), "index.json".to_owned()], |path| {
        assert_eq!(path, "index.json");
        Ok(index.clone())
    })
    .unwrap();
    assert_eq!(
        execute(&["publish".to_owned()], |_| unreachable!()),
        Err(IndexError::PublicationRefused)
    );
}

fn decode_hex(text: &str) -> [u8; 32] {
    let bytes = text.as_bytes();
    let mut out = [0u8; 32];
    for index in 0..32 {
        out[index] = (nibble(bytes[index * 2]) << 4) | nibble(bytes[index * 2 + 1]);
    }
    out
}

fn nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => unreachable!(),
    }
}
