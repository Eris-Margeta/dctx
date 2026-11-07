// SPDX-License-Identifier: Apache-2.0
//! Six-hex stable-id alias digest.
//! Implements SPEC-MAP-MODE section 5.2 by calling `dctx-hash` (decision 077, decision 078).
//! Not content identity and not a git object id. [`super::ContentId`] owns identity.
//! [`super::StableId::alias`] is the only caller.

/// SHA-256 bytes of `input` for the short alias. The length is not part of the alias.
pub(crate) fn alias_digest(input: &[u8]) -> Result<[u8; 32], dctx_hash::HashError> {
    Ok(*dctx_hash::Sha256::digest(input)?.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    #[test]
    fn published_vectors_come_from_dctx_hash() {
        for (input, expected) in [
            (
                "",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                "abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
        ] {
            let actual: String = alias_digest(input.as_bytes()).unwrap().iter().fold(
                String::new(),
                |mut text, byte| {
                    write!(text, "{byte:02x}").unwrap();
                    text
                },
            );
            assert_eq!(actual, expected);
        }
    }
}
