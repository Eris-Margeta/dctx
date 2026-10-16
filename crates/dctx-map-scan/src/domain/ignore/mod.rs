//! Public ignore engine and compiled pattern API.
//! Implements SPEC-MAP-MODE section 6.1 and ADR-015b.
//! Pure matching consumes rules supplied by filesystem adapters.

pub(crate) mod defaults;
mod diagnostics;
mod engine;
mod glob;
mod matching;
mod parser;
mod rule;
#[cfg(test)]
use crate::adapters::rules::IgnoreError;
#[cfg(test)]
use crate::domain::{FileHandlingDecision, ItemContext};
pub use diagnostics::CacheStats;
pub use engine::{EngineStats, IgnoreEngine};
#[cfg(test)]
use glob::glob_match;
pub use matching::{MatchCache, should_ignore_item, should_ignore_item_cached};
pub use parser::parse_ignore_pattern_line;
pub use rule::{IgnoreRule, PatternType};
#[cfg(test)]
use std::{fs, path::PathBuf};
#[cfg(test)]
mod parity_0;
#[cfg(test)]
mod parity_1;
#[cfg(test)]
mod parity_2;
#[cfg(test)]
mod parity_3;
#[cfg(test)]
mod parity_4;
