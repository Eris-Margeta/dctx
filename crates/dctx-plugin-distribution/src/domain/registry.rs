// SPDX-License-Identifier: Apache-2.0
//! Host-owned registry: installed immutable versions, the activation record, enablement and consent.
//! Implements docs/formal/dctx-plugins/Install.tla (`PP9Integrity`: active is complete, verified and allowed;
//! leases only on complete versions; activation never below the high-water mark unless an approved rollback)
//! and PLUGINS.md §5.1 enable/disable/remove semantics. Pure; depends on consent, trust and names.
//! Used by the install and management services, persisted through `registry_codec`.

use std::collections::BTreeMap;

use super::consent::{CapabilitySet, ConsentApproval, ConsentRecord};
use super::digest::{Fingerprint, Sha256Hex};
use super::names::{PluginId, Version};
use super::trust::TrustStore;
use crate::error::DistributionError;

/// Where the selected bytes come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Installed from a signed release or offline bundle.
    Installed,
    /// Shipped inside the containing dctx release.
    Bundled,
    /// Explicit standalone override of a bundled plugin.
    Override,
}

/// Persistent user choice. `Disabled` is sticky across installs, updates and bundled releases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enablement {
    /// Installed inactive, never enabled.
    NeverEnabled,
    /// Enabled by explicit consent or bundled preference.
    Enabled,
    /// Disabled or removed; only an explicit enable clears it.
    Disabled,
}

/// OS confinement state reported by the platform adapter (decision 080).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confinement {
    /// A qualified provider confines the worker.
    Established,
    /// No qualified provider on this OS.
    Unavailable,
}

/// One complete, published, immutable version directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledVersion {
    /// Exact version.
    pub version: Version,
    /// Signed release sequence.
    pub release_sequence: u64,
    /// SHA-256 of the executable for the host target.
    pub digest: Sha256Hex,
    /// SHA-256 of the exact manifest bytes.
    pub manifest_sha256: Sha256Hex,
    /// Signer fingerprint.
    pub signer: Fingerprint,
    /// Requested capabilities.
    pub capabilities: CapabilitySet,
    /// Failed reverification; never admitted.
    pub quarantined: bool,
}

/// An explicit rollback below the high-water mark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RollbackEntry {
    /// Exact digest rolled back to.
    pub digest: Sha256Hex,
    /// Caller-stated reason.
    pub reason: String,
}

/// All registry facts about one plugin identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginRecord {
    /// Selection origin.
    pub origin: Origin,
    /// Complete installed versions.
    pub versions: Vec<InstalledVersion>,
    /// Digest of the active version.
    pub active: Option<Sha256Hex>,
    /// Highest release sequence ever activated (Install.tla `highWater`).
    pub high_water: u64,
    /// Persistent enablement.
    pub enablement: Enablement,
    /// Current digest-bound consent.
    pub consent: Option<ConsentRecord>,
    /// Worker leases per digest.
    pub leases: BTreeMap<Sha256Hex, u32>,
    /// Explicit rollbacks, oldest first.
    pub rollbacks: Vec<RollbackEntry>,
}

impl PluginRecord {
    fn new(origin: Origin) -> Self {
        Self {
            origin,
            versions: Vec::new(),
            active: None,
            high_water: 0,
            enablement: Enablement::NeverEnabled,
            consent: None,
            leases: BTreeMap::new(),
            rollbacks: Vec::new(),
        }
    }

    /// Installed version by digest.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn version(&self, digest: &Sha256Hex) -> Option<&InstalledVersion> {
        self.versions.iter().find(|version| version.digest == *digest)
    }

    /// The active installed version.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn active_version(&self) -> Option<&InstalledVersion> {
        self.active.as_ref().and_then(|digest| self.version(digest))
    }

    fn leased(&self, digest: &Sha256Hex) -> bool {
        self.leases.get(digest).is_some_and(|count| *count > 0)
    }

    fn usable<'a>(&'a self, digest: &Sha256Hex, trust: &TrustStore) -> Result<&'a InstalledVersion, DistributionError> {
        let version = self.version(digest).ok_or(DistributionError::NotInstalled)?;
        if version.quarantined {
            return Err(DistributionError::Quarantined);
        }
        if trust.is_artifact_revoked(digest) {
            return Err(DistributionError::ArtifactRevoked);
        }
        if trust.is_key_revoked(&version.signer) {
            return Err(DistributionError::KeyRevoked);
        }
        Ok(version)
    }

    fn rebind_consent(&mut self, version: &InstalledVersion) -> Result<(), DistributionError> {
        if self.enablement != Enablement::Enabled {
            return Ok(());
        }
        let derived = self
            .consent
            .as_ref()
            .and_then(|consent| consent.derive(version.digest, &version.signer, &version.capabilities));
        self.consent = Some(derived.ok_or(DistributionError::ConsentRequired)?);
        Ok(())
    }
}

/// The whole registry. Format version 1.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registry {
    pub(crate) plugins: BTreeMap<PluginId, PluginRecord>,
}

impl Registry {
    /// Record by ID.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn get(&self, id: &PluginId) -> Option<&PluginRecord> {
        self.plugins.get(id)
    }

    /// All records in ID order.
    pub fn records(&self) -> impl Iterator<Item = (&PluginId, &PluginRecord)> {
        self.plugins.iter()
    }

    fn record_mut(&mut self, id: &PluginId) -> Result<&mut PluginRecord, DistributionError> {
        self.plugins.get_mut(id).ok_or(DistributionError::NotInstalled)
    }

    /// Adds a published version inactive; refuses downgrade and sequence replay. Idempotent per digest.
    /// # Errors
    /// Refuses release sequence downgrades or replayed sequences with a different artifact.
    pub fn record_install(&mut self, id: &PluginId, origin: Origin, version: InstalledVersion) -> Result<(), DistributionError> {
        let record = self.plugins.entry(id.clone()).or_insert_with(|| PluginRecord::new(origin));
        if record.version(&version.digest).is_some() {
            return Ok(());
        }
        if version.release_sequence < record.high_water {
            return Err(DistributionError::Downgrade { candidate: version.release_sequence, floor: record.high_water });
        }
        if record.versions.iter().any(|known| known.release_sequence == version.release_sequence) {
            return Err(DistributionError::Replay { sequence: version.release_sequence });
        }
        if origin == Origin::Override || record.versions.is_empty() {
            record.origin = origin;
        }
        record.versions.push(version);
        Ok(())
    }

    /// Atomically selects `digest` as active; consent of an enabled plugin must cover it.
    /// # Errors
    /// Refuses missing, quarantined, untrusted/revoked or downgraded versions, or incompatible existing consent.
    pub fn activate(&mut self, id: &PluginId, digest: &Sha256Hex, trust: &TrustStore) -> Result<(), DistributionError> {
        let record = self.record_mut(id)?;
        let version = record.usable(digest, trust)?.clone();
        if version.release_sequence < record.high_water {
            return Err(DistributionError::Downgrade { candidate: version.release_sequence, floor: record.high_water });
        }
        record.rebind_consent(&version)?;
        record.active = Some(version.digest);
        record.high_water = version.release_sequence;
        Ok(())
    }

    /// Explicit exact-digest rollback with a reason; cannot revive revoked or quarantined versions.
    /// # Errors
    /// Refuses missing or unusable versions, an empty rollback reason, or incompatible existing consent.
    pub fn rollback(&mut self, id: &PluginId, digest: &Sha256Hex, reason: &str, trust: &TrustStore) -> Result<(), DistributionError> {
        let record = self.record_mut(id)?;
        let version = record.usable(digest, trust)?.clone();
        if reason.trim().is_empty() {
            return Err(DistributionError::malformed(crate::error::Document::Registry, "rollback-reason"));
        }
        record.rebind_consent(&version)?;
        record.active = Some(version.digest);
        record.high_water = record.high_water.max(version.release_sequence);
        record.rollbacks.push(RollbackEntry { digest: version.digest, reason: reason.to_owned() });
        Ok(())
    }

    /// Enables the active version with existing or newly approved consent.
    /// # Errors
    /// Refuses missing/inactive/unusable versions, missing confinement, or absent/mismatched consent approval.
    pub fn enable(
        &mut self,
        id: &PluginId,
        approval: Option<&ConsentApproval>,
        confinement: Confinement,
        trust: &TrustStore,
    ) -> Result<(), DistributionError> {
        let record = self.record_mut(id)?;
        let digest = record.active.ok_or(DistributionError::NotActive)?;
        let version = record.usable(&digest, trust)?.clone();
        if record.origin != Origin::Bundled && confinement != Confinement::Established {
            return Err(DistributionError::ConfinementRequired);
        }
        let existing = record.consent.as_ref().and_then(|c| c.derive(digest, &version.signer, &version.capabilities));
        let consent = match (existing, approval) {
            (Some(consent), _) => consent,
            (None, Some(approval)) if approval.digest == digest && version.capabilities.is_within(&approval.capabilities) => {
                ConsentRecord {
                    digest,
                    signer: version.signer,
                    capabilities: approval.capabilities.clone(),
                    scope: approval.scope.clone(),
                }
            }
            _ => return Err(DistributionError::ConsentRequired),
        };
        record.consent = Some(consent);
        record.enablement = Enablement::Enabled;
        Ok(())
    }

    /// Persists disable and returns the number of worker leases the caller must cancel and reap.
    /// # Errors
    /// Returns `NotInstalled` for an unknown plugin ID.
    pub fn disable(&mut self, id: &PluginId) -> Result<u32, DistributionError> {
        let record = self.record_mut(id)?;
        record.enablement = Enablement::Disabled;
        Ok(record.leases.values().sum())
    }

    /// Marks a version as failed reverification.
    /// # Errors
    /// Returns `NotInstalled` for an unknown plugin ID or artifact digest.
    pub fn quarantine(&mut self, id: &PluginId, digest: &Sha256Hex) -> Result<(), DistributionError> {
        let record = self.record_mut(id)?;
        let version = record.versions.iter_mut().find(|v| v.digest == *digest).ok_or(DistributionError::NotInstalled)?;
        version.quarantined = true;
        Ok(())
    }

    /// Unlinks an inactive, unleased version.
    /// # Errors
    /// Refuses an unknown plugin/version or a version with active leases.
    pub fn remove_version(&mut self, id: &PluginId, digest: &Sha256Hex) -> Result<(), DistributionError> {
        let record = self.record_mut(id)?;
        if record.active == Some(*digest) || record.leased(digest) {
            return Err(DistributionError::Leased);
        }
        let before = record.versions.len();
        record.versions.retain(|version| version.digest != *digest);
        if record.versions.len() == before { Err(DistributionError::NotInstalled) } else { Ok(()) }
    }

    /// Removes all versions after leases end; keeps a sticky disabled mask record.
    /// # Errors
    /// Refuses an unknown plugin or a plugin with active leases.
    pub fn remove_plugin(&mut self, id: &PluginId) -> Result<Vec<Sha256Hex>, DistributionError> {
        let record = self.record_mut(id)?;
        record.enablement = Enablement::Disabled;
        if record.leases.values().any(|count| *count > 0) {
            return Err(DistributionError::Leased);
        }
        record.active = None;
        record.consent = None;
        Ok(record.versions.drain(..).map(|version| version.digest).collect())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn version(sequence: u64, seed: u8) -> InstalledVersion {
        InstalledVersion {
            version: Version::parse(&format!("{sequence}.0.0")).unwrap(),
            release_sequence: sequence,
            digest: Sha256Hex::from_bytes([seed; 32]),
            manifest_sha256: Sha256Hex::from_bytes([seed.wrapping_add(100); 32]),
            signer: Sha256Hex::from_bytes([1; 32]),
            capabilities: CapabilitySet::default(),
            quarantined: false,
        }
    }

    #[test]
    fn install_refuses_downgrade_and_replay_but_is_idempotent() {
        let id = PluginId::parse("dctx/git").unwrap();
        let trust = TrustStore::new();
        let mut registry = Registry::default();
        registry.record_install(&id, Origin::Installed, version(2, 2)).unwrap();
        registry.record_install(&id, Origin::Installed, version(2, 2)).unwrap();
        assert_eq!(registry.record_install(&id, Origin::Installed, version(2, 9)), Err(DistributionError::Replay { sequence: 2 }));
        registry.activate(&id, &Sha256Hex::from_bytes([2; 32]), &trust).unwrap();
        assert_eq!(
            registry.record_install(&id, Origin::Installed, version(1, 1)),
            Err(DistributionError::Downgrade { candidate: 1, floor: 2 })
        );
    }

    #[test]
    fn rollback_never_leaves_active_above_the_high_water_mark() {
        let id = PluginId::parse("dctx/git").unwrap();
        let trust = TrustStore::new();
        let mut registry = Registry::default();
        registry.record_install(&id, Origin::Installed, version(1, 1)).unwrap();
        registry.rollback(&id, &Sha256Hex::from_bytes([1; 32]), "first selection", &trust).unwrap();
        let record = registry.get(&id).unwrap();
        assert_eq!((record.high_water, record.rollbacks.len()), (1, 1));
    }

    #[test]
    fn quarantined_versions_cannot_be_activated_or_removed_while_active() {
        let id = PluginId::parse("dctx/git").unwrap();
        let trust = TrustStore::new();
        let digest = Sha256Hex::from_bytes([1; 32]);
        let mut registry = Registry::default();
        registry.record_install(&id, Origin::Installed, version(1, 1)).unwrap();
        registry.activate(&id, &digest, &trust).unwrap();
        assert_eq!(registry.remove_version(&id, &digest), Err(DistributionError::Leased));
        registry.quarantine(&id, &digest).unwrap();
        assert_eq!(registry.activate(&id, &digest, &trust), Err(DistributionError::Quarantined));
    }
}
