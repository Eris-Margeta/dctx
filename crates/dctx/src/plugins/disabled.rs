// SPDX-License-Identifier: Apache-2.0
//! Plugin-free facade host for PP6 strict isolation (F-STRICT).
//! Uses only facade and template types; no runtime ports or process primitives exist here.
//! Session hooks use the plan's participants to report unavailability before any admission.

use super::{Boundary, HookEffect};
use crate::domain::DctxError;
use dctx_template::{PluginBinding, PluginPlan, PluginState};

/// Unavailable plugin host in a build without `plugin-runtime`.
/// Constructors and scripts retain no state; required hooks refuse and advisory hooks warn.
#[derive(Debug, Clone)]
pub struct PluginHost;

impl PluginHost {
    /// Returns an unavailable host; even fake workers cannot run in this build.
    pub fn fake() -> Self {
        Self
    }

    /// Returns an unavailable host without opening platform ports.
    pub fn platform() -> Self {
        Self
    }

    /// Refuses audit setup without inspecting the directory or creating storage.
    #[cfg(feature = "fs")]
    pub fn platform_with_audit(_: &std::path::Path, _: usize) -> Result<Self, DctxError> {
        Err(unavailable())
    }

    /// Discards a script. No effect can execute while the runtime is disabled.
    pub fn on(self, _: impl Into<String>, _: Boundary, _: Vec<HookEffect>) -> Self {
        self
    }

    /// Refuses file seeding because this build contains no plugin file port.
    pub fn seed(self, _: &str, _: &[u8]) -> Result<Self, DctxError> {
        Err(unavailable())
    }

    /// Workers started: always zero.
    pub fn spawns(&self) -> u64 {
        0
    }

    /// Launch attempts: always zero.
    pub fn admissions(&self) -> u64 {
        0
    }

    /// Git recipe children: always zero.
    pub fn git_children(&self) -> u64 {
        0
    }
}

fn unavailable() -> DctxError {
    DctxError::UnsupportedCapability {
        capability: "plugin-runtime",
        detail: "plugins are unavailable in this build".into(),
    }
}

pub(crate) fn participants(plan: &PluginPlan, boundary: Boundary) -> Vec<&PluginBinding> {
    plan.order
        .iter()
        .filter_map(|id| plan.plugins.iter().find(|plugin| &plugin.id == id))
        .filter(|plugin| plugin.state == PluginState::Enabled)
        .filter(|plugin| plugin.hooks.iter().any(|hook| hook.name == boundary.as_str() && hook.version == 1))
        .collect()
}
