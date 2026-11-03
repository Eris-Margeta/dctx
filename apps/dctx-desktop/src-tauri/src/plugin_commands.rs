// SPDX-License-Identifier: Apache-2.0
//! Tauri commands for the desktop plugin surface (PLUGINS.md §7.3 PL-APP).
//! Delegates lifecycle rules and command error strings to dctx-desktop-api.
//! Depends on distribution types, the protocol capability decoder and plugin_ports.
//! Consent transports caller-approved fields only; no worker or Git process is started.

use dctx_desktop_api::{DesktopError, PluginOps, PluginRow, PluginUpdate};
use dctx_plugin_distribution::{
    domain::registry_codec, CapabilitySet, ConsentApproval, ConsentScope, DistributionError,
    Document, InstallPolicy, NetworkError, Registry, Sha256Hex, Store, TrustStore,
};
use dctx_plugin_protocol::{decode_object, DecodeLimits};
use serde::{Deserialize, Serialize};

use crate::plugin_ports::{host_target, FsStore, Offline, SystemClock};

/// JSON transport echoing the API's identity, version, digest, grants and scope review.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsentRequest {
    id: String,
    version: String,
    digest: String,
    capabilities: serde_json::Value,
    capability_lines: Vec<String>,
    scope: ApprovalScope,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum ApprovalScope {
    User,
    Project { path: String },
}

fn reviewable(rows: Vec<PluginRow>) -> Result<Vec<PluginRow>, String> {
    let store = FsStore::new(crate::config::desktop_config_dir()?.join("plugins"));
    let registry = store.load_registry()
        .and_then(|bytes| bytes.map_or_else(|| Ok(Registry::default()), |bytes| registry_codec::decode(&bytes)))
        .map_err(|reason| DesktopError::from(reason).command_string())?;
    rows.into_iter().map(|row| row.with_consent(&registry)
        .map_err(|error| error.command_string())).collect()
}

impl ConsentRequest {
    fn into_approval(self) -> Result<ConsentApproval, DesktopError> {
        let digest = Sha256Hex::parse(&self.digest, Document::Registry)?;
        let bytes = self.capabilities.to_string();
        let value = decode_object(bytes.as_bytes(), DecodeLimits::MANIFEST).map_err(|error| {
            DistributionError::Malformed {
                document: Document::Manifest,
                reason: error.reason,
            }
        })?;
        let capabilities = CapabilitySet::from_manifest(&value)?;
        let scope = match self.scope {
            ApprovalScope::User => ConsentScope::User,
            ApprovalScope::Project { path } => ConsentScope::Project(path),
        };
        Ok(ConsentApproval {
            digest,
            capabilities,
            scope,
        })
    }
}

fn with_ops<T>(
    operation: impl FnOnce(&PluginOps<'_>) -> Result<T, DesktopError>,
) -> Result<T, String> {
    let store = FsStore::new(crate::config::desktop_config_dir()?.join("plugins"));
    let clock = SystemClock;
    // Matches CLI product wiring: bundled metadata needs no downloaded publisher pin.
    let trust = TrustStore::new();
    let policy = InstallPolicy::new(host_target());
    let network = Offline;
    let ops = PluginOps::new(&network, &store, &clock, &trust, &policy);
    operation(&ops).map_err(|error| error.command_string())
}

/// Lists cached and bundled rows without creating or updating plugin storage.
#[tauri::command]
pub fn list_plugins() -> Result<Vec<PluginRow>, String> {
    reviewable(with_ops(|ops| ops.list())?)
}

/// Enables using existing consent, or returns a needs-consent row.
#[tauri::command]
pub fn enable_plugin(id: String) -> Result<PluginRow, String> {
    with_ops(|ops| ops.enable(&id))
}

/// Persists disablement and returns the refreshed plugin row.
/// This management surface has no worker launcher or process leases of its own.
#[tauri::command]
pub fn disable_plugin(id: String) -> Result<PluginRow, String> {
    with_ops(|ops| {
        ops.disable(&id)?;
        ops.list()?
            .into_iter()
            .find(|row| row.id == id)
            .ok_or_else(|| DistributionError::NotInstalled.into())
    })
}

/// Converts the explicit reviewed payload into the API's ConsentApproval and applies it.
/// Requires the exact consent payload disclosed by list_plugins, including its scope.
/// Capabilities use the manifest's required/optional/outputs/network object.
#[tauri::command]
pub fn consent_plugin(id: String, approval: ConsentRequest) -> Result<PluginRow, String> {
    let reviewed = serde_json::to_value(&approval).map_err(|error| error.to_string())?;
    let current = list_plugins()?.into_iter().find(|row| row.id == id);
    if approval.id != id || current.and_then(|row| row.consent).as_ref() != Some(&reviewed) {
        return Err(DesktopError::from(DistributionError::ConsentRequired).command_string());
    }
    with_ops(|ops| ops.consent(&id, &approval.into_approval()?))
}

/// Reports bundled app-release updates; independent discovery is offline until approved.
#[tauri::command]
pub fn check_plugin_update(id: String) -> Result<PluginUpdate, String> {
    with_ops(|ops| match ops.check_update(&id, None, None) {
        // No verified index can be fetched by the offline desktop adapter.
        Err(DesktopError::Plugin {
            reason: DistributionError::NotFound,
        }) => Err(DistributionError::Network(NetworkError::Unreachable).into()),
        result => result,
    })
}
