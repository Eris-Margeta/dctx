// SPDX-License-Identifier: Apache-2.0
//! Whitespace-normalized fingerprints for rename and move detection.
//! Implements SPEC-MAP-MODE section 5.2.
//! Depends on dctx-hash SHA-256. Every parser adapter uses this fingerprint.

use dctx_hash::Sha256;

const HEX: &[u8; 16] = b"0123456789abcdef";

/// SHA-256 of the body with whitespace runs collapsed and edges trimmed.
/// Empty or whitespace-only input hashes the empty byte string. This helper
/// has no parser file-size limit; callers must bound their own input. It treats
/// whitespace textually, including whitespace inside source string literals.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn body_fingerprint(body: &str) -> String {
    let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hex = String::with_capacity(64);
    if let Ok(digest) = Sha256::digest(normalized.as_bytes()) {
        for byte in digest.as_bytes() {
            hex.push(char::from(HEX[usize::from(byte >> 4)]));
            hex.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_are_known_sha256_and_normalize_whitespace() {
        assert_eq!(
            body_fingerprint("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(body_fingerprint(" a\n\t b "), body_fingerprint("a b"));
        assert_ne!(body_fingerprint("a b"), body_fingerprint("a c"));
    }
}
