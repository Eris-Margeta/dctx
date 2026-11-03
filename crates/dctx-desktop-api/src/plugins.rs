// SPDX-License-Identifier: Apache-2.0
//! Public desktop plugin seam and catalog-to-distribution conversion.
//! Implements PLUGINS.md §4, §5 and §7.3 PL-APP using public crate APIs only.
//! Depends on the facade catalog, its typed records and the distribution state codec.
//! Used by `plugins_ops` and the future Tauri adapter; no worker is executed.

use dctx_plugin_distribution::{
    domain::registry_codec, DistributionError, Document, InstalledVersion, PluginId, Sha256Hex,
};
use dctx_template::Capability;
use serde_json::{json, Value};

use crate::DesktopError;

pub use crate::plugins_ops::{PluginFailure, PluginOps};
pub use crate::plugins_view::{PluginRow, PluginSource, PluginState};

/// Cached update selection. A bundled plugin follows its containing signed release.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PluginUpdate {
    /// Canonical plugin identity.
    pub id: String,
    /// Candidate version from the supplied verified index; no download occurs.
    pub version: Option<String>,
    /// True when the containing desktop release supplies updates.
    pub containing_release: bool,
}

impl From<DistributionError> for DesktopError {
    fn from(error: DistributionError) -> Self {
        match error {
            DistributionError::ConsentRequired => Self::PluginNeedsConsent,
            reason => Self::Plugin { reason },
        }
    }
}

/// The bundled catalog represented as a distribution version, with exact requested grants.
/// This is metadata only and grants no consent. It uses the same signer identity as the CLI.
///
/// # Errors
/// Propagates invalid bundled metadata, capability-set or digest decoding errors.
pub fn bundled_plugin_version() -> Result<InstalledVersion, DesktopError> {
    let catalog = dctx::bundled_git_plugin();
    let missing = DistributionError::IntegrityFailure;
    let digest = Sha256Hex::from_bytes(catalog.digest.ok_or(missing)?);
    let manifest = Sha256Hex::from_bytes(catalog.manifest_digest.ok_or(missing)?);
    let signer = Sha256Hex::of(catalog.publisher_key.as_bytes())?;
    let entries = catalog
        .records
        .iter()
        .map(capability_entry)
        .collect::<Result<Vec<_>, _>>()?;
    // CapabilitySet is opaque. The public versioned codec imports exact typed catalog
    // entries without duplicating protocol decoding or inventing a signed manifest.
    let data = json!({"format": registry_codec::FORMAT, "plugins": [{
        "id": catalog.id, "origin": "bundled", "versions": [{
            "version": catalog.version, "release_sequence": 1,
            "digest": digest.to_string(), "manifest_sha256": manifest.to_string(),
            "signer": signer.to_string(), "capabilities": entries, "quarantined": false
        }], "active": digest.to_string(), "high_water": 1, "enablement": "enabled",
        "consent": null, "leases": [], "rollbacks": []
    }]});
    let registry = registry_codec::decode(data.to_string().as_bytes())?;
    let id = PluginId::parse(&catalog.id)?;
    registry
        .get(&id)
        .and_then(|record| record.active_version())
        .cloned()
        .ok_or(missing.into())
}

fn capability_entry(capability: &Capability) -> Result<Value, DistributionError> {
    let (section, entry) = match capability {
        Capability::FileRead { path, max_bytes } => (
            "required",
            json!({
                "kind": "file.read", "scope": "project", "path": path, "max_bytes": max_bytes
            }),
        ),
        Capability::FileWrite {
            path,
            create,
            max_growth_bytes,
            max_bytes,
        } => (
            "required",
            json!({
                "kind": "file.write", "scope": "project", "path": path, "create": create,
                "max_growth_bytes": max_growth_bytes, "max_bytes": max_bytes
            }),
        ),
        Capability::Program {
            operation,
            resources,
            max_output_bytes,
        } => (
            "optional",
            json!({
                "kind": "program.run", "program": "git", "operation": operation,
                "resources": resources, "max_output_bytes": max_output_bytes
            }),
        ),
        _ => {
            return Err(DistributionError::Malformed {
                document: Document::Manifest,
                reason: "bundled-capability",
            })
        }
    };
    Ok(json!({"section": section, "entry": entry}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_grants_preserve_scopes_and_limits() {
        let version = bundled_plugin_version().unwrap();
        let lines = crate::plugins_view::capability_lines(&version.capabilities)
            .unwrap()
            .join("\n");
        assert_eq!(version.capabilities.len(), 3);
        for expected in [
            "1048576",
            "1052672",
            "4096",
            "65536",
            "git-admin:stage-ignore",
            "project",
        ] {
            assert!(lines.contains(expected));
        }
        assert_eq!(
            DesktopError::from(DistributionError::ConsentRequired),
            DesktopError::PluginNeedsConsent
        );
        assert!(DesktopError::from(DistributionError::Quarantined)
            .command_string()
            .contains("Quarantined"));
    }
}
