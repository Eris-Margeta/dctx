//! Pure DTS v1 renderer and v2 `ContextPlan` compiler.
//! Implements ARCHITECTURE-PROPOSAL §2 and §7 row DX-B2, contracts/DTS-v2.md, and PLUGINS.md §3.5.
//! Depends on dctx-core for decisions and token estimates, and on dctx-hash for SHA-256.
//! No filesystem, process, network or plugin execution; callers use [`compile`], [`load`] and [`render_plan`].

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]

mod binary;
mod bound;
mod error;
mod grant;
mod legacy_diff;
mod legacy_output;
mod lower;
mod model;
mod opcode;
mod plugin;
mod plugin_codec;
mod plugin_plan;
mod source;
mod v1;

pub use binary::{from_bytes, to_bytes};
pub use bound::{authorize_path, narrow_decision, render_plan, AnswerRecord};
pub use error::{TemplateError, TemplateValidationError};
pub use legacy_diff::render_legacy_diff;
pub use legacy_output::{render_legacy_snapshot, LegacyLayout, LegacyOutput, TreeStamps};
pub use lower::compile;
pub use model::{
    BudgetPlan, ContextPlan, GrantRule, HostCeiling, IndexPlan, Inst, NamedCode, RenderProgram, SelectRule,
    Selection, ViewPlan,
};
pub use plugin::{
    bundled_git_plugin, Capability, ConfigEntry, ConfigValue, FieldKind, FieldSpec, HookDecl, KnownPlugin, Liveness,
    NetworkPolicy, PluginBinding, PluginGroup, PluginOrigin, PluginPlan, PluginState, Resolution, ResourceLimits,
    UnavailableReason, RESERVED_ALIASES,
};
pub use v1::{render_markers, replace_vars, CompiledTemplate, MarkerNode, Template};

/// Loads a plan and checks plugin locks, order and ceilings against `host`.
/// [`from_bytes`] checks structure only. A blob the current policy would refuse does not load.
///
/// # Errors
/// Propagates binary decoding failures and refuses plugin locks, ordering, grants or ceilings that the current host policy rejects.
pub fn load(bytes: &[u8], host: &HostCeiling) -> Result<ContextPlan, TemplateError> {
    let plan = from_bytes(bytes)?;
    plugin_plan::revalidate(&plan, host)?;
    Ok(plan)
}
