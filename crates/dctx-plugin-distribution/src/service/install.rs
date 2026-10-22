//! Install transaction: resolved → downloaded → verified → staged → installed-inactive.
//! Implements PLUGINS.md §5.1 install (index, `--url`, `--file`), §5.3 transaction and I-INTEGRITY,
//! I-OFFLINE, I-BUNDLE, I-IDENTITY: one trust gate for every source, size/hash while streaming,
//! partial or refused payloads never published, the version is recorded inactive, nothing is executed.
//! Depends on trust, bundle, sums, registry and the ports; used by CLI/app install and update.

use super::Distribution;
use crate::domain::bundle::OfflineBundle;
use crate::domain::consent::CapabilitySet;
use crate::domain::digest::{Fingerprint, PayloadCheck, Sha256Hex};
use crate::domain::index::VerifiedIndex;
use crate::domain::manifest::{Artifact, Payload, RELEASE_FILES};
use crate::domain::names::{PluginId, SafeFileName, Version};
use crate::domain::origin::HttpsUrl;
use crate::domain::registry::{InstalledVersion, Origin};
use crate::domain::signature::SIGNATURE_BYTES;
use crate::domain::sums::{MAX_SUMS_BYTES, check_sums};
use crate::domain::trust::VerifiedManifest;
use crate::error::{DistributionError, Document, NetworkError};
use crate::ports::InstallKey;

const MANIFEST_MAX: u64 = 262_144;

/// What a successful install recorded; the plugin stays inactive and disabled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallReport {
    /// Immutable version directory.
    pub key: InstallKey,
    /// Selection origin.
    pub origin: Origin,
    /// Signed release sequence.
    pub release_sequence: u64,
    /// Signer fingerprint (provenance display).
    pub signer: Fingerprint,
    /// Requested capabilities to display before enable.
    pub capabilities: CapabilitySet,
    /// Origins contacted for this install, for the "show requested origins" display.
    pub origins: Vec<String>,
}

enum Source<'s> {
    Remote(&'s HttpsUrl),
    Bundle(&'s OfflineBundle),
}

impl Distribution<'_> {
    /// Installs `id` from the verified discovery index (newest stable or exact `version`).
    /// # Errors
    /// Propagates index/trust selection, network/origin, signature, payload, quota, lock, and storage failures.
    pub fn install_from_index(
        &self,
        index: &VerifiedIndex,
        id: &PluginId,
        version: Option<&Version>,
    ) -> Result<InstallReport, DistributionError> {
        let entry = index.entry(id, self.trust)?;
        let release = entry.select(version)?;
        self.install_remote(id, &release.manifest_url, Some(release.manifest_sha256))
    }

    /// Installs from a signed manifest URL (`--url`); identity must match the approved `id`.
    /// # Errors
    /// Propagates origin/network, manifest/trust/signature, payload, quota, lock, and storage failures.
    pub fn install_url(&self, id: &PluginId, manifest_url: &HttpsUrl) -> Result<InstallReport, DistributionError> {
        self.install_remote(id, manifest_url, None)
    }

    fn install_remote(
        &self,
        id: &PluginId,
        manifest_url: &HttpsUrl,
        manifest_sha256: Option<Sha256Hex>,
    ) -> Result<InstallReport, DistributionError> {
        let held = self.lock()?;
        let manifest = self.fetch_all(manifest_url, MANIFEST_MAX)?;
        if manifest_sha256.is_some_and(|expected| Sha256Hex::of(&manifest).ok() != Some(expected)) {
            return Err(DistributionError::ManifestDigestMismatch);
        }
        let sig_url = manifest_url.sibling(&safe("plugin.json.sig")?);
        let signature = self.fetch_all(&sig_url, SIGNATURE_BYTES as u64)?;
        let verified = self.trust.authenticate(&manifest, &signature, id)?;
        match self.fetch_all(&manifest_url.sibling(&safe("SHA256SUMS")?), MAX_SUMS_BYTES) {
            Ok(sums) => check_sums(&verified.manifest, &sums)?,
            Err(DistributionError::Network(NetworkError::NotFound)) => {}
            Err(other) => return Err(other),
        }
        let mut report = self.accept(&held, verified, &signature, Source::Remote(manifest_url))?;
        report.origins = vec![manifest_url.origin()];
        Ok(report)
    }

    /// Installs an offline `.dctx-plugin` bundle (`--file`) with the same trust gates; no network use.
    /// # Errors
    /// Refuses malformed/oversized bundles, missing payloads, trust/signature/digest failures, unsupported targets, or lock/storage failures.
    pub fn install_bundle(&self, id: &PluginId, bytes: &[u8]) -> Result<InstallReport, DistributionError> {
        let held = self.lock()?;
        let bundle = OfflineBundle::parse(bytes, self.policy.max_bundle_bytes)?;
        let missing = DistributionError::malformed(Document::Bundle, "missing-manifest");
        let manifest = bundle.get("plugin.json").ok_or(missing)?;
        let signature = bundle.get("plugin.json.sig").ok_or(missing)?;
        let verified = self.trust.authenticate(&manifest.bytes, &signature.bytes, id)?;
        let artifact = verified.manifest.artifact_for(self.policy.target)?;
        check_bundle_payloads(&bundle, artifact, &verified)?;
        if let Some(sums) = bundle.get("SHA256SUMS") {
            check_sums(&verified.manifest, &sums.bytes)?;
        }
        let signature = signature.bytes.clone();
        self.accept(&held, verified, &signature, Source::Bundle(&bundle))
    }

    #[allow(clippy::needless_pass_by_value, reason = "The install transaction owns its verified manifest and selected payload source until publication completes.")]
    fn accept(
        &self,
        held: &super::Held<'_>,
        verified: VerifiedManifest,
        signature: &[u8],
        source: Source<'_>,
    ) -> Result<InstallReport, DistributionError> {
        let manifest = &verified.manifest;
        let artifact = manifest.artifact_for(self.policy.target)?;
        if self.trust.is_artifact_revoked(&artifact.executable.sha256) {
            return Err(DistributionError::ArtifactRevoked);
        }
        if artifact.payloads().any(|payload| payload.bytes > self.policy.max_artifact_bytes) {
            return Err(DistributionError::LimitExceeded { limit: self.policy.max_artifact_bytes });
        }
        let mut registry = self.registry()?;
        let origin = match registry.get(&manifest.id) {
            Some(record) if record.origin != Origin::Installed => Origin::Override,
            _ => Origin::Installed,
        };
        let version = InstalledVersion {
            version: manifest.version.clone(),
            release_sequence: manifest.release_sequence,
            digest: artifact.executable.sha256,
            manifest_sha256: Sha256Hex::of(manifest.bytes())?,
            signer: verified.signer,
            capabilities: manifest.capabilities.clone(),
            quarantined: false,
        };
        let already = registry.get(&manifest.id).is_some_and(|record| record.version(&version.digest).is_some());
        registry.record_install(&manifest.id, origin, version)?;
        let key = InstallKey { id: manifest.id.clone(), version: manifest.version.clone(), digest: artifact.executable.sha256 };
        if !already {
            let token = held.token;
            self.store.discard_staging(token, &key)?;
            let staged = self.stage_all(token, &key, artifact, manifest.bytes(), signature, &source);
            if let Err(error) = staged.and_then(|()| self.store.publish(token, &key)) {
                let _ = self.store.discard_staging(token, &key);
                return Err(error);
            }
            self.save(held, &registry)?;
        }
        Ok(InstallReport {
            key,
            origin,
            release_sequence: manifest.release_sequence,
            signer: verified.signer,
            capabilities: manifest.capabilities.clone(),
            origins: Vec::new(),
        })
    }

    fn stage_all(
        &self,
        token: crate::ports::LockToken,
        key: &InstallKey,
        artifact: &Artifact,
        manifest: &[u8],
        signature: &[u8],
        source: &Source<'_>,
    ) -> Result<(), DistributionError> {
        for payload in artifact.payloads() {
            self.stage_payload(token, key, payload, source)?;
        }
        self.store.stage_append(token, key, &safe("plugin.json")?, manifest)?;
        self.store.stage_append(token, key, &safe("plugin.json.sig")?, signature)
    }

    fn stage_payload(
        &self,
        token: crate::ports::LockToken,
        key: &InstallKey,
        payload: &Payload,
        source: &Source<'_>,
    ) -> Result<(), DistributionError> {
        let mut check = PayloadCheck::new(payload.bytes, payload.sha256);
        let mut sink = |chunk: &[u8]| -> Result<(), DistributionError> {
            check.update(chunk)?;
            self.store.stage_append(token, key, &payload.file, chunk)
        };
        match source {
            Source::Remote(manifest_url) => {
                self.fetch(&manifest_url.sibling(&payload.file), self.policy.max_artifact_bytes, &mut sink)?;
            }
            Source::Bundle(bundle) => {
                let entry = bundle.get(payload.file.as_str()).ok_or(DistributionError::TargetMismatch)?;
                sink(&entry.bytes)?;
            }
        }
        check.finish()
    }
}

fn safe(name: &str) -> Result<SafeFileName, DistributionError> {
    SafeFileName::parse(name)
}

/// Exactly the selected target's payloads plus ancillary release files; nothing undeclared.
fn check_bundle_payloads(bundle: &OfflineBundle, artifact: &Artifact, verified: &VerifiedManifest) -> Result<(), DistributionError> {
    if bundle.get(artifact.executable.file.as_str()).is_none() {
        return Err(DistributionError::TargetMismatch);
    }
    for payload in &artifact.runtime_resources {
        if bundle.get(payload.file.as_str()).is_none() {
            return Err(DistributionError::malformed(Document::Bundle, "missing-payload"));
        }
    }
    let other_targets = verified.manifest.artifacts.iter().filter(|other| other.target != artifact.target);
    let foreign: Vec<&str> = other_targets.flat_map(super::super::domain::manifest::Artifact::payloads).map(|p| p.file.as_str()).collect();
    for entry in bundle.entries() {
        let name = entry.name.as_str();
        let selected = artifact.payloads().any(|payload| payload.file.as_str() == name);
        if selected || RELEASE_FILES.contains(&name) {
            continue;
        }
        if foreign.contains(&name) {
            return Err(DistributionError::TargetMismatch);
        }
        return Err(DistributionError::malformed(Document::Bundle, "undeclared-payload"));
    }
    Ok(())
}
