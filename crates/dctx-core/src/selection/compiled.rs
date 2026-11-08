// SPDX-License-Identifier: Apache-2.0
//! Candidate indexes preserve the greatest matching rule position. The original
//! matcher remains the judge for every non-exact candidate and for explanations.
use super::rule::rule_matches;
use crate::datatypes::{IgnoreRule, PatternType};
use crate::domain::ItemContext;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub(super) struct CompiledRules {
    names: HashMap<String, [Option<usize>; 2]>,
    paths: HashMap<String, [Option<usize>; 2]>,
    suffixes: HashMap<u8, Vec<usize>>,
    other: Vec<usize>,
}

impl CompiledRules {
    pub(super) fn new(rules: &[IgnoreRule]) -> Self {
        let mut compiled = Self::default();
        for (index, rule) in rules.iter().enumerate() {
            match rule.pattern_type {
                PatternType::Basename | PatternType::Path => {
                    let (table, pattern) = if rule.pattern_type == PatternType::Basename {
                        (&mut compiled.names, rule.pattern.as_str())
                    } else {
                        (
                            &mut compiled.paths,
                            rule.pattern.strip_prefix('/').unwrap_or(&rule.pattern),
                        )
                    };
                    let slots = table.entry(pattern.to_string()).or_insert([None; 2]);
                    if !rule.is_dir_only {
                        slots[0] = Some(index);
                    }
                    slots[1] = Some(index);
                }
                PatternType::Suffix if !rule.pattern.is_empty() => {
                    if let Some(last) = rule.pattern.as_bytes().last() {
                        compiled.suffixes.entry(*last).or_default().push(index);
                    }
                }
                _ => compiled.other.push(index),
            }
        }
        compiled
    }

    pub(super) fn matches(&self, rules: &[IgnoreRule], context: &ItemContext<'_>) -> bool {
        let slot = usize::from(context.is_directory);
        let mut last = self
            .names
            .get(context.name)
            .and_then(|slots| slots[slot])
            .max(
                self.paths
                    .get(context.relative_path)
                    .and_then(|slots| slots[slot]),
            );
        let suffixes = context
            .name
            .as_bytes()
            .last()
            .and_then(|last| self.suffixes.get(last));
        for index in self.other.iter().chain(suffixes.into_iter().flatten()) {
            if last.is_none_or(|last| *index > last)
                && rule_matches(
                    &rules[*index],
                    context.relative_path,
                    context.name,
                    context.is_directory,
                )
            {
                last = Some(*index);
            }
        }
        last.is_some_and(|index| !rules[index].is_negation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::rule::{last_match, parse_rules};

    #[test]
    fn candidates_preserve_last_match_directory_flags_and_empty_patterns() {
        let lines = [
            "a",
            "!a/",
            "*.rs",
            "!a.rs",
            "/sub/a.rs",
            "!sub/a.rs/",
            "sub/*",
            "!**/keep.rs",
            "*",
            "!",
            "*/",
            "!*.rs/",
            "sub/?",
            "!*.log",
        ];
        for rotation in 0..lines.len() {
            let text = lines[rotation..]
                .iter()
                .chain(&lines[..rotation])
                .copied()
                .collect::<Vec<_>>()
                .join("\n");
            let rules = parse_rules(&text);
            let compiled = CompiledRules::new(&rules);
            for path in [
                "",
                "a",
                "a.rs",
                "sub/a.rs",
                "sub/keep.rs",
                "sub/a",
                "sub/a.log",
                "é.rs",
                "sub/deep/keep.rs",
            ] {
                for is_directory in [false, true] {
                    let context = ItemContext {
                        relative_path: path,
                        name: path.rsplit('/').next().unwrap(),
                        is_directory,
                        debug_mode: false,
                    };
                    assert_eq!(
                        compiled.matches(&rules, &context),
                        last_match(&rules, path, context.name, is_directory),
                        "{text}: {path}, {is_directory}"
                    );
                }
            }
        }
    }
}
