// SPDX-License-Identifier: Apache-2.0
//! Selection bounded context: which items a pack sees and whose content it may read.
//! Implements the ignore/omit rule semantics of ARCHITECTURE-PROPOSAL §2 and PRODUCT-CONTRACTS C03.
//! Depends on [`crate::datatypes`] and [`crate::domain`]; pure, with no filesystem or environment access.
//! Used by filesystem adapters, frontends and the dctx-pack engine.

pub mod defaults;
pub mod glob;
pub mod policy;
pub mod rule;

pub use defaults::{HARDCODED_IGNORE_RULES, HARDCODED_OMIT_CONTENT_RULES};
pub use glob::glob_match;
pub use policy::{LayerCounts, RuleSource, RuleTexts, SelectionPolicy};
pub use rule::{last_match, parse_ignore_pattern_line, parse_rules, rule_matches};

mod compiled;
