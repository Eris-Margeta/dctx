//! IO-free extraction vocabulary: facts, precision labels, limits and refusals.
//! Implements PARSERS-v1 (ParseResult, completion, limits) for PHP source.
//! Used by the lexer, the structural walker and callers of the public API.
mod facts;
mod limits;

pub use facts::{
    Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexedFile, Precision, RawReference,
};
pub use limits::{LexError, Limits};

use dctx_hash::Sha256;

/// SHA-256 hex of the body with whitespace runs collapsed and edges trimmed.
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

const HEX: &[u8; 16] = b"0123456789abcdef";

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
        assert_eq!(body_fingerprint("").len(), 64);
    }
}
