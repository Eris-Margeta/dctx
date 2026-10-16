//! Pure dctx/git domain: rules, ignore text, globs, coverage, the protection plan, outcomes and commands.
//! Implements PLUGINS.md §6.1–6.2 and plugin-api git.md; no IO, no protocol framing, no broker calls.
//! Depends on std only; used by the app layer and by the property tests.

pub mod command;
pub mod coverage;
pub mod glob;
pub mod outcome;
pub mod pattern;
pub mod plan;
pub mod rules;
pub mod text;
