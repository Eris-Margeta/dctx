//! Pure DTS v1 renderer and v2 ContextPlan compiler.
//! Implements ARCHITECTURE-PROPOSAL §2 and §7 row DX-B2, contracts/DTS-v2.md, and PLUGINS.md §3.5.
//! Depends on dctx-core for decisions and token estimates, and on dctx-hash for SHA-256.
//! No filesystem, process, network or plugin execution; callers use [`compile`] and [`render_plan`].

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]

mod binary;
mod bound;
mod error;
mod lower;
mod model;
mod opcode;
mod source;
mod v1;

pub use binary::{from_bytes, to_bytes};
pub use bound::{authorize_path, narrow_decision, render_plan, AnswerRecord};
pub use error::{TemplateError, TemplateValidationError};
pub use lower::compile;
pub use model::{
    bundled_git_plugin, BudgetPlan, ConfigEntry, ConfigValue, ContextPlan, FieldKind, FieldSpec, GrantRule,
    HostCeiling, IndexPlan, Inst, KnownPlugin, NamedCode, NetworkPolicy, PluginBinding, PluginGroup, PluginPlan,
    PluginState, RenderProgram, SelectRule, Selection, ViewPlan,
};
pub use v1::{render_markers, replace_vars, CompiledTemplate, MarkerNode, Template};
