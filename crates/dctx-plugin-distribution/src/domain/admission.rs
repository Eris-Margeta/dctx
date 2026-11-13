// SPDX-License-Identifier: Apache-2.0
//! Launch admission, worker leases, bundled registration and visible status.
//! Implements Install.tla `Revalidate`/`Launch`/`EndWorker` preconditions at the registry level,
//! PLUGINS.md §5.1 list columns and §5.4 bundled/override/sticky-mask rules (I-BUNDLED, I-OFFLINE).
//! Pure; depends on registry and trust. Used by the management service; launch itself belongs to PL-A.

use super::digest::Sha256Hex;
use super::names::{PluginId, Version};
use super::registry::{Enablement, InstalledVersion, Origin, PluginRecord, Registry};
use super::trust::TrustStore;
use crate::error::DistributionError;

/// Permission to start one worker on the exact leased digest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admission {
    /// Plugin identity.
    pub id: PluginId,
    /// Exact leased executable digest.
    pub digest: Sha256Hex,
    /// Its version.
    pub version: Version,
}

/// The `plugin list` view of one record. Never requires running the plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginStatus {
    /// Plugin identity.
    pub id: PluginId,
    /// Selection origin.
    pub origin: Origin,
    /// Active version, if any.
    pub version: Option<Version>,
    /// Persistent enablement.
    pub enablement: Enablement,
    /// True when the active version is not covered by recorded consent.
    pub needs_consent: bool,
    /// True when the active version is quarantined or revoked.
    pub blocked: bool,
    /// Age of the last known revocation state, never a claim of current freshness.
    pub revocation_age_secs: Option<u64>,
}

impl Registry {
    /// Admits one worker of the active version and takes a lease on its digest.
    /// Each successful call takes another lease. A count of `u32::MAX` refuses further
    /// workers without changing the registry; release finished worker leases to continue.
    /// # Errors
    /// Refuses missing/disabled/inactive plugins, required consent, untrusted or revoked versions, or stale revocation data.
    pub fn admit(
        &mut self,
        id: &PluginId,
        trust: &TrustStore,
        now: u64,
        freshness_ceiling: Option<u64>,
    ) -> Result<Admission, DistributionError> {
        let record = self
            .plugins
            .get_mut(id)
            .ok_or(DistributionError::NotInstalled)?;
        match record.enablement {
            Enablement::Enabled => {}
            Enablement::Disabled => return Err(DistributionError::Disabled),
            Enablement::NeverEnabled => return Err(DistributionError::ConsentRequired),
        }
        let digest = record.active.ok_or(DistributionError::NotActive)?;
        let version = admissible(record, &digest, trust)?;
        trust.check_freshness(now, freshness_ceiling)?;
        let version = version.version.clone();
        let count = record.leases.entry(digest).or_insert(0);
        *count = count
            .checked_add(1)
            .ok_or(DistributionError::LimitExceeded {
                limit: u64::from(u32::MAX),
            })?;
        Ok(Admission {
            id: id.clone(),
            digest,
            version,
        })
    }

    /// Ends one worker lease.
    pub fn release(&mut self, admission: &Admission) {
        if let Some(record) = self.plugins.get_mut(&admission.id) {
            if let Some(count) = record.leases.get_mut(&admission.digest) {
                *count = count.saturating_sub(1);
            }
            record.leases.retain(|_, count| *count > 0);
        }
    }

    /// Registers the bundled build authenticated by the containing release. A sticky mask survives.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "Preserve the public value-taking registration contract for an installed version, including the already-recorded case."
    )]
    pub fn register_bundled(&mut self, id: &PluginId, version: InstalledVersion) {
        let record = self
            .plugins
            .entry(id.clone())
            .or_insert_with(|| PluginRecord {
                origin: Origin::Bundled,
                versions: Vec::new(),
                active: None,
                high_water: 0,
                enablement: Enablement::Enabled,
                consent: None,
                leases: std::collections::BTreeMap::default(),
                rollbacks: Vec::new(),
            });
        if record.version(&version.digest).is_none() {
            record.versions.push(version.clone());
        }
        if record.origin == Origin::Bundled && record.enablement != Enablement::Disabled {
            record.active = Some(version.digest);
            record.high_water = record.high_water.max(version.release_sequence);
        }
    }

    /// `plugin list` row for `id`.
    /// # Errors
    /// Returns `NotInstalled` for an unknown plugin ID.
    pub fn status(
        &self,
        id: &PluginId,
        trust: &TrustStore,
        now: u64,
    ) -> Result<PluginStatus, DistributionError> {
        let record = self
            .plugins
            .get(id)
            .ok_or(DistributionError::NotInstalled)?;
        let active = record.active_version();
        let needs_consent = active.is_some_and(|version| {
            !record
                .consent
                .as_ref()
                .is_some_and(|consent| consent.covers(&version.signer, &version.capabilities))
        });
        let blocked = record
            .active
            .is_some_and(|digest| admissible(record, &digest, trust).is_err());
        Ok(PluginStatus {
            id: id.clone(),
            origin: record.origin,
            version: active.map(|version| version.version.clone()),
            enablement: record.enablement,
            needs_consent,
            blocked,
            revocation_age_secs: trust.revocation_age(now),
        })
    }
}

fn admissible<'a>(
    record: &'a PluginRecord,
    digest: &Sha256Hex,
    trust: &TrustStore,
) -> Result<&'a InstalledVersion, DistributionError> {
    let version = record
        .version(digest)
        .ok_or(DistributionError::NotInstalled)?;
    if version.quarantined {
        return Err(DistributionError::Quarantined);
    }
    if trust.is_artifact_revoked(digest) {
        return Err(DistributionError::ArtifactRevoked);
    }
    if trust.is_key_revoked(&version.signer) {
        return Err(DistributionError::KeyRevoked);
    }
    let covered = record
        .consent
        .as_ref()
        .is_some_and(|consent| consent.covers(&version.signer, &version.capabilities));
    if covered {
        Ok(version)
    } else {
        Err(DistributionError::ConsentRequired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::registry::tests::version;

    #[test]
    fn bundled_registration_respects_a_sticky_mask() {
        let id = PluginId::parse("dctx/git").unwrap();
        let trust = TrustStore::new();
        let mut registry = Registry::default();
        registry.register_bundled(&id, version(1, 1));
        let status = registry.status(&id, &trust, 0).unwrap();
        assert_eq!(
            (status.enablement, status.needs_consent),
            (Enablement::Enabled, true)
        );
        assert_eq!(
            registry.admit(&id, &trust, 0, None),
            Err(DistributionError::ConsentRequired)
        );
        registry.disable(&id).unwrap();
        registry.register_bundled(&id, version(2, 2));
        let record = registry.get(&id).unwrap();
        assert_eq!(
            record.active,
            Some(Sha256Hex::from_bytes([1; 32])),
            "masked plugin is not reselected"
        );
        assert_eq!(
            registry.admit(&id, &trust, 0, None),
            Err(DistributionError::Disabled)
        );
    }
}
