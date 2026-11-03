// SPDX-License-Identifier: Apache-2.0
//! Shared acceptance oracle for the PL-ED integration tests.
//! Uses ed25519-dalek 2.2.0 `VerifyingKey::verify_strict`.
//! Length and key-decoding errors count as rejection, never as skipped cases.

#![forbid(unsafe_code)]

pub fn reference_accepts(key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    let Ok(key_bytes) = <&[u8; 32]>::try_from(key) else {
        return false;
    };
    let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(key_bytes) else {
        return false;
    };
    let Ok(signature) = ed25519_dalek::Signature::from_slice(signature) else {
        return false;
    };
    key.verify_strict(message, &signature).is_ok()
}
