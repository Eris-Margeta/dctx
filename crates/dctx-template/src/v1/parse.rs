//! V1 source parser and `{{var}}` substitution.
//! Implements ADR-003 two-pass resolution and contracts/DTS-v2.md variable cycles.
//! Depends on [`crate::error`]. Used by [`super::compiled`] and the v2 source extractor.
//! Comment and blank lines are skipped in every section, matching the shipped parser.

use std::collections::{HashMap, HashSet};

use crate::error::TemplateValidationError;

/// A parsed v1 template. Variables are already resolved; partials and layout are not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// Resolved configuration variables.
    pub variables: HashMap<String, String>,
    /// Tree-entry partial. `{{var}}` is resolved later by [`Template::compile`].
    pub partial_tree_entry: String,
    /// File-block partial.
    pub partial_file_block: String,
    /// Document layout.
    pub layout: String,
    /// Variable names that participate in a reference cycle.
    pub cycles: Vec<String>,
}

impl Template {
    /// Parses v1 source. Unknown headers do not change the current section.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn parse(raw: &str) -> Self {
        let mut variables = HashMap::new();
        let mut partial_tree = String::new();
        let mut partial_file = String::new();
        let mut layout = String::new();
        let mut current_section = "";

        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            }
            if trimmed == "[VARIABLES]"
                || trimmed == "[PARTIAL:TREE_ENTRY]"
                || trimmed == "[PARTIAL:FILE_BLOCK]"
                || trimmed == "[LAYOUT]"
            {
                current_section = trimmed;
                continue;
            }
            match current_section {
                "[VARIABLES]" => {
                    if let Some((key, val)) = trimmed.split_once('=') {
                        let key = key.trim().to_string();
                        let val = val.trim().trim_matches('"').replace("\\n", "\n");
                        variables.insert(key, val);
                    }
                }
                "[PARTIAL:TREE_ENTRY]" => {
                    partial_tree.push_str(line);
                    partial_tree.push('\n');
                }
                "[PARTIAL:FILE_BLOCK]" => {
                    partial_file.push_str(line);
                    partial_file.push('\n');
                }
                "[LAYOUT]" => {
                    layout.push_str(line);
                    layout.push('\n');
                }
                _ => {}
            }
        }

        finish(variables, &partial_tree, &partial_file, &layout)
    }

    /// Inserts a runtime variable such as `project`.
    pub fn insert_variable(&mut self, key: &str, value: &str) {
        self.variables.insert(key.to_string(), value.to_string());
    }

    /// Reports empty layouts, unresolved `{{var}}` references and variable cycles.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn validate(&self) -> Vec<TemplateValidationError> {
        let mut errors = Vec::new();
        if self.layout.is_empty() {
            errors.push(TemplateValidationError::EmptySection("[LAYOUT]".to_string()));
        }
        for name in &self.cycles {
            errors.push(TemplateValidationError::Cycle(name.clone()));
        }
        collect_unresolved(&self.partial_tree_entry, &self.variables, &mut errors);
        collect_unresolved(&self.partial_file_block, &self.variables, &mut errors);
        collect_unresolved(&self.layout, &self.variables, &mut errors);
        errors
    }
}

pub(crate) fn finish(
    mut variables: HashMap<String, String>,
    partial_tree: &str,
    partial_file: &str,
    layout: &str,
) -> Template {
    let raw = variables.clone();
    for _ in 0..2 {
        let keys: Vec<String> = variables.keys().cloned().collect();
        for key in keys {
            let Some(val) = variables.get(&key).cloned() else {
                continue;
            };
            let resolved = replace_vars(&val, &variables);
            variables.insert(key, resolved);
        }
    }
    Template {
        variables,
        partial_tree_entry: partial_tree.trim().to_string(),
        partial_file_block: partial_file.trim().to_string(),
        layout: layout.trim().to_string(),
        cycles: find_cycles(&raw),
    }
}

/// Replaces `{{key}}` using `HashMap` iteration order, matching the v1 renderer.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
#[allow(clippy::implicit_hasher, reason = "Keep the public v1 HashMap signature and its documented default-hasher iteration contract; generalizing it would change the API.")]
pub fn replace_vars(text: &str, vars: &HashMap<String, String>) -> String {
    let mut result = text.to_string();
    if !result.contains("{{") {
        return result;
    }
    for (key, val) in vars {
        let pattern = format!("{{{{{key}}}}}");
        result = result.replace(&pattern, val);
    }
    result
}

/// Replaces `{{key}}` in the given order. V2 rendering uses this for stable bytes.
pub fn replace_vars_in_order(text: &str, vars: &[(String, String)]) -> String {
    let mut result = text.to_string();
    if !result.contains("{{") {
        return result;
    }
    for (key, val) in vars {
        let pattern = format!("{{{{{key}}}}}");
        result = result.replace(&pattern, val);
    }
    result
}

fn collect_unresolved(text: &str, vars: &HashMap<String, String>, errors: &mut Vec<TemplateValidationError>) {
    for name in brace_names(text) {
        if !name.is_empty() && !name.contains('{') && !name.contains('}') && !vars.contains_key(&name) {
            errors.push(TemplateValidationError::UnresolvedVariable(name));
        }
    }
}

/// Names inside `{{...}}`, skipping `{{{data}}}` placeholders.
pub(crate) fn brace_names(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut names = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if bytes.get(i + 2) == Some(&b'{') {
                if let Some(end) = text[i + 3..].find("}}}") {
                    i += 3 + end + 3;
                    continue;
                }
                i += 3;
                continue;
            }
            if let Some(end) = text[i + 2..].find("}}") {
                names.push(text[i + 2..i + 2 + end].to_string());
                i += 2 + end + 2;
                continue;
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    names
}

fn find_cycles(vars: &HashMap<String, String>) -> Vec<String> {
    let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
    for (key, value) in vars {
        let deps: Vec<&str> = brace_names(value)
            .into_iter()
            .filter(|name| vars.contains_key(name))
            .map(|name| {
                vars.get_key_value(&name).map_or("", |(k, _)| k.as_str())
            })
            .filter(|name| !name.is_empty())
            .collect();
        edges.insert(key.as_str(), deps);
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut cycles = Vec::new();
    let mut keys: Vec<&str> = edges.keys().copied().collect();
    keys.sort_unstable();
    for key in keys {
        walk(key, &edges, &mut visiting, &mut visited, &mut cycles);
    }
    cycles.sort();
    cycles.dedup();
    cycles
}

fn walk(
    key: &str,
    edges: &HashMap<&str, Vec<&str>>,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
    cycles: &mut Vec<String>,
) {
    if visited.contains(key) {
        return;
    }
    if !visiting.insert(key.to_string()) {
        cycles.push(key.to_string());
        return;
    }
    if let Some(next) = edges.get(key) {
        for dep in next {
            walk(dep, edges, visiting, visited, cycles);
        }
    }
    visiting.remove(key);
    visited.insert(key.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_passes_resolve_a_chain_and_a_cycle_is_recorded() {
        let raw = "[VARIABLES]\na = \"A\"\nb = \"{{a}}B\"\nc = \"{{b}}C\"\n[LAYOUT]\nx\n";
        let template = Template::parse(raw);
        assert_eq!(template.variables.get("c").map(String::as_str), Some("ABC"));
        assert_eq!(template.cycles, [] as [std::string::String; 0]);

        let cyclic = "[VARIABLES]\na = \"{{b}}\"\nb = \"{{a}}\"\n[LAYOUT]\n{{a}}\n";
        let template = Template::parse(cyclic);
        assert_ne!(template.cycles, [] as [std::string::String; 0]);
        assert!(template.validate().iter().any(|err| matches!(err, TemplateValidationError::Cycle(_))));
    }
}
