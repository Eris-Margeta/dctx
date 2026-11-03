// SPDX-License-Identifier: Apache-2.0
//! Host-managed trust: pinned/approved keys, publisher pins, revocation state and freshness.
//! Implements PLUGINS.md §5.2 trust paragraph and requirements I-TRUST/I-IDENTITY/I-SIGNATURE:
//! a key downloaded beside its signature is never trust; unknown publishers need independent approval;
//! renamed locators cannot replace a pinned signer; offline status is last-known state plus age.
//! Pure; depends on manifest, signature and digest. Used by the install and management services.
//! Rotation is explicit reapproval through [`TrustStore::pin_publisher`]; no trust-bundle format is invented.

use std::collections::{BTreeMap, BTreeSet};

use super::digest::{Fingerprint, Sha256Hex, fingerprint};
use super::manifest::Manifest;
use super::names::{KeyId, PluginId};
use super::signature::{MANIFEST_DOMAIN, verify_detached};
use crate::error::DistributionError;

/// Why a key is trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustSource {
    /// Shipped with the official distribution.
    Pinned,
    /// Approved by the user from an independently verified fingerprint.
    Approved,
    /// Supplied by an organization trust bundle.
    Organization,
}

/// A trusted Ed25519 public key under a local trust-record ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedKey {
    /// Local ID a manifest's `publisher_key` names.
    pub key_id: KeyId,
    /// Exact 32 public key bytes.
    pub public_key: [u8; 32],
    /// SHA-256 of `public_key`.
    pub fingerprint: Fingerprint,
    /// Origin of the trust.
    pub source: TrustSource,
}

/// A manifest whose signature, signer trust and identity all passed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedManifest {
    /// The authenticated manifest.
    pub manifest: Manifest,
    /// Fingerprint of the key that signed it.
    pub signer: Fingerprint,
}

/// Trust state supplied and persisted by the host, never by a download.
#[derive(Clone, Debug, Default)]
pub struct TrustStore {
    keys: Vec<TrustedKey>,
    pins: BTreeMap<PluginId, Fingerprint>,
    revoked_keys: BTreeSet<Fingerprint>,
    revoked_artifacts: BTreeSet<Sha256Hex>,
    revocation_checked_at: Option<u64>,
}

impl TrustStore {
    /// An empty store: nothing is trusted.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an official key shipped with the distribution.
    /// # Errors
    /// Refuses a key ID already bound to a different key.
    pub fn pin_official(&mut self, key_id: KeyId, public_key: [u8; 32]) -> Result<Fingerprint, DistributionError> {
        let fingerprint = fingerprint(&public_key);
        self.add(TrustedKey { key_id, public_key, fingerprint, source: TrustSource::Pinned })?;
        Ok(fingerprint)
    }

    /// Adds a key only when its bytes hash to the independently supplied fingerprint.
    /// # Errors
    /// Refuses a fingerprint mismatch or a key ID already bound to a different key.
    pub fn approve_key(
        &mut self,
        key_id: KeyId,
        public_key: [u8; 32],
        independent_fingerprint: &Fingerprint,
        source: TrustSource,
    ) -> Result<(), DistributionError> {
        let fingerprint = fingerprint(&public_key);
        if fingerprint != *independent_fingerprint {
            return Err(DistributionError::FingerprintMismatch);
        }
        self.add(TrustedKey { key_id, public_key, fingerprint, source })
    }

    fn add(&mut self, key: TrustedKey) -> Result<(), DistributionError> {
        match self.keys.iter().find(|known| known.key_id == key.key_id) {
            Some(known) if known.public_key == key.public_key => Ok(()),
            Some(_) => Err(DistributionError::SignerChanged),
            None => {
                self.keys.push(key);
                Ok(())
            }
        }
    }

    /// Records the explicitly approved signer of a plugin identity; replacing a pin is reapproval.
    pub fn pin_publisher(&mut self, id: PluginId, signer: Fingerprint) {
        self.pins.insert(id, signer);
    }

    /// Pinned signer of `id`, if any.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn publisher_pin(&self, id: &PluginId) -> Option<&Fingerprint> {
        self.pins.get(id)
    }

    /// Applies revocations from an authenticated refresh at `checked_at` (Unix seconds).
    pub fn record_revocations(
        &mut self,
        keys: impl IntoIterator<Item = Fingerprint>,
        artifacts: impl IntoIterator<Item = Sha256Hex>,
        checked_at: u64,
    ) {
        self.revoked_keys.extend(keys);
        self.revoked_artifacts.extend(artifacts);
        self.revocation_checked_at = Some(self.revocation_checked_at.map_or(checked_at, |at| at.max(checked_at)));
    }

    /// True when the artifact digest is revoked.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn is_artifact_revoked(&self, digest: &Sha256Hex) -> bool {
        self.revoked_artifacts.contains(digest)
    }

    /// True when the signing key is revoked.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn is_key_revoked(&self, signer: &Fingerprint) -> bool {
        self.revoked_keys.contains(signer)
    }

    /// Age in seconds of the last known revocation state, `None` if never refreshed.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn revocation_age(&self, now: u64) -> Option<u64> {
        self.revocation_checked_at.map(|at| now.saturating_sub(at))
    }

    /// Refuses when a freshness ceiling is set and the last known state is older (or missing).
    /// # Errors
    /// Refuses missing or older-than-ceiling revocation data when a freshness ceiling is configured.
    pub fn check_freshness(&self, now: u64, ceiling_secs: Option<u64>) -> Result<(), DistributionError> {
        let Some(ceiling) = ceiling_secs else { return Ok(()) };
        match self.revocation_age(now) {
            Some(age) if age <= ceiling => Ok(()),
            age_secs => Err(DistributionError::StaleRevocation { age_secs }),
        }
    }

    /// Authenticates exact manifest bytes for the selected identity `expected`.
    ///
    /// Order: schema, trusted key, key revocation, signature, identity, publisher pin.
    /// # Errors
    /// Refuses malformed manifests, unknown/revoked keys, invalid signatures, identity mismatches, or missing/conflicting publisher pins.
    pub fn authenticate(
        &self,
        manifest_bytes: &[u8],
        signature: &[u8],
        expected: &PluginId,
    ) -> Result<VerifiedManifest, DistributionError> {
        let manifest = Manifest::parse(manifest_bytes)?;
        let key = self
            .keys
            .iter()
            .find(|key| key.key_id == manifest.publisher_key)
            .ok_or(DistributionError::UnknownKey)?;
        if self.is_key_revoked(&key.fingerprint) {
            return Err(DistributionError::KeyRevoked);
        }
        verify_detached(MANIFEST_DOMAIN, &key.public_key, manifest.bytes(), signature)?;
        if manifest.id != *expected {
            return Err(DistributionError::IdentityMismatch);
        }
        match self.pins.get(&manifest.id) {
            None => Err(DistributionError::UnknownPublisher),
            Some(pin) if *pin != key.fingerprint => Err(DistributionError::SignerChanged),
            Some(_) => Ok(VerifiedManifest { manifest, signer: key.fingerprint }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [
        0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a, 0x0e, 0xe1,
        0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
    ];
    const GIT: &[u8] = include_bytes!("../../../../docs/design/plugin-api/fixtures/valid/manifest-git.json");

    #[test]
    fn approval_requires_the_independent_fingerprint() {
        let mut trust = TrustStore::new();
        let id = KeyId::parse("k").unwrap();
        assert_eq!(
            trust.approve_key(id.clone(), KEY, &Sha256Hex::from_bytes([0; 32]), TrustSource::Approved),
            Err(DistributionError::FingerprintMismatch)
        );
        assert!(trust.approve_key(id.clone(), KEY, &fingerprint(&KEY), TrustSource::Approved).is_ok());
        assert_eq!(trust.pin_official(id, [9; 32]), Err(DistributionError::SignerChanged));
    }

    #[test]
    fn freshness_reports_age_and_respects_the_ceiling() {
        let mut trust = TrustStore::new();
        assert!(trust.check_freshness(100, None).is_ok());
        assert_eq!(trust.check_freshness(100, Some(10)), Err(DistributionError::StaleRevocation { age_secs: None }));
        trust.record_revocations([], [], 50);
        assert_eq!(trust.revocation_age(55), Some(5));
        assert!(trust.check_freshness(60, Some(10)).is_ok());
        assert_eq!(trust.check_freshness(61, Some(10)), Err(DistributionError::StaleRevocation { age_secs: Some(11) }));
        trust.record_revocations([], [], 40);
        assert_eq!(trust.revocation_age(55), Some(5), "an older refresh never rewinds freshness");
    }

    #[test]
    fn unknown_key_is_refused_before_signature_work() {
        let trust = TrustStore::new();
        let id = PluginId::parse("dctx/git").unwrap();
        assert_eq!(trust.authenticate(GIT, &[0; 64], &id).map(|_| ()), Err(DistributionError::UnknownKey));
    }

    #[test]
    fn revoked_key_is_refused() {
        let mut trust = TrustStore::new();
        let signer = trust.pin_official(KeyId::parse("fixture-key-1").unwrap(), KEY).unwrap();
        trust.record_revocations([signer], [], 1);
        let id = PluginId::parse("dctx/git").unwrap();
        assert_eq!(trust.authenticate(GIT, &[0; 64], &id).map(|_| ()), Err(DistributionError::KeyRevoked));
    }
}
