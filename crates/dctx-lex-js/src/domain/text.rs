//! Pure text rules shared with dctx-map-parse: summaries, `JSDoc` and fingerprints.
//! Implements SPEC-MAP-MODE 5.2 (fingerprints) and 5.4 (tier-zero summaries).
//! Uses dctx-hash SHA-256; the walker and assembly call these.
use dctx_hash::Sha256;

/// SHA-256 hex of the body with whitespace runs collapsed and edges trimmed.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
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

/// Removes all whitespace, as reference spellings do.
pub(crate) fn compact(value: &str) -> String {
    value.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Whitespace-normalized text up to and including the first `". "`.
pub(crate) fn sentence(value: &str) -> String {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    match value.find(". ") {
        Some(end) => value[..=end].into(),
        None => value,
    }
}

/// First sentence of a line that starts with a comment marker.
pub(crate) fn first_comment(line: &str) -> Option<String> {
    let line = line.trim();
    ["//!", "///", "//", "#", "<!--", "/*", "--", ";"]
        .iter()
        .find_map(|prefix| line.strip_prefix(prefix))
        .map(|body| sentence(body.trim_end_matches("-->").trim_end_matches("*/")))
}

/// First description line of a `/** */` comment, as one sentence.
pub(crate) fn jsdoc(comment: &str) -> Option<String> {
    let body = comment.strip_prefix("/**")?.strip_suffix("*/")?;
    body.lines()
        .map(|line| line.trim().trim_start_matches('*').trim())
        .find(|line| !line.is_empty() && !line.starts_with('@'))
        .map(sentence)
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

    #[test]
    fn summaries_follow_the_first_sentence_rules() {
        assert_eq!(first_comment("  // First. Second."), Some("First.".into()));
        assert_eq!(first_comment("code();"), None);
        assert_eq!(jsdoc("/* plain */"), None);
        assert_eq!(jsdoc("/**/"), None);
        assert_eq!(
            jsdoc("/**\n * @param x\n * Does it. More.\n */"),
            Some("Does it.".into())
        );
        assert_eq!(compact("a .\n b"), "a.b");
    }
}
