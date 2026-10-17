//! Tauri commands for the desktop plugin surface (PLUGINS.md §7.3 PL-APP).
//! Delegates lifecycle rules and command error strings to dctx-desktop-api.
//! Depends on distribution types, the protocol capability decoder and plugin_ports.
//! Consent transports caller-approved fields only; no worker or Git process is started.

use dctx_desktop_api::{DesktopError, PluginOps, PluginRow, PluginUpdate};
use dctx_plugin_distribution::{
    CapabilitySet, ConsentApproval, ConsentScope, DistributionError, Document, InstallPolicy,
    NetworkError, Sha256Hex, TrustStore,
};
use dctx_plugin_protocol::{decode_object, DecodeLimits};
use serde::Deserialize;

use crate::plugin_ports::{host_target, FsStore, Offline, SystemClock};

/// JSON transport of the API's explicit digest, capability and scope approval.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsentRequest {
    digest: String,
    capabilities: serde_json::Value,
    scope: ApprovalScope,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum ApprovalScope {
    User,
    Project { path: String },
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
    with_ops(|ops| ops.list())
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
/// Capabilities use the manifest's required/optional/outputs/network object;
/// scope is {kind: "user"} or {kind: "project", path: "..."}.
#[tauri::command]
pub fn consent_plugin(id: String, approval: ConsentRequest) -> Result<PluginRow, String> {
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
