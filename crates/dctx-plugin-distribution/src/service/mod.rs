// SPDX-License-Identifier: Apache-2.0
//! The distribution facade shared by CLI and app: policy, ports, lock and registry IO.
//! Implements PLUGINS.md §5.1 "facade operations shared with the app" and §5.3 "operation lock".
//! Depends on ports and domain; submodules hold download, install and management operations.
//! Used by PL-CLI and PL-APP; nothing here launches a plugin.

mod download;
mod install;
mod manage;

pub use install::InstallReport;
pub use manage::{RecoveryReport, VerifyOutcome};

use crate::domain::names::Target;
use crate::domain::origin::OriginPolicy;
use crate::domain::registry::Registry;
use crate::domain::registry_codec;
use crate::domain::trust::TrustStore;
use crate::error::DistributionError;
use crate::ports::{Clock, LockToken, Network, Store};

/// Default per-artifact download ceiling (256 MiB).
pub const DEFAULT_MAX_ARTIFACT_BYTES: u64 = 256 << 20;

/// Installer policy chosen by the user or organization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallPolicy {
    /// Host target whose artifact is selected.
    pub target: Target,
    /// Per-payload download/storage ceiling.
    pub max_artifact_bytes: u64,
    /// Offline bundle container ceiling.
    pub max_bundle_bytes: u64,
    /// Download origin rules.
    pub origin: OriginPolicy,
    /// Organization freshness ceiling for revocation state, in seconds.
    pub freshness_ceiling_secs: Option<u64>,
}

impl InstallPolicy {
    /// Defaults for `target`: 256 MiB per artifact, bundles up to 8 artifacts' worth, no ceiling.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn new(target: Target) -> Self {
        Self {
            target,
            max_artifact_bytes: DEFAULT_MAX_ARTIFACT_BYTES,
            max_bundle_bytes: DEFAULT_MAX_ARTIFACT_BYTES * 8,
            origin: OriginPolicy::default(),
            freshness_ceiling_secs: None,
        }
    }
}

/// Facade over the ports; borrow-only so CLI and app can share one store.
pub struct Distribution<'a> {
    network: &'a dyn Network,
    store: &'a dyn Store,
    clock: &'a dyn Clock,
    trust: &'a TrustStore,
    policy: &'a InstallPolicy,
}

impl<'a> Distribution<'a> {
    /// Wires the facade.
    pub fn new(
        network: &'a dyn Network,
        store: &'a dyn Store,
        clock: &'a dyn Clock,
        trust: &'a TrustStore,
        policy: &'a InstallPolicy,
    ) -> Self {
        Self {
            network,
            store,
            clock,
            trust,
            policy,
        }
    }

    /// Current registry, read without the lock (list/info/search never mutate).
    /// # Errors
    /// Propagates storage read errors or malformed registry decoding.
    pub fn registry(&self) -> Result<Registry, DistributionError> {
        match self.store.load_registry()? {
            Some(bytes) => registry_codec::decode(&bytes),
            None => Ok(Registry::default()),
        }
    }

    fn lock(&self) -> Result<Held<'a>, DistributionError> {
        Ok(Held {
            store: self.store,
            token: self.store.try_lock()?,
        })
    }

    fn save(&self, held: &Held<'_>, registry: &Registry) -> Result<(), DistributionError> {
        self.store
            .save_registry(held.token, &registry_codec::encode(registry))
    }

    /// Loads, applies `change` and atomically saves under the lock; nothing is saved on refusal.
    fn mutate<T>(
        &self,
        change: impl FnOnce(&mut Registry) -> Result<T, DistributionError>,
    ) -> Result<T, DistributionError> {
        let held = self.lock()?;
        let mut registry = self.registry()?;
        let result = change(&mut registry)?;
        self.save(&held, &registry)?;
        Ok(result)
    }
}

/// Held operation lock, released on drop (including on refusal or interruption).
struct Held<'a> {
    store: &'a dyn Store,
    token: LockToken,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.store.unlock(self.token);
    }
}
