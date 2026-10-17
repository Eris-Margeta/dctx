//! Injected desktop plugin operations (PLUGINS.md §5, §7.3 PL-APP).
//! Uses Distribution, its ports and the facade bundled catalog; mirrors CLI management.
//! Tauri supplies storage roots through Store, clock, network, trust and confinement.
//! List/help/check-update are read-only; no operation executes a plugin or Git.

use dctx_plugin_distribution::{
    Clock, Confinement, ConsentApproval, Distribution, DistributionError, Enablement, InstallPolicy, Network, Origin,
    PluginId, Registry, Store, TrustStore, VerifiedIndex, Version,
};

use crate::plugins::{bundled_plugin_version, PluginUpdate};
use crate::plugins_view::{capability_lines, row, PluginRow, PluginState};
use crate::DesktopError;

/// A last management failure supplied by the caller; listing never writes failure state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginFailure {
    /// Identity whose operation failed.
    pub id: PluginId,
    /// Typed refusal retained by the caller.
    pub reason: DistributionError,
}

/// Borrowed management facade with every environmental input supplied by the adapter.
pub struct PluginOps<'a> {
    distribution: Distribution<'a>,
    clock: &'a dyn Clock,
    trust: &'a TrustStore,
    confinement: Confinement,
    failures: &'a [PluginFailure],
}

impl<'a> PluginOps<'a> {
    /// Wires ports without reading or creating storage. Confinement defaults to unavailable.
    pub fn new(
        network: &'a dyn Network,
        store: &'a dyn Store,
        clock: &'a dyn Clock,
        trust: &'a TrustStore,
        policy: &'a InstallPolicy,
    ) -> Self {
        Self {
            distribution: Distribution::new(network, store, clock, trust, policy),
            clock,
            trust,
            confinement: Confinement::Unavailable,
            failures: &[],
        }
    }

    /// Supplies the platform adapter's confinement result for third-party enablement.
    pub fn with_confinement(mut self, confinement: Confinement) -> Self {
        self.confinement = confinement;
        self
    }

    /// Supplies last operation failures for the UI. Later entries take precedence.
    pub fn with_failures(mut self, failures: &'a [PluginFailure]) -> Self {
        self.failures = failures;
        self
    }

    /// Lists cached metadata and a synthetic bundled row without persisting it.
    /// A copied binary has the enabled preference and a needs-consent badge.
    pub fn list(&self) -> Result<Vec<PluginRow>, DesktopError> {
        let mut registry = self.distribution.registry()?;
        let catalog = dctx::bundled_git_plugin();
        let id = PluginId::parse(&catalog.id)?;
        if registry.get(&id).is_none() {
            registry.register_bundled(&id, bundled_plugin_version()?);
        }
        registry.records().map(|(id, _)| self.view(&registry, id)).collect()
    }

    /// Describes consent and update behavior without reading any port.
    pub fn help(&self) -> &'static str {
        "Bundled git is enabled by default; its disclosed grants require explicit consent. \
         Enable never grants consent. Disable persists through updates. \
         Third-party installs stay inactive until enablement and consent. \
         Check-update reads only the supplied verified index; bundled updates follow the app release."
    }

    /// Enables using existing consent. Missing consent returns a needs-consent row without writes.
    pub fn enable(&self, raw_id: &str) -> Result<PluginRow, DesktopError> {
        let id = PluginId::parse(raw_id)?;
        let mut current = self.find(&id)?;
        if current.needs_consent {
            current.state = PluginState::NeedsConsent;
            return Ok(current);
        }
        self.distribution.enable(&id, None, self.confinement)?;
        self.find(&id)
    }

    /// Persists a sticky user disable. Returns leased worker count for adapter cancellation.
    /// The adapter must stop those workers; this pure seam never starts or stops processes.
    pub fn disable(&self, raw_id: &str) -> Result<u32, DesktopError> {
        let id = PluginId::parse(raw_id)?;
        self.prepare_bundled(&id)?;
        Ok(self.distribution.disable(&id)?)
    }

    /// Explicitly approves the reviewed digest/grants/scope and enables it, as CLI --approve does.
    /// Approval is caller-supplied; this method never invents a grant or chooses a scope.
    pub fn consent(&self, raw_id: &str, approval: &ConsentApproval) -> Result<PluginRow, DesktopError> {
        let id = PluginId::parse(raw_id)?;
        self.prepare_bundled(&id)?;
        let registry = self.distribution.registry()?;
        let record = registry.get(&id).ok_or(DistributionError::NotInstalled)?;
        if record.active.is_none() {
            if record.origin != Origin::Bundled && self.confinement != Confinement::Established {
                return Err(DistributionError::ConfinementRequired.into());
            }
            let candidate = record
                .version(&approval.digest)
                .ok_or(DistributionError::ConsentRequired)?;
            if !candidate.capabilities.is_within(&approval.capabilities) {
                return Err(DistributionError::ConsentRequired.into());
            }
            self.distribution.activate(&id, &approval.digest)?;
        }
        self.distribution.enable(&id, Some(approval), self.confinement)?;
        self.find(&id)
    }

    /// Checks the supplied verified index; never fetches, installs, activates or grants consent.
    /// Bundled entries report the containing-release path, matching CLI update --check.
    pub fn check_update(
        &self,
        raw_id: &str,
        index: Option<&VerifiedIndex>,
        version: Option<&Version>,
    ) -> Result<PluginUpdate, DesktopError> {
        let id = PluginId::parse(raw_id)?;
        let registry = self.distribution.registry()?;
        let bundled = id.as_str() == dctx::bundled_git_plugin().id
            && registry.get(&id).is_none_or(|record| record.origin == Origin::Bundled);
        let candidate = if bundled {
            None
        } else {
            let index = index.ok_or(DistributionError::NotFound)?;
            Some(
                index
                    .entry(&id, self.trust)?
                    .select(version)?
                    .version
                    .as_str()
                    .to_owned(),
            )
        };
        Ok(PluginUpdate {
            id: raw_id.to_owned(),
            version: candidate,
            containing_release: bundled,
        })
    }

    fn prepare_bundled(&self, id: &PluginId) -> Result<(), DesktopError> {
        if id.as_str() == dctx::bundled_git_plugin().id && self.distribution.registry()?.get(id).is_none() {
            self.distribution.register_bundled(id, bundled_plugin_version()?)?;
        }
        Ok(())
    }

    fn find(&self, id: &PluginId) -> Result<PluginRow, DesktopError> {
        self.list()?
            .into_iter()
            .find(|row| row.id == id.as_str())
            .ok_or(DistributionError::NotInstalled.into())
    }

    fn view(&self, registry: &Registry, id: &PluginId) -> Result<PluginRow, DesktopError> {
        let record = registry.get(id).ok_or(DistributionError::NotInstalled)?;
        let mut status = registry.status(id, self.trust, self.clock.now())?;
        // An install may be complete but not yet activated. Disclose that candidate's
        // grants while preserving inactive enablement, rather than claiming it runs.
        let selected = record
            .active_version()
            .or_else(|| record.versions.iter().max_by_key(|v| v.release_sequence));
        let mut failure = None;
        let capabilities = if let Some(version) = selected {
            status.version = Some(version.version.clone());
            status.needs_consent = !record
                .consent
                .as_ref()
                .is_some_and(|consent| consent.covers(&version.signer, &version.capabilities));
            failure = if version.quarantined {
                Some(DistributionError::Quarantined)
            } else if self.trust.is_artifact_revoked(&version.digest) {
                Some(DistributionError::ArtifactRevoked)
            } else if self.trust.is_key_revoked(&version.signer) {
                Some(DistributionError::KeyRevoked)
            } else {
                None
            };
            capability_lines(&version.capabilities)?
        } else {
            Vec::new()
        };
        let failure = self
            .failures
            .iter()
            .rev()
            .find(|last| last.id == *id)
            .map(|last| last.reason)
            .or(failure);
        let mut result = row(&status, capabilities, failure.map(|reason| reason.to_string()));
        if record.origin == Origin::Bundled && id.as_str() == dctx::bundled_git_plugin().id {
            result.capabilities.extend(dctx::bundled_git_plugin().capabilities);
        }
        // NeverEnabled and Disabled both remain inactive even if consent is missing.
        result.enabled = record.enablement == Enablement::Enabled;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_refusals_keep_the_single_error_mapping() {
        let reason = DistributionError::ConfinementRequired;
        assert_eq!(DesktopError::from(reason), DesktopError::Plugin { reason });
    }
}
