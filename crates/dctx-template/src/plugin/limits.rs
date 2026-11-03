// SPDX-License-Identifier: Apache-2.0
//! Compiled plugin binding, admitted ceilings, liveness and the plan shape.
//! Implements PLUGINS.md §3.5 and plugin-api/dts.md (admitted limits and liveness).
//! Depends on [`super`] value types and [`super::super::error`]; used by the binary and plan codecs.

use super::*;
use crate::error::TemplateError;

/// One compiled plugin binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginBinding {
    /// Plugin id.
    pub id: String,
    /// Resolved version, or empty when unavailable.
    pub version: String,
    /// Artifact digest from the host lock.
    pub digest: Option<[u8; 32]>,
    /// Manifest digest from the host lock.
    pub manifest_digest: Option<[u8; 32]>,
    /// Publisher from the host lock, or empty when unavailable.
    pub publisher_key: String,
    /// Origin from the host lock. Absent when unavailable.
    pub origin: Option<PluginOrigin>,
    /// Available lock or recorded absence.
    pub resolution: Resolution,
    /// The template marked the plugin required. Unavailable entries are false.
    pub required: bool,
    /// Enablement after user disable and catalog lookup.
    pub state: PluginState,
    /// Command alias, or empty.
    pub alias: String,
    /// Effective hooks. Empty unless [`PluginState::Enabled`].
    pub hooks: Vec<HookDecl>,
    /// Effective typed grants. Empty unless [`PluginState::Enabled`].
    pub allow: Vec<Capability>,
    /// Effective config in key order. Empty unless [`PluginState::Enabled`].
    pub config: Vec<ConfigEntry>,
    /// Plugins that must run before this one.
    pub before: Vec<String>,
    /// Plugins that must run after this one.
    pub after: Vec<String>,
}

/// Caller and organization resource ceiling copied into the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceLimits {
    /// Largest accepted frame.
    pub frame_bytes: u64,
    /// Most bytes read through the broker.
    pub read_bytes: u64,
    /// Most broker actions.
    pub actions: u64,
    /// Most outstanding broker requests.
    pub outstanding_requests: u64,
    /// Diagnostics ring.
    pub diagnostics_bytes: u64,
    /// Largest raw chunk.
    pub chunk_bytes: u64,
    /// Plugin state quota.
    pub state_bytes: u64,
    /// Temporary upload quota.
    pub upload_bytes: u64,
    /// Queue bytes.
    pub queue_bytes: u64,
    /// Memory budget.
    pub memory_bytes: u64,
    /// Work counter.
    pub work_units: u64,
}

impl ResourceLimits {
    /// Defaults from the v1 git manifest and dts plan fixture.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn standard() -> Self {
        Self {
            frame_bytes: 1_048_576,
            read_bytes: 8_388_608,
            actions: 128,
            outstanding_requests: 16,
            diagnostics_bytes: 65_536,
            chunk_bytes: 65_536,
            state_bytes: 65_536,
            upload_bytes: 1_052_672,
            queue_bytes: 2_097_152,
            memory_bytes: 67_108_864,
            work_units: 1_000_000,
        }
    }

    /// Every field is at most `ceiling`.
    ///
    /// # Errors
    /// Returns a budget-above-ceiling error for the first resource field that exceeds the corresponding ceiling.
    pub fn within(&self, ceiling: &Self) -> Result<(), TemplateError> {
        within(self.frame_bytes, ceiling.frame_bytes, "frame_bytes")?;
        within(self.read_bytes, ceiling.read_bytes, "read_bytes")?;
        within(self.actions, ceiling.actions, "actions")?;
        within(self.outstanding_requests, ceiling.outstanding_requests, "outstanding_requests")?;
        within(self.diagnostics_bytes, ceiling.diagnostics_bytes, "diagnostics_bytes")?;
        within(self.chunk_bytes, ceiling.chunk_bytes, "chunk_bytes")?;
        within(self.state_bytes, ceiling.state_bytes, "state_bytes")?;
        within(self.upload_bytes, ceiling.upload_bytes, "upload_bytes")?;
        within(self.queue_bytes, ceiling.queue_bytes, "queue_bytes")?;
        within(self.memory_bytes, ceiling.memory_bytes, "memory_bytes")?;
        within(self.work_units, ceiling.work_units, "work_units")
    }
}

fn within(requested: u64, ceiling: u64, field: &'static str) -> Result<(), TemplateError> {
    if requested > ceiling {
        Err(TemplateError::BudgetAboveCeiling { field, requested, ceiling })
    } else {
        Ok(())
    }
}

/// Liveness observation policy. The template cannot set it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Liveness {
    /// Interactive continue/cancel. No inactivity kill.
    Interactive {
        /// Observation cadence, in milliseconds.
        sample_ms: u64,
        /// Reap grace after cancel, in milliseconds.
        cancel_grace_ms: u64,
    },
    /// Unattended. Unresolved suspicion fails closed.
    UnattendedFailClosed {
        /// Observation cadence, in milliseconds.
        sample_ms: u64,
        /// Reap grace after cancel, in milliseconds.
        cancel_grace_ms: u64,
    },
    /// Unattended with a bounded last-resort inactivity window.
    UnattendedBounded {
        /// Observation cadence, in milliseconds.
        sample_ms: u64,
        /// Reap grace after cancel, in milliseconds.
        cancel_grace_ms: u64,
        /// Inactivity observation window, in milliseconds.
        inactivity_ms: u64,
    },
}

impl Liveness {
    /// Unattended fail-closed defaults from the dts plan fixture.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn fail_closed() -> Self {
        Self::UnattendedFailClosed { sample_ms: 1000, cancel_grace_ms: 5000 }
    }
}

/// Plugin section of a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPlan {
    /// PLUGINS section name, or empty when the source has none.
    pub organization: String,
    /// At most one group.
    pub groups: Vec<PluginGroup>,
    /// Bindings in id order.
    pub plugins: Vec<PluginBinding>,
    /// Enabled plugins in topological order. Canonical id breaks ties.
    pub order: Vec<String>,
    /// Admitted resource ceiling.
    pub limits: ResourceLimits,
    /// Admitted liveness policy.
    pub liveness: Liveness,
}
