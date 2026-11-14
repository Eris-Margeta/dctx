// SPDX-License-Identifier: Apache-2.0
//! Known answers for the crate's SHA-256 API (there is no BLAKE API).
//! Empty, abc and decimal-million-a answers are the published SHA-256 examples:
//! https://csrc.nist.gov/projects/cryptographic-standards-and-guidelines/example-values
//! Repeated-a boundary and binary-MiB answers were independently calculated
//! with Python hashlib/OpenSSL and are pinned here, not computed by the crate.

use dctx_hash::{IDENTITY_VERSION, IdentityAlgorithm, Sha256};
use std::fmt::Write;

fn assert_vector(message: &[u8], expected: &str) {
    let digest = Sha256::digest(message).unwrap();
    let mut actual = String::new();
    for byte in digest.as_bytes() {
        write!(actual, "{byte:02x}").unwrap();
    }
    assert_eq!(actual, expected);
    assert_eq!(digest.byte_length(), u64::try_from(message.len()).unwrap());
    assert_eq!(digest.version(), IDENTITY_VERSION);
    assert_eq!(digest.algorithm(), IdentityAlgorithm::Sha256);
    assert_eq!(digest.algorithm_code(), IdentityAlgorithm::SHA256);
    for size in [1, 7, 55, 56, 63, 64, 65, 4096] {
        let mut state = Sha256::new();
        state.update(&[]).unwrap();
        for chunk in message.chunks(size) {
            state.update(chunk).unwrap();
            state.update(&[]).unwrap();
        }
        assert_eq!(state.finalize(), digest, "chunk size {size}");
    }
}

#[test]
fn sha256_empty() {
    assert_vector(
        b"",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
}

#[test]
fn sha256_abc() {
    assert_vector(
        b"abc",
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
}

#[test]
fn sha256_million_a() {
    assert_vector(
        &vec![b'a'; 1_000_000],
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
    );
}

#[test]
fn sha256_mib_a() {
    assert_vector(
        &vec![b'a'; 1_048_576],
        "9bc1b2a288b26af7257a36277ae3816a7d4f16e89c1e7e77d0a5c48bad62b360",
    );
}

#[test]
fn sha256_boundary_55() {
    assert_vector(
        &[b'a'; 55],
        "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318",
    );
}

#[test]
fn sha256_boundary_56() {
    assert_vector(
        &[b'a'; 56],
        "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a",
    );
}

#[test]
fn sha256_boundary_63() {
    assert_vector(
        &[b'a'; 63],
        "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34",
    );
}

#[test]
fn sha256_boundary_64() {
    assert_vector(
        &[b'a'; 64],
        "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb",
    );
}

#[test]
fn sha256_boundary_65() {
    assert_vector(
        &[b'a'; 65],
        "635361c48bb9eab14198e76ea8ab7f1a41685d6ad62aa9146d301d4f17eb0ae0",
    );
}

#[test]
fn every_split_matches_one_shot_with_nonuniform_bytes() {
    use sha2::Digest;

    let bytes: Vec<u8> = (0..=255).chain(0..=128).collect();
    for length in [0, 3, 55, 56, 63, 64, 65, 127, 128, 129, bytes.len()] {
        let message = &bytes[..length];
        let expected = Sha256::digest(message).unwrap();
        assert_eq!(
            expected.as_bytes().as_slice(),
            sha2::Sha256::digest(message).as_slice()
        );
        for split in 0..=length {
            let mut state = Sha256::new();
            state.update(&message[..split]).unwrap();
            state.update(&[]).unwrap();
            state.update(&message[split..]).unwrap();
            assert_eq!(state.finalize(), expected, "length {length}, split {split}");
        }
    }
}
