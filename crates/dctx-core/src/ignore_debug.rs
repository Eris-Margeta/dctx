// Debug utilities for ignore module
// Use with: DCTX_DEBUG=1 dctx . -c

use crate::datatypes::IgnoreRule;
use std::env;

pub fn is_debug_enabled() -> bool {
    env::var("DCTX_DEBUG").unwrap_or_default() == "1"
}

pub fn debug_print(msg: &str) {
    if is_debug_enabled() {
        eprintln!("[DEBUG] {}", msg);
    }
}

pub fn debug_rules(rules: &[IgnoreRule]) {
    if !is_debug_enabled() {
        return;
    }

    eprintln!("\n========== LOADED IGNORE RULES ==========");
    eprintln!("Total rules: {}", rules.len());

    let mut hardcoded_count = 0;
    let mut global_count = 0;
    let mut project_count = 0;

    for (idx, rule) in rules.iter().enumerate() {
        let source = if idx < 15 {
            hardcoded_count += 1;
            "HARDCODED"
        } else if idx < 15 + global_count {
            global_count += 1;
            "GLOBAL   "
        } else {
            project_count += 1;
            "PROJECT  "
        };

        eprintln!(
            "[{}] {} {:?} | pattern=\"{}\" dir_only={} negation={}",
            idx,
            source,
            rule.pattern_type,
            rule.pattern,
            rule.is_dir_only,
            rule.is_negation
        );
    }

    eprintln!("========================================\n");
}

pub fn debug_ignore_check(
    item_path: &str,
    item_name: &str,
    is_dir: bool,
    matched_rule: Option<&IgnoreRule>,
    result: bool,
) {
    if !is_debug_enabled() {
        return;
    }

    let type_str = if is_dir { "DIR " } else { "FILE" };
    let result_str = if result { "IGNORED" } else { "KEPT   " };

    if let Some(rule) = matched_rule {
        eprintln!(
            "[MATCH] {} {} | {} | matched by: \"{}\" (type={:?}, negation={})",
            type_str, result_str, item_path, rule.pattern, rule.pattern_type, rule.is_negation
        );
    } else if is_debug_enabled() {
        eprintln!("[MATCH] {} {} | {} | no rules matched", type_str, result_str, item_path);
    }
}
