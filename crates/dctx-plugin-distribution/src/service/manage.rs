//! Management operations: activate/update, rollback, enable, disable, remove, verify, admit and recover.
//! Implements PLUGINS.md §5.1 commands and §5.3 crash recovery (I-ATOMIC, I-MANAGEMENT, I-CONSENT):
//! every mutation holds the operation lock and switches the registry atomically; verify quarantines
//! failures without executing anything; recovery keeps the last complete activation and discards partials.
//! Depends on registry, trust and the ports; used by PL-CLI/PL-APP. Launch itself belongs to PL-A.

use super::Distribution;
use crate::domain::admission::{Admission, PluginStatus};
use crate::domain::consent::ConsentApproval;
use crate::domain::digest::{PayloadCheck, Sha256Hex};
use crate::domain::names::{PluginId, SafeFileName};
use crate::domain::registry::{Confinement, InstalledVersion};
use crate::error::DistributionError;
use crate::ports::InstallKey;

/// Result of reverifying one installed version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyOutcome {
    /// Version digest.
    pub digest: Sha256Hex,
    /// `Ok` or the refusal that quarantined it.
    pub result: Result<(), DistributionError>,
}

/// What crash recovery cleaned up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Partial staging directories deleted.
    pub discarded_staging: Vec<InstallKey>,
    /// Published directories the registry never referenced.
    pub removed_orphans: Vec<InstallKey>,
}

impl Distribution<'_> {
    /// Switches the activation record to an installed digest (update). Old versions stay for leases.
    /// # Errors
    /// Propagates stale revocation data, registry activation/consent refusals, and lock/storage failures.
    pub fn activate(&self, id: &PluginId, digest: &Sha256Hex) -> Result<(), DistributionError> {
        self.trust.check_freshness(self.clock.now(), self.policy.freshness_ceiling_secs)?;
        self.mutate(|registry| registry.activate(id, digest, self.trust))
    }

    /// Explicit exact-digest rollback below the high-water mark, with a recorded reason.
    /// # Errors
    /// Propagates freshness, rollback reason/version/consent refusals, and lock/storage failures.
    pub fn rollback(&self, id: &PluginId, digest: &Sha256Hex, reason: &str) -> Result<(), DistributionError> {
        self.trust.check_freshness(self.clock.now(), self.policy.freshness_ceiling_secs)?;
        self.mutate(|registry| registry.rollback(id, digest, reason, self.trust))
    }

    /// Enables the active version under existing or explicitly approved consent.
    /// # Errors
    /// Propagates freshness, version/trust/confinement/consent refusals, and lock/storage failures.
    pub fn enable(&self, id: &PluginId, approval: Option<&ConsentApproval>, confinement: Confinement) -> Result<(), DistributionError> {
        self.trust.check_freshness(self.clock.now(), self.policy.freshness_ceiling_secs)?;
        self.mutate(|registry| registry.enable(id, approval, confinement, self.trust))
    }

    /// Persists a sticky disable; returns how many leased workers the caller must cancel.
    /// # Errors
    /// Propagates unknown-plugin and lock/registry/storage failures.
    pub fn disable(&self, id: &PluginId) -> Result<u32, DistributionError> {
        self.mutate(|registry| registry.disable(id))
    }

    /// Removes every version after leases end; the disabled mask record stays.
    /// # Errors
    /// Refuses missing or leased plugins and propagates lock/registry/storage failures.
    pub fn remove(&self, id: &PluginId) -> Result<(), DistributionError> {
        let held = self.lock()?;
        let mut registry = self.registry()?;
        let record = registry.get(id).ok_or(DistributionError::NotInstalled)?;
        let keys: Vec<InstallKey> = record.versions.iter().map(|version| key(id, version)).collect();
        registry.remove_plugin(id)?;
        self.save(&held, &registry)?;
        for key in &keys {
            self.store.remove_installed(held.token, key)?;
        }
        Ok(())
    }

    /// Unlinks one inactive version whose leases have ended.
    /// # Errors
    /// Refuses missing or leased versions and propagates lock/registry/storage failures.
    pub fn remove_version(&self, id: &PluginId, digest: &Sha256Hex) -> Result<(), DistributionError> {
        let held = self.lock()?;
        let mut registry = self.registry()?;
        let version = registry.get(id).and_then(|record| record.version(digest)).ok_or(DistributionError::NotInstalled)?;
        let key = key(id, version);
        registry.remove_version(id, digest)?;
        self.save(&held, &registry)?;
        self.store.remove_installed(held.token, &key)
    }

    /// Rechecks every installed version's files, signature, trust and target; quarantines failures.
    /// # Errors
    /// Propagates lock/registry/storage failures; per-version verification failures are returned as outcomes and trigger quarantine.
    pub fn verify(&self, id: &PluginId) -> Result<Vec<VerifyOutcome>, DistributionError> {
        let held = self.lock()?;
        let mut registry = self.registry()?;
        let versions = registry.get(id).ok_or(DistributionError::NotInstalled)?.versions.clone();
        let mut outcomes = Vec::with_capacity(versions.len());
        for version in &versions {
            let result = self.reverify(id, version);
            if result.is_err() {
                registry.quarantine(id, &version.digest)?;
            }
            outcomes.push(VerifyOutcome { digest: version.digest, result });
        }
        self.save(&held, &registry)?;
        Ok(outcomes)
    }

    fn reverify(&self, id: &PluginId, version: &InstalledVersion) -> Result<(), DistributionError> {
        let key = key(id, version);
        let read = |name: &str| self.store.read_installed(&key, &SafeFileName::parse(name)?);
        let manifest = read("plugin.json")?;
        if Sha256Hex::of(&manifest)? != version.manifest_sha256 {
            return Err(DistributionError::IntegrityFailure);
        }
        let verified = self.trust.authenticate(&manifest, &read("plugin.json.sig")?, id)?;
        if verified.signer != version.signer {
            return Err(DistributionError::SignerChanged);
        }
        let artifact = verified.manifest.artifact_for(self.policy.target)?;
        if artifact.executable.sha256 != version.digest {
            return Err(DistributionError::TargetMismatch);
        }
        if self.trust.is_artifact_revoked(&version.digest) {
            return Err(DistributionError::ArtifactRevoked);
        }
        for payload in artifact.payloads() {
            let bytes = self.store.read_installed(&key, &payload.file)?;
            PayloadCheck::whole(&bytes, payload.bytes, payload.sha256)?;
        }
        Ok(())
    }

    /// Admits one worker on the exact active digest and leases it. The launcher must still bind bytes (PL-A).
    /// # Errors
    /// Propagates plugin admission/trust/freshness refusals and lock/registry/storage failures.
    pub fn admit(&self, id: &PluginId) -> Result<Admission, DistributionError> {
        let now = self.clock.now();
        self.mutate(|registry| registry.admit(id, self.trust, now, self.policy.freshness_ceiling_secs))
    }

    /// Ends a worker lease.
    /// # Errors
    /// Propagates lock/registry/storage failures while updating the lease count.
    pub fn release(&self, admission: &Admission) -> Result<(), DistributionError> {
        self.mutate(|registry| {
            registry.release(admission);
            Ok(())
        })
    }

    /// Records the bundled build the containing signed release authenticated; a sticky mask wins.
    /// # Errors
    /// Propagates lock/registry/storage failures while recording the bundled version.
    pub fn register_bundled(&self, id: &PluginId, version: InstalledVersion) -> Result<(), DistributionError> {
        self.mutate(|registry| {
            registry.register_bundled(id, version);
            Ok(())
        })
    }

    /// `plugin list` rows; reads metadata only.
    /// # Errors
    /// Propagates registry decoding/storage failures or an unavailable plugin status.
    pub fn list(&self) -> Result<Vec<PluginStatus>, DistributionError> {
        let registry = self.registry()?;
        let now = self.clock.now();
        registry.records().map(|(id, _)| registry.status(id, self.trust, now)).collect()
    }

    /// Discards partial staging and unreferenced published directories; the activation record is untouched.
    /// # Errors
    /// Propagates lock/registry/storage failures while removing abandoned staging or unreferenced installs.
    pub fn recover(&self) -> Result<RecoveryReport, DistributionError> {
        let held = self.lock()?;
        let registry = self.registry()?;
        let mut report = RecoveryReport::default();
        for key in self.store.staged()? {
            self.store.discard_staging(held.token, &key)?;
            report.discarded_staging.push(key);
        }
        for key in self.store.installed()? {
            let referenced = registry.get(&key.id).is_some_and(|record| record.version(&key.digest).is_some());
            if !referenced {
                self.store.remove_installed(held.token, &key)?;
                report.removed_orphans.push(key);
            }
        }
        Ok(report)
    }
}

fn key(id: &PluginId, version: &InstalledVersion) -> InstallKey {
    InstallKey { id: id.clone(), version: version.version.clone(), digest: version.digest }
}
