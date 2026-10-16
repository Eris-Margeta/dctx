//! Whitespace-normalized fingerprints for rename and move detection.
//! Implements SPEC-MAP-MODE section 5.2 using SHA-256.
//! Pure string transformation used by every parser adapter.
use sha2::{Digest, Sha256};

/// SHA-256 of the body with whitespace runs collapsed and edges trimmed.
pub fn body_fingerprint(body: &str) -> String {
    let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{:x}", Sha256::digest(normalized.as_bytes()))
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
