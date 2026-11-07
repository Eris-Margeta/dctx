// SPDX-License-Identifier: Apache-2.0
//! Serializable plugin rows for the desktop (PLUGINS.md §7.3 PL-APP).
//! Projects public distribution records and the facade's bundled catalog into display data.
//! Depends on serde and distribution types; used by `plugins_ops` and later Tauri commands.

use dctx_plugin_distribution::{
    CapabilitySet, DistributionError, Document, Enablement, Origin, PluginId, PluginStatus,
    Registry,
};
use serde::{Deserialize, Serialize};

/// Where the selected plugin comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginSource {
    /// Part of the containing desktop release.
    Bundled,
    /// Independently installed, including an explicit bundled override.
    Installed,
}

/// Actionable state displayed by the plugin panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginState {
    /// Enabled with recorded consent.
    Enabled,
    /// Disabled or installed inactive.
    Disabled,
    /// Explicit approval is required before use.
    NeedsConsent,
    /// A failure reason is available.
    Failed,
}

/// One plugin row, derived without executing plugin code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRow {
    /// Canonical publisher/name identity.
    pub id: String,
    /// Catalog name, or the installed identity when no display name is exposed.
    pub name: String,
    /// Selected version, absent for an inactive install without activation.
    pub version: Option<String>,
    /// Bundled or independently installed.
    pub source: PluginSource,
    /// State for the primary UI badge.
    pub state: PluginState,
    /// Persistent enablement, including bundled enabled / needs-consent.
    pub enabled: bool,
    /// Whether the selected version lacks covering consent.
    pub needs_consent: bool,
    /// Human-readable requested grants, including bounds and scopes when exposed.
    pub capabilities: Vec<String>,
    /// Last supplied failure or current integrity/trust refusal.
    pub last_failure: Option<String>,
    /// Exact review payload, attached by the desktop adapter from its registry snapshot.
    #[serde(default)]
    pub consent: Option<serde_json::Value>,
}

impl PluginRow {
    /// Attaches lossless requested grants for explicit user-wide approval.
    /// Refuses a stale row rather than combining identity/version from different snapshots.
    ///
    /// # Errors
    /// Refuses malformed consent JSON or plugin/capability identities and propagates registry consent-view failures.
    pub fn with_consent(mut self, registry: &Registry) -> Result<Self, crate::DesktopError> {
        self.consent = None;
        let id = PluginId::parse(&self.id)?;
        let bundled;
        let selected = if let Some(record) = registry.get(&id) {
            record
                .active_version()
                .or_else(|| record.versions.iter().max_by_key(|v| v.release_sequence))
        } else if self.id == dctx::bundled_git_plugin().id {
            bundled = crate::bundled_plugin_version()?;
            Some(&bundled)
        } else {
            return Err(DistributionError::NotInstalled.into());
        };
        if let Some(version) = selected {
            if self.version.as_deref() != Some(version.version.as_str()) {
                return Err(DistributionError::ConsentRequired.into());
            }
            let mut capabilities = serde_json::json!({
                "required": [], "optional": [], "outputs": [], "network": []
            });
            let malformed = DistributionError::Malformed {
                document: Document::Registry,
                reason: "capability-display",
            };
            for (section, entry) in version
                .capabilities
                .expansion_over(&CapabilitySet::default())
            {
                let entry: serde_json::Value =
                    serde_json::from_str(&entry).map_err(|_| malformed)?;
                capabilities
                    .get_mut(&section)
                    .and_then(|value| value.as_array_mut())
                    .ok_or(malformed)?
                    .push(entry);
            }
            self.consent = Some(serde_json::json!({
                "id": self.id, "version": version.version.as_str(),
                "digest": version.digest.to_string(), "capabilities": capabilities,
                "capability_lines": capability_lines(&version.capabilities)?,
                "scope": {"kind": "user"}
            }));
        }
        Ok(self)
    }
}

pub(crate) fn row(
    status: &PluginStatus,
    capabilities: Vec<String>,
    failure: Option<String>,
) -> PluginRow {
    let enabled = status.enablement == Enablement::Enabled;
    let state = if failure.is_some() {
        PluginState::Failed
    } else if !enabled {
        PluginState::Disabled
    } else if status.needs_consent {
        PluginState::NeedsConsent
    } else {
        PluginState::Enabled
    };
    PluginRow {
        id: status.id.as_str().to_owned(),
        name: status.id.as_str().to_owned(),
        version: status
            .version
            .as_ref()
            .map(|version| version.as_str().to_owned()),
        source: if status.origin == Origin::Bundled {
            PluginSource::Bundled
        } else {
            PluginSource::Installed
        },
        state,
        enabled,
        needs_consent: status.needs_consent,
        capabilities,
        last_failure: failure,
        consent: None,
    }
}

pub(crate) fn capability_lines(
    capabilities: &CapabilitySet,
) -> Result<Vec<String>, DistributionError> {
    capabilities
        .expansion_over(&CapabilitySet::default())
        .into_iter()
        .map(|(section, entry)| {
            let refuse = DistributionError::Malformed {
                document: Document::Registry,
                reason: "capability-display",
            };
            let value: serde_json::Value = serde_json::from_str(&entry).map_err(|_| refuse)?;
            let fields = value.as_object().ok_or(refuse)?;
            let kind = fields
                .get("kind")
                .and_then(|kind| kind.as_str())
                .unwrap_or("capability");
            let action = match kind {
                "file.read" => "Read file",
                "file.write" => "Write file",
                "program.run" => "Run approved program operation",
                "outputs.read" => "Read selected outputs",
                other => other,
            };
            let details = fields
                .iter()
                .filter(|(key, _)| key.as_str() != "kind")
                .map(|(key, value)| {
                    let value = value
                        .as_str()
                        .map_or_else(|| value.to_string(), str::to_owned);
                    format!("{}: {value}", key.replace('_', " "))
                })
                .collect::<Vec<_>>()
                .join("; ");
            Ok(format!("{section}: {action} ({details})"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_tokens_match_the_ui_contract() {
        assert_eq!(
            serde_json::to_string(&PluginState::NeedsConsent).unwrap(),
            "\"needs-consent\""
        );
        assert_eq!(
            serde_json::to_string(&PluginSource::Bundled).unwrap(),
            "\"bundled\""
        );
    }
}
