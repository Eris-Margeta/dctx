// SPDX-License-Identifier: Apache-2.0
//! Typed refusals of the installer, trust and discovery client.
//! Implements the "typed errors, nonzero status" rule of PLUGINS.md §5.1 and distribution.md.
//! Depends on dctx-ed25519 for the verifier's reason; used by every domain module and service.
//! A refusal carries static names and numbers only, never document bytes or untrusted text.

use std::fmt;

use dctx_ed25519::VerificationError;

/// Document whose structure was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Document {
    /// `plugin.json`.
    Manifest,
    /// `plugin.json.sig` or `index.json.sig`.
    Signature,
    /// `SHA256SUMS`.
    Sums,
    /// The discovery index.
    Index,
    /// A `.dctx-plugin` offline bundle.
    Bundle,
    /// The host-owned registry record.
    Registry,
    /// A URL or locator.
    Locator,
}

/// Reason a URL, redirect or connection was refused (I-ORIGIN).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginRefusal {
    /// Not an `https://` URL.
    Scheme,
    /// The authority carries `user@` information.
    Userinfo,
    /// The URL has a `#fragment`.
    Fragment,
    /// Malformed host, port, path or characters.
    Syntax,
    /// Loopback, private, link-local or unspecified destination.
    PrivateDestination,
    /// A redirect left the selected origin for an unapproved host.
    NotApproved,
    /// More redirects than the policy allows.
    TooManyRedirects,
}

/// Failure reported by the network port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkError {
    /// No connection could be made.
    Unreachable,
    /// The server refused for rate limiting; no credentials are tried.
    RateLimited,
    /// Any other non-success HTTP status.
    Status(u16),
    /// The resource does not exist.
    NotFound,
    /// The stream ended or failed before the declared length.
    Interrupted,
}

/// Every refusal of this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistributionError {
    /// A document failed decoding or its closed schema.
    Malformed {
        /// Refused document.
        document: Document,
        /// Static rule name.
        reason: &'static str,
    },
    /// The detached Ed25519 signature did not verify.
    Signature(VerificationError),
    /// `publisher_key` names no trusted key record.
    UnknownKey,
    /// An independently supplied fingerprint does not match the key bytes.
    FingerprintMismatch,
    /// The signing key has been revoked.
    KeyRevoked,
    /// No publisher pin exists for this plugin ID; independent approval is required.
    UnknownPublisher,
    /// The signer differs from the plugin's pinned publisher key.
    SignerChanged,
    /// The manifest's ID differs from the selected or approved identity.
    IdentityMismatch,
    /// The release artifact digest has been revoked.
    ArtifactRevoked,
    /// The release sequence is below the accepted high-water mark.
    Downgrade {
        /// Candidate release sequence.
        candidate: u64,
        /// Accepted high-water sequence.
        floor: u64,
    },
    /// A different release reuses an already accepted sequence number.
    Replay {
        /// Reused sequence.
        sequence: u64,
    },
    /// The manifest has no artifact for the host target.
    UnsupportedTarget,
    /// Supplied payloads do not correspond to the selected target.
    TargetMismatch,
    /// Bytes received differ from the signed length.
    SizeMismatch {
        /// Signed length.
        expected: u64,
    },
    /// Bytes received differ from the signed SHA-256.
    IntegrityFailure,
    /// `SHA256SUMS` disagrees with the signed manifest.
    SumsMismatch,
    /// The manifest bytes differ from the digest named by the index.
    ManifestDigestMismatch,
    /// A size or count ceiling was exceeded.
    LimitExceeded {
        /// The ceiling.
        limit: u64,
    },
    /// A URL or connection violated the origin policy.
    Origin(OriginRefusal),
    /// The network port failed.
    Network(NetworkError),
    /// Another install or activation holds the operation lock.
    Busy,
    /// The store failed or was interrupted mid-operation.
    Interrupted,
    /// The plugin or version is not installed.
    NotInstalled,
    /// No version is active.
    NotActive,
    /// The version failed reverification and is quarantined.
    Quarantined,
    /// The plugin is persistently disabled.
    Disabled,
    /// The plan needs new or expanded capability consent.
    ConsentRequired,
    /// Third-party enablement needs established OS confinement (decision 080).
    ConfinementRequired,
    /// Last known revocation state is older than the freshness ceiling.
    StaleRevocation {
        /// Age of the last refresh, if one ever happened.
        age_secs: Option<u64>,
    },
    /// Active workers still lease this version.
    Leased,
    /// The discovery index has no matching plugin or release.
    NotFound,
}

impl DistributionError {
    /// Shorthand for [`DistributionError::Malformed`].
    pub(crate) const fn malformed(document: Document, reason: &'static str) -> Self {
        Self::Malformed { document, reason }
    }
}

impl From<VerificationError> for DistributionError {
    fn from(error: VerificationError) -> Self {
        Self::Signature(error)
    }
}

impl From<NetworkError> for DistributionError {
    fn from(error: NetworkError) -> Self {
        Self::Network(error)
    }
}

impl From<OriginRefusal> for DistributionError {
    fn from(error: OriginRefusal) -> Self {
        Self::Origin(error)
    }
}

impl fmt::Display for DistributionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed { document, reason } => write!(f, "malformed {document:?}: {reason}"),
            Self::Signature(error) => write!(f, "signature refused: {error}"),
            Self::Downgrade { candidate, floor } => {
                write!(f, "release sequence {candidate} is below accepted {floor}")
            }
            Self::Replay { sequence } => write!(
                f,
                "release sequence {sequence} already names another release"
            ),
            Self::SizeMismatch { expected } => {
                write!(f, "payload size differs from signed {expected} bytes")
            }
            Self::LimitExceeded { limit } => write!(f, "limit of {limit} exceeded"),
            Self::Origin(reason) => write!(f, "origin refused: {reason:?}"),
            Self::Network(reason) => write!(f, "network failure: {reason:?}"),
            Self::StaleRevocation {
                age_secs: Some(age),
            } => {
                write!(
                    f,
                    "revocation state is {age} s old, beyond the freshness ceiling"
                )
            }
            Self::StaleRevocation { age_secs: None } => {
                f.write_str("revocation state was never refreshed")
            }
            other => write!(f, "{other:?}"),
        }
    }
}

impl std::error::Error for DistributionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_never_needs_document_bytes() {
        let error = DistributionError::malformed(Document::Manifest, "unknown-field");
        assert_eq!(error.to_string(), "malformed Manifest: unknown-field");
        assert_eq!(
            DistributionError::Downgrade {
                candidate: 1,
                floor: 2
            }
            .to_string(),
            "release sequence 1 is below accepted 2"
        );
        assert_eq!(DistributionError::Busy.to_string(), "Busy");
    }
}
