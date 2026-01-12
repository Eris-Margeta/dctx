// crates/dctx-core/src/template.rs

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Template {
    pub variables: HashMap<String, String>,
    pub partial_tree_entry: String,
    pub partial_file_block: String,
    pub layout: String,
}

impl Template {
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

            // CRITICAL FIX: Only change section if it explicitly matches a known header.
            // Previously, [DIRECTORY:...] content was triggering this and breaking the parser.
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
                        let val = val.trim().trim_matches('"').to_string();
                        // Interpret \n as newline
                        let val = val.replace("\\n", "\n");
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

        // Pass 1: Resolve internal variables (e.g., instructions_text using markers)
        // We do 2 passes to handle nested references.
        for _ in 0..2 {
            let keys: Vec<String> = variables.keys().cloned().collect();
            for key in keys {
                let val = variables[&key].clone();
                let resolved = replace_vars(&val, &variables);
                variables.insert(key, resolved);
            }
        }

        // Clean up partials (trim trailing newlines from accumulation)
        Self {
            variables,
            partial_tree_entry: partial_tree.trim().to_string(),
            partial_file_block: partial_file.trim().to_string(),
            layout: layout.trim().to_string(),
        }
    }

    /// Inject an external variable (like project_name) into the template context.
    pub fn insert_variable(&mut self, key: &str, value: &str) {
        self.variables.insert(key.to_string(), value.to_string());
    }
}

/// Helper to replace {{VAR}} with values from a map.
pub fn replace_vars(text: &str, vars: &HashMap<String, String>) -> String {
    let mut result = text.to_string();
    // Optimization: Check if text actually contains "{{" before iterating map
    if !result.contains("{{") {
        return result;
    }

    for (key, val) in vars {
        let pattern = format!("{{{{{}}}}}", key);
        result = result.replace(&pattern, val);
    }
    result
}
