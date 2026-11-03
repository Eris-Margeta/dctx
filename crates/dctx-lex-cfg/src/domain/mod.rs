// SPDX-License-Identifier: Apache-2.0
//! IO-free extraction vocabulary: facts, precision labels, limits and refusals.
//! Implements PARSERS-v1 (`ParseResult`, completion, limits) for configuration files.
//! Used by the JSON, TOML and YAML walkers and by callers of the public API.
//! Body fingerprints stream through `dctx_hash::Sha256` over whitespace-skipped runs.
mod facts;
mod limits;

pub use facts::{
    Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexedFile, Precision, RawReference,
};
pub use limits::{LexError, Limits};

/// SHA-256 hex of the body with whitespace runs collapsed and edges trimmed.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
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
    let mut hasher = dctx_hash::Sha256::new();
    // A file cannot reach the SHA-256 length cap. On refusal the hasher is unchanged.
    let _ = hasher.update(bytes);
    hex_digest(hasher.finalize().as_bytes())
}

/// SHA-256 of `split_whitespace` words joined by one space, without building that buffer.
fn digest_normalized(body: &str) -> String {
    let mut hasher = dctx_hash::Sha256::new();
    let mut separate = false;
    for word in body.split_whitespace() {
        if separate && hasher.update(b" ").is_err() {
            break;
        }
        if hasher.update(word.as_bytes()).is_err() {
            break;
        }
        separate = true;
    }
    hex_digest(hasher.finalize().as_bytes())
}

fn hex_digest(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(64);
    for byte in bytes {
        hex.push(char::from(HEX[usize::from(byte >> 4)]));
        hex.push(char::from(HEX[usize::from(byte & 15)]));
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

    #[test]
    fn digests_match_dctx_hash_across_block_edges() {
        let samples: [&[u8]; 8] = [
            b"",
            b"a",
            b"abc",
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            &[0u8; 55],
            &[0u8; 56],
            &[0u8; 63],
            &[0u8; 64],
        ];
        for sample in samples {
            let digest = dctx_hash::Sha256::digest(sample).expect("length");
            let hex = super::hex_digest(digest.as_bytes());
            assert_eq!(super::digest_hex(sample), hex, "len {}", sample.len());
        }
        assert_eq!(
            body_fingerprint("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            body_fingerprint(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
