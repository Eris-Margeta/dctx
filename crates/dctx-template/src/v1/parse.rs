// SPDX-License-Identifier: Apache-2.0
//! V1 source parser and `{{var}}` substitution.
//! Implements ADR-003 two-pass resolution and contracts/DTS-v2.md variable cycles.
//! Depends on [`crate::error`]. Used by [`super::compiled`] and the v2 source extractor.
//! Comment and blank lines are skipped in every section, matching the shipped parser.

use std::collections::{HashMap, HashSet};

use crate::error::{MAX_PLAN_BYTES, MAX_STRING_BYTES, TemplateError, TemplateValidationError};

/// A parsed v1 template. Valid variables are resolved; partials and layout are not.
/// Call [`Template::validate`] before rendering to reject retained invalid variable definitions.
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
    /// Repeated variables keep the last value; repeated sections append their bodies.
    /// Substitution uses lexical key order in two passes, and cycles are recorded before expansion.
    /// This legacy API has no source-size cap. Variable expansion is bounded to 65,536 bytes
    /// per value and 1 MiB in total; on overflow raw values are retained and [`Template::validate`]
    /// reports an invalid-syntax error. Use [`crate::compile`] for bounded source admission.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn parse(raw: &str) -> Self {
        let template = Self::parse_unresolved(raw);
        if variable_bytes(&template.variables).is_err() {
            return template;
        }
        finish_bounded(
            template.variables.clone(),
            &template.partial_tree_entry,
            &template.partial_file_block,
            &template.layout,
        )
        .unwrap_or(template)
    }

    pub(crate) fn parse_bounded(raw: &str) -> Result<Self, TemplateError> {
        let template = Self::parse_unresolved(raw);
        finish_bounded(
            template.variables,
            &template.partial_tree_entry,
            &template.partial_file_block,
            &template.layout,
        )
    }

    fn parse_unresolved(raw: &str) -> Self {
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

        Self {
            variables,
            partial_tree_entry: partial_tree.trim().to_string(),
            partial_file_block: partial_file.trim().to_string(),
            layout: layout.trim().to_string(),
            cycles: Vec::new(),
        }
    }

    /// Inserts or replaces a runtime variable such as `project`, without expanding its value.
    /// Existing resolved values and recorded cycles are unchanged; parse again to resolve new dependencies.
    pub fn insert_variable(&mut self, key: &str, value: &str) {
        self.variables.insert(key.to_string(), value.to_string());
    }

    /// Reports empty layouts, unresolved `{{var}}` references and recorded variable cycles.
    /// Runtime `{{{field}}}` placeholders are left to rendering. No expressions are evaluated.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn validate(&self) -> Vec<TemplateValidationError> {
        let mut errors = Vec::new();
        if self.layout.is_empty() {
            errors.push(TemplateValidationError::EmptySection(
                "[LAYOUT]".to_string(),
            ));
        }
        for name in &self.cycles {
            errors.push(TemplateValidationError::Cycle(name.clone()));
        }
        if self.cycles.is_empty() {
            let expansion = variable_bytes(&self.variables)
                .and_then(|_| finish_bounded(self.variables.clone(), "", "", "").map(|_| ()));
            if let Err(error) = expansion {
                errors.push(TemplateValidationError::InvalidSyntax(error.to_string()));
            }
        }
        collect_unresolved(&self.partial_tree_entry, &self.variables, &mut errors);
        collect_unresolved(&self.partial_file_block, &self.variables, &mut errors);
        collect_unresolved(&self.layout, &self.variables, &mut errors);
        errors
    }
}

pub(crate) fn finish_bounded(
    mut variables: HashMap<String, String>,
    partial_tree: &str,
    partial_file: &str,
    layout: &str,
) -> Result<Template, TemplateError> {
    let mut variable_bytes = variable_bytes(&variables)?;
    let cycles = find_cycles(&variables);
    for _ in 0..if cycles.is_empty() { 2 } else { 0 } {
        let mut keys: Vec<_> = variables.keys().cloned().collect();
        keys.sort();
        for key in keys {
            if let Some(value) = variables.get(&key) {
                let resolved = replace_vars_with_limit(value, &variables, MAX_STRING_BYTES)?;
                variable_bytes = variable_bytes - value.len() + resolved.len();
                if variable_bytes > MAX_PLAN_BYTES as usize {
                    return Err(TemplateError::Limit {
                        what: "variable expansion",
                        limit: u64::from(MAX_PLAN_BYTES),
                    });
                }
                variables.insert(key, resolved);
            }
        }
    }
    Ok(Template {
        variables,
        partial_tree_entry: partial_tree.trim().to_string(),
        partial_file_block: partial_file.trim().to_string(),
        layout: layout.trim().to_string(),
        cycles,
    })
}

fn variable_bytes(variables: &HashMap<String, String>) -> Result<usize, TemplateError> {
    // Choose the same refusal regardless of hash-map iteration order when both caps fail.
    if variables
        .iter()
        .any(|(key, value)| key.len() > MAX_STRING_BYTES || value.len() > MAX_STRING_BYTES)
    {
        return Err(TemplateError::Limit {
            what: "variable expansion",
            limit: MAX_STRING_BYTES as u64,
        });
    }
    let mut total = 0usize;
    for (key, value) in variables {
        total = total.saturating_add(key.len()).saturating_add(value.len());
    }
    if total > MAX_PLAN_BYTES as usize {
        return Err(TemplateError::Limit {
            what: "variable expansion",
            limit: u64::from(MAX_PLAN_BYTES),
        });
    }
    Ok(total)
}

pub(crate) fn replace_vars_bounded(
    text: &str,
    vars: &HashMap<String, String>,
) -> Result<String, TemplateError> {
    replace_vars_with_limit(text, vars, MAX_PLAN_BYTES as usize)
}

fn replace_vars_with_limit(
    text: &str,
    vars: &HashMap<String, String>,
    limit: usize,
) -> Result<String, TemplateError> {
    let too_large = || TemplateError::Limit {
        what: "variable expansion",
        limit: limit as u64,
    };
    if text.len() > limit {
        return Err(too_large());
    }
    let mut result = text.to_string();
    if !result.contains("{{") {
        return Ok(result);
    }
    let mut entries: Vec<_> = vars.iter().collect();
    entries.sort_by_key(|(key, _)| *key);
    for (key, value) in entries {
        let pattern = format!("{{{{{key}}}}}");
        let mut size = 0usize;
        for (part, substitute) in variable_parts(&result) {
            let count = if substitute {
                part.matches(&pattern).count()
            } else {
                0
            };
            let len = (part.len() - count * pattern.len())
                .saturating_add(count.saturating_mul(value.len()));
            size = size.saturating_add(len);
            if size > limit {
                return Err(too_large());
            }
        }
        result = replace_variable(&result, &pattern, value);
    }
    Ok(result)
}

/// Replaces `{{key}}` in lexical key order. Triple-braced runtime fields remain literal.
/// Each key is applied once; inserted references to later keys resolve in the same pass.
/// This compatibility helper has no output-size cap; use [`crate::compile`] for untrusted source.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
#[allow(
    clippy::implicit_hasher,
    reason = "Keep the public v1 HashMap signature; substitution sorts keys independently of the hasher."
)]
pub fn replace_vars(text: &str, vars: &HashMap<String, String>) -> String {
    let mut result = text.to_string();
    if !result.contains("{{") {
        return result;
    }
    let mut entries: Vec<_> = vars.iter().collect();
    entries.sort_by_key(|(key, _)| *key);
    for (key, val) in entries {
        let pattern = format!("{{{{{key}}}}}");
        result = replace_variable(&result, &pattern, val);
    }
    result
}

// A configuration variable must not match the inner braces of a data field,
// particularly `{{{project}}}` after the caller supplies the project name.
fn replace_variable(text: &str, pattern: &str, value: &str) -> String {
    let mut output = String::new();
    for (part, substitute) in variable_parts(text) {
        if substitute {
            output.push_str(&part.replace(pattern, value));
        } else {
            output.push_str(part);
        }
    }
    output
}

fn variable_parts(mut text: &str) -> impl Iterator<Item = (&str, bool)> {
    std::iter::from_fn(move || {
        if text.is_empty() {
            return None;
        }
        let (end, substitute) = match text.find("{{{") {
            Some(0) => (text.find("}}}").map_or(text.len(), |end| end + 3), false),
            Some(start) => (start, true),
            None => (text.len(), true),
        };
        let (part, rest) = text.split_at(end);
        text = rest;
        Some((part, substitute))
    })
}

fn collect_unresolved(
    text: &str,
    vars: &HashMap<String, String>,
    errors: &mut Vec<TemplateValidationError>,
) {
    for name in brace_names(text) {
        if !name.is_empty()
            && !name.contains('{')
            && !name.contains('}')
            && !vars.contains_key(&name)
        {
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
            .map(|name| vars.get_key_value(&name).map_or("", |(k, _)| k.as_str()))
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
    // Keep DFS state on the heap: user-controlled variable chains must not
    // consume one call frame per dependency. Reverse pushes preserve visit order.
    let mut pending = vec![(key, false)];
    while let Some((key, leaving)) = pending.pop() {
        if leaving {
            visiting.remove(key);
            visited.insert(key.to_string());
            continue;
        }
        if visited.contains(key) {
            continue;
        }
        if !visiting.insert(key.to_string()) {
            cycles.push(key.to_string());
            continue;
        }
        pending.push((key, true));
        if let Some(next) = edges.get(key) {
            pending.extend(next.iter().rev().map(|dep| (*dep, false)));
        }
    }
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
        assert!(
            template
                .validate()
                .iter()
                .any(|err| matches!(err, TemplateValidationError::Cycle(_)))
        );
    }

    #[test]
    fn robustness_deep_variable_dependencies() {
        let mut vars = HashMap::new();
        for index in 0..20_000 {
            vars.insert(format!("v{index:05}"), format!("{{{{v{:05}}}}}", index + 1));
        }
        vars.insert("v20000".into(), "done".into());
        assert!(find_cycles(&vars).is_empty());
        vars.insert("v20000".into(), "{{v00000}}".into());
        assert_eq!(find_cycles(&vars), vec!["v00000".to_owned()]);
    }
}
