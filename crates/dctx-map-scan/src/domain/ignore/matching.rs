//! Ordered rule matching and negation.
//! Implements SPEC-MAP-MODE section 6.1 and ADR-014/015b.
//! Uses standard-library types; consumed by scan clients.

use super::glob::glob_match;
use super::{IgnoreRule, PatternType};
use std::{cell::RefCell, collections::HashMap};

/// Exact pattern/text results, bounded to 10,000 entries by the matcher.
/// Pattern identity prevents stale results when callers reuse a cache.
pub type MatchCache = RefCell<HashMap<(String, String), bool>>;

const MAX_CACHE_ENTRIES: usize = 10_000;

/// Evaluates ordered rules without caching.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn should_ignore_item(
    item_relative_path: &str,
    item_name: &str,
    is_item_dir: bool,
    rules: &[IgnoreRule],
    debug_verbose: bool,
) -> bool {
    should_ignore_item_cached(
        item_relative_path,
        item_name,
        is_item_dir,
        rules,
        debug_verbose,
        None,
    )
}

/// Evaluates ordered rules using an optional bounded glob cache.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn should_ignore_item_cached(
    item_relative_path: &str,
    item_name: &str,
    is_item_dir: bool,
    rules: &[IgnoreRule],
    _debug_verbose: bool,
    cache: Option<&MatchCache>,
) -> bool {
    if rules.is_empty() {
        return false;
    }

    let mut is_ignored = false;

    for rule in rules {
        if rule.is_dir_only && !is_item_dir {
            continue;
        }

        let matched = match rule.pattern_type {
            PatternType::Invalid => false,
            PatternType::Path => item_relative_path == rule.pattern,
            PatternType::Prefix => item_relative_path.starts_with(&rule.pattern),
            PatternType::Basename => item_name == rule.pattern,
            PatternType::Suffix => item_name.ends_with(&rule.pattern),
            PatternType::Glob => {
                let text = if rule.pattern.contains('/') {
                    item_relative_path
                } else {
                    item_name
                };

                if let Some(cache_ref) = cache
                    && let Ok(mut cache_map) = cache_ref.try_borrow_mut()
                {
                    let key = (rule.pattern.clone(), text.to_owned());
                    if let Some(result) = cache_map.get(&key) {
                        *result
                    } else {
                        let result = glob_match(rule.pattern.as_bytes(), text.as_bytes());
                        if cache_map.len() < MAX_CACHE_ENTRIES {
                            cache_map.insert(key, result);
                        }
                        result
                    }
                } else {
                    glob_match(rule.pattern.as_bytes(), text.as_bytes())
                }
            }
        };

        if matched {
            is_ignored = !rule.is_negation;
        }
    }

    is_ignored
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ignore::parse_ignore_pattern_line;

    #[test]
    fn reused_cache_keys_include_pattern_identity() {
        let cache = MatchCache::default();
        let ignored = [parse_ignore_pattern_line("**/*.secret").unwrap()];
        let omitted = [parse_ignore_pattern_line("**/*.lock").unwrap()];
        assert!(!should_ignore_item_cached(
            "deps.lock",
            "deps.lock",
            false,
            &ignored,
            false,
            Some(&cache)
        ));
        assert!(should_ignore_item_cached(
            "deps.lock",
            "deps.lock",
            false,
            &omitted,
            false,
            Some(&cache)
        ));
        assert_eq!(cache.borrow().len(), 2);
    }

    #[test]
    fn cache_bound_and_borrow_contention_do_not_change_results() {
        let cache = MatchCache::default();
        let rules = [parse_ignore_pattern_line("**/*.log").unwrap()];
        for i in 0..=MAX_CACHE_ENTRIES {
            let path = format!("logs/{i}.log");
            assert!(should_ignore_item_cached(
                &path,
                &path,
                false,
                &rules,
                false,
                Some(&cache)
            ));
        }
        assert_eq!(cache.borrow().len(), MAX_CACHE_ENTRIES);
        let held_borrow = cache.borrow_mut();
        assert!(should_ignore_item_cached(
            "error.log",
            "error.log",
            false,
            &rules,
            false,
            Some(&cache)
        ));
        assert!(!should_ignore_item_cached(
            "main.rs",
            "main.rs",
            false,
            &rules,
            false,
            Some(&cache)
        ));
        drop(held_borrow);
    }
}
