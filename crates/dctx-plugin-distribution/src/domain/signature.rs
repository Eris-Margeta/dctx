//! Domain-separated detached Ed25519 verification of exact document bytes.
//! Implements distribution.md §Authenticated bytes: `ASCII(domain) || exact_bytes`, no separator,
//! length prefix or prehash; 64 raw signature bytes. decision 081: verification only, via dctx-ed25519.
//! Pure; used by trust (manifests) and index (discovery). This crate never signs.

use dctx_ed25519::verify;

use crate::error::{DistributionError, Document};

/// Domain of publisher manifest signatures.
pub const MANIFEST_DOMAIN: &[u8] = b"dctx-plugin-manifest-v1";
/// Domain of discovery index signatures.
pub const INDEX_DOMAIN: &[u8] = b"dctx-plugin-index-v1";
/// Exact length of a detached signature file.
pub const SIGNATURE_BYTES: usize = 64;

/// Verifies `signature` over `domain || document` with `public_key`.
pub fn verify_detached(
    domain: &[u8],
    public_key: &[u8; 32],
    document: &[u8],
    signature: &[u8],
) -> Result<(), DistributionError> {
    if signature.len() != SIGNATURE_BYTES {
        return Err(DistributionError::malformed(Document::Signature, "signature-length"));
    }
    let mut message = Vec::with_capacity(domain.len() + document.len());
    message.extend_from_slice(domain);
    message.extend_from_slice(document);
    verify(public_key, &message, signature)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_lengths_and_garbage_are_refused() {
        let key = [0u8; 32];
        assert_eq!(
            verify_detached(MANIFEST_DOMAIN, &key, b"{}", &[0; 63]),
            Err(DistributionError::malformed(Document::Signature, "signature-length"))
        );
        assert!(matches!(
            verify_detached(MANIFEST_DOMAIN, &key, b"{}", &[0; 64]),
            Err(DistributionError::Signature(_))
        ));
    }

    #[test]
    fn domains_differ_and_are_not_prefixes() {
        assert_ne!(MANIFEST_DOMAIN, INDEX_DOMAIN);
        assert!(!MANIFEST_DOMAIN.starts_with(INDEX_DOMAIN) && !INDEX_DOMAIN.starts_with(MANIFEST_DOMAIN));
    }
}
