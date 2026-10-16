//! DTS v1 variables, partials, layout and marker rendering.
//! Implements ADR-003 and the byte-level behaviour of dctx-core's template module.
//! Depends on [`crate::error`]. Used by v2 RENDER lowering and by [`crate::render_markers`].
//! Two substitution passes resolve nested variables; cycles are refused at compile time.

mod compiled;
mod parse;
mod render;

pub use compiled::CompiledTemplate;
pub use parse::{replace_vars, replace_vars_in_order, Template};
pub(crate) use parse::finish;
pub use render::{render_markers, MarkerNode};
pub(crate) use render::end_marker_of;
