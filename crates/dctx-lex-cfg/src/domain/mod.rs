//! IO-free extraction vocabulary: facts, precision labels, limits and refusals.
//! Implements PARSERS-v1 (ParseResult, completion, limits) for configuration files.
//! Used by the JSON, TOML and YAML walkers and by callers of the public API.
mod facts;
mod limits;

pub use facts::{
    Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexedFile, Precision, RawReference,
};
pub use limits::{LexError, Limits};

/// SHA-256 hex of the body with whitespace runs collapsed and edges trimmed.
pub fn body_fingerprint(body: &str) -> String {
    if !has_whitespace(body) {
        return fingerprint_plain(body);
    }
    digest_normalized(body)
}

/// SHA-256 hex of `body` when the caller already knows it has no whitespace.
pub(crate) fn fingerprint_plain(body: &str) -> String {
    digest_hex(body.as_bytes())
}

/// True when [`str::split_whitespace`] would drop or collapse a character.
///
/// U+000B is whitespace for `split_whitespace` but not `u8::is_ascii_whitespace`.
pub(crate) fn has_whitespace(source: &str) -> bool {
    let mut unicode = false;
    for byte in source.bytes() {
        if byte.is_ascii_whitespace() || byte == 0x0B {
            return true;
        }
        if byte >= 0x80 {
            unicode = true;
        }
    }
    unicode && source.chars().any(char::is_whitespace)
}

fn digest_hex(bytes: &[u8]) -> String {
    crate::digest::sha256_hex(bytes)
}

fn digest_normalized(body: &str) -> String {
    crate::digest::sha256_hex_whitespace(body)
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
        assert_eq!(body_fingerprint("").len(), 64);
    }

    #[test]
    fn fingerprints_match_join_without_allocating_the_normalized_body() {
        fn oracle(body: &str) -> String {
            let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
            super::digest_hex(normalized.as_bytes())
        }
        let samples = [
            "",
            "abc",
            " a\n\t b ",
            " \t\n",
            "a  b\tc",
            "a\u{000b}b",
            "a\u{00a0}b",
            "café déjà",
            "line\r\nline",
            "   ",
            "a\u{3000}b",
            "no-whitespace-at-all",
        ];
        for sample in samples {
            assert_eq!(body_fingerprint(sample), oracle(sample), "{sample:?}");
            if !has_whitespace(sample) {
                assert_eq!(fingerprint_plain(sample), body_fingerprint(sample), "{sample:?}");
            }
        }
    }
}
