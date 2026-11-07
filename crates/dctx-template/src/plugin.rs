// SPDX-License-Identifier: Apache-2.0
//! Plugin catalog, typed grants and the compiled plugin plan.
//! Implements PLUGINS.md §3.5 and plugin-api/dts.md (exact locks, typed config).
//! Depends on [`crate::error`]. [`crate::plugin_plan`] checks a template against a host.
//! A template narrows this catalog. It cannot fetch, retarget or widen it.

mod catalog;
mod core;
mod limits;

pub use catalog::{RESERVED_ALIASES, bundled_git_plugin};
pub use core::{
    Capability, ConfigEntry, ConfigValue, FieldKind, FieldSpec, HookDecl, KnownPlugin,
    NetworkPolicy, PluginGroup, PluginOrigin, PluginState, Resolution, UnavailableReason,
};
pub use limits::{Liveness, PluginBinding, PluginPlan, ResourceLimits};
