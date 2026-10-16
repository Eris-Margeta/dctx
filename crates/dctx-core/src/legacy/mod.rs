//! Legacy compatibility modules (DX-B0 bridge): today's IO-heavy pack pipeline, unchanged in behaviour.
//! Re-exported at their old paths (`dctx_core::walker`, `dctx_core::ignore`, …) so dctx-cli and the
//! desktop keep building; rules and facts delegate to the pure modules and the new engine is dctx-pack.
//! Removed at the announced breaking release once consumers move to the facade (proposal §7, D3).

pub mod config;
pub mod content_reader;
pub mod git_safety;
pub mod ignore;
pub mod llm_formatter;
pub mod omit_content;
pub mod reader;
pub mod registry;
pub mod state;
pub mod template;
pub mod walker;
pub mod writer;
