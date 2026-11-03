// SPDX-License-Identifier: Apache-2.0
//! Typed refusals for the public plugin index.
//! Implements PLUGINS.md §5.3 and §7.2 PL-X, plus distribution.md authenticated bytes.
//! Depends on thiserror. The domain, command, and CLI return these errors.
//! Unknown input is a refusal. This checker does not publish an index.

use thiserror::Error;

/// Failure of index validation, test-key verification, or the local command.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum IndexError {
    /// The document is larger than the 4 MiB page cap.
    #[error("index is larger than 4 MiB")]
    TooLarge,
    /// The bytes are not one strict JSON value.
    #[error("index JSON is malformed")]
    Malformed,
    /// An object repeats a key, including after escape decoding.
    #[error("duplicate JSON key")]
    DuplicateKey,
    /// Nesting of objects and arrays exceeds 32, counting the root as 1.
    #[error("index JSON is nested deeper than 32")]
    Depth,
    /// Object members or one array exceed the bounded page.
    #[error("index JSON exceeds the member limit")]
    TooManyMembers,
    /// A number is signed, fractional, exponential, or outside the safe range.
    #[error("integer is not a canonical decimal in range")]
    Integer,
    /// A required field is missing, the wrong type, or outside its pattern.
    #[error("index schema rejected {0}")]
    Schema(&'static str),
    /// Two entries use the same plugin id.
    #[error("duplicate plugin id")]
    DuplicateId,
    /// A plugin repeats a version or does not increase `release_sequence`.
    #[error("duplicate or reordered release")]
    DuplicateRelease,
    /// The page claims a command alias, a reserved name, or the same repository twice.
    #[error("alias claim")]
    AliasClaim,
    /// Metadata is outside the signed schema, or the index key did not authenticate it.
    #[error("unsigned metadata")]
    UnsignedMetadata,
    /// A known plugin id is listed under a different publisher fingerprint.
    #[error("publisher fingerprint changed")]
    ChangedPublisher,
    /// A plugin id has no local test pin.
    #[error("unknown publisher")]
    UnknownPublisher,
    /// A summary contains terminal, bidi, or zero-width characters.
    #[error("hostile display string")]
    HostileDisplay,
    /// The detached Ed25519 signature is the wrong length or fails the equation.
    #[error("index signature did not verify")]
    Signature,
    /// A manifest locator is not public HTTPS without userinfo.
    #[error("manifest URL origin is not an accepted public https locator")]
    Origin,
    /// A fixture tried to carry a private key.
    #[error("private key material is not accepted in index artifacts")]
    SecretMaterial,
    /// Publish and sign are not commands of this packet.
    #[error("repository publication and signing are outside this local checker")]
    PublicationRefused,
    /// The command line is not `validate` or `check`.
    #[error(
        "usage: plugin-index validate <index.json> | plugin-index check <index.json> <index.json.sig> <test-key> <publisher-pins.json>"
    )]
    Usage,
    /// The adapter could not read a local path.
    #[error("could not read {path}")]
    Io {
        /// Path the operator passed.
        path: String,
    },
}
