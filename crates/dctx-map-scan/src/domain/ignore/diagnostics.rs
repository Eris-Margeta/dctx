// SPDX-License-Identifier: Apache-2.0
//! Ignore cache and rule diagnostics.
//! Implements ADR-015b observable matching without filesystem IO.
//! Depends on domain values and is used by the ignore engine.

use super::glob::glob_match;
use super::{IgnoreEngine, IgnoreRule, MatchCache, PatternType};
use crate::domain::ItemContext;

impl IgnoreEngine {
    /// Reports cache allocation and entry counts.
    pub fn cache_stats(&self) -> CacheStats {
        let cache = self.match_cache.borrow();
        CacheStats {
            entries: cache.len(),
            capacity: cache.capacity(),
            memory_bytes: cache.len() * std::mem::size_of::<((String, String), bool)>(),
        }
    }

    #[cfg(test)]
    /// Clears cached ignore matches.
    pub fn clear_cache(&self) {
        self.match_cache.borrow_mut().clear();
    }

    #[doc(hidden)]
    /// Exposes the ignore cache for parity diagnostics.
    pub fn match_cache(&self) -> &MatchCache {
        &self.match_cache
    }

    #[doc(hidden)]
    /// Returns compiled ignore rules.
    pub fn ignore_rules(&self) -> &[IgnoreRule] {
        &self.ignore_rules
    }
}

#[derive(Debug, Clone)]
/// Cache allocation diagnostics.
pub struct CacheStats {
    /// Cached result count.
    pub entries: usize,
    /// Allocated result capacity.
    pub capacity: usize,
    /// Estimated occupied result bytes.
    pub memory_bytes: usize,
}

pub(super) fn matches_rule(context: &ItemContext, rule: &IgnoreRule) -> bool {
    if rule.is_dir_only && !context.is_directory {
        return false;
    }

    match rule.pattern_type {
        PatternType::Invalid => false,
        PatternType::Path => context.relative_path == rule.pattern,
        PatternType::Prefix => context.relative_path.starts_with(&rule.pattern),
        PatternType::Basename => context.name == rule.pattern,
        PatternType::Suffix => context.name.ends_with(&rule.pattern),
        PatternType::Glob => {
            if rule.pattern.contains('/') {
                glob_match(rule.pattern.as_bytes(), context.relative_path.as_bytes())
            } else {
                glob_match(rule.pattern.as_bytes(), context.name.as_bytes())
            }
        }
    }
}
