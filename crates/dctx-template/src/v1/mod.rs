// SPDX-License-Identifier: Apache-2.0
//! DTS v1 variables, partials, layout and marker rendering.
//! Implements ADR-003 and the byte-level behaviour of dctx-core's template module.
//! Depends on [`crate::error`]. Used by v2 RENDER lowering and by [`crate::render_markers`].
//! Two substitution passes resolve nested variables; cycles are refused at compile time.

mod compiled;
mod parse;
mod render;

pub use compiled::CompiledTemplate;
pub(crate) use parse::finish;
pub use parse::{Template, replace_vars};
pub use render::{
    MarkerNode, render_markers, write_legacy_diff, write_legacy_snapshot,
    write_legacy_snapshot_with,
};
