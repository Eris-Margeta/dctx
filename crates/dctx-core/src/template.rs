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

#[cfg(test)]
mod tests {
    use super::*;

    // --- REPLACE_VARS TESTS ---

    #[test]
    fn test_replace_vars_simple() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "World".to_string());
        let result = replace_vars("Hello, {{name}}!", &vars);
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_replace_vars_multiple() {
        let mut vars = HashMap::new();
        vars.insert("first".to_string(), "Hello".to_string());
        vars.insert("second".to_string(), "World".to_string());
        let result = replace_vars("{{first}}, {{second}}!", &vars);
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_replace_vars_no_vars() {
        let vars = HashMap::new();
        let result = replace_vars("No variables here", &vars);
        assert_eq!(result, "No variables here");
    }

    #[test]
    fn test_replace_vars_missing_var() {
        let vars = HashMap::new();
        let result = replace_vars("{{missing}} variable", &vars);
        assert_eq!(result, "{{missing}} variable"); // Should remain unchanged
    }

    #[test]
    fn test_replace_vars_repeated_var() {
        let mut vars = HashMap::new();
        vars.insert("x".to_string(), "Y".to_string());
        let result = replace_vars("{{x}} and {{x}} again", &vars);
        assert_eq!(result, "Y and Y again");
    }

    #[test]
    fn test_replace_vars_optimization_no_braces() {
        let mut vars = HashMap::new();
        vars.insert("unused".to_string(), "value".to_string());
        let result = replace_vars("No braces here", &vars);
        assert_eq!(result, "No braces here");
    }

    // --- TEMPLATE PARSE TESTS ---

    #[test]
    fn test_template_parse_variables() {
        let raw = r#"
[VARIABLES]
key1 = "value1"
key2 = "value2"
[LAYOUT]
"#;
        let tmpl = Template::parse(raw);
        assert_eq!(tmpl.variables.get("key1"), Some(&"value1".to_string()));
        assert_eq!(tmpl.variables.get("key2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_template_parse_newline_escape() {
        let raw = r#"
[VARIABLES]
multiline = "line1\nline2"
[LAYOUT]
"#;
        let tmpl = Template::parse(raw);
        assert_eq!(tmpl.variables.get("multiline"), Some(&"line1\nline2".to_string()));
    }

    #[test]
    fn test_template_parse_variable_references() {
        let raw = r#"
[VARIABLES]
prefix = "START"
full = "{{prefix}}_END"
[LAYOUT]
"#;
        let tmpl = Template::parse(raw);
        assert_eq!(tmpl.variables.get("full"), Some(&"START_END".to_string()));
    }

    #[test]
    fn test_template_parse_partial_tree() {
        let raw = r#"
[VARIABLES]
[PARTIAL:TREE_ENTRY]
{{{node.type_char}}} {{{node.path}}}
[LAYOUT]
"#;
        let tmpl = Template::parse(raw);
        assert!(tmpl.partial_tree_entry.contains("{{{node.type_char}}}"));
        assert!(tmpl.partial_tree_entry.contains("{{{node.path}}}"));
    }

    #[test]
    fn test_template_parse_partial_file() {
        let raw = r#"
[VARIABLES]
[PARTIAL:FILE_BLOCK]
<FILE id="{{{file.id}}}">
{{{file.content}}}
</FILE>
[LAYOUT]
"#;
        let tmpl = Template::parse(raw);
        assert!(tmpl.partial_file_block.contains("{{{file.id}}}"));
        assert!(tmpl.partial_file_block.contains("{{{file.content}}}"));
    }

    #[test]
    fn test_template_parse_layout() {
        let raw = r#"
[VARIABLES]
[LAYOUT]
=== TREE ===
{{{comp.tree}}}
=== FILES ===
{{{comp.files}}}
"#;
        let tmpl = Template::parse(raw);
        assert!(tmpl.layout.contains("{{{comp.tree}}}"));
        assert!(tmpl.layout.contains("{{{comp.files}}}"));
    }

    #[test]
    fn test_template_parse_ignores_comments() {
        let raw = r#"
# This is a comment
[VARIABLES]
# Another comment
key = "value"
[LAYOUT]
# Comment in layout section is NOT ignored (it's content)
"#;
        let tmpl = Template::parse(raw);
        assert_eq!(tmpl.variables.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_template_parse_ignores_empty_lines() {
        let raw = r#"
[VARIABLES]

key = "value"

[LAYOUT]
"#;
        let tmpl = Template::parse(raw);
        assert_eq!(tmpl.variables.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_template_parse_content_with_brackets() {
        // This tests the critical fix: [DIRECTORY:...] should NOT be treated as a section header
        let raw = r#"
[VARIABLES]
marker = "[DIRECTORY:{{project}}]"
[LAYOUT]
{{{marker}}}
{{{comp.tree}}}
"#;
        let tmpl = Template::parse(raw);
        assert!(tmpl.layout.contains("{{{marker}}}"));
        assert!(tmpl.layout.contains("{{{comp.tree}}}"));
    }

    #[test]
    fn test_template_insert_variable() {
        let raw = r#"
[VARIABLES]
existing = "value"
[LAYOUT]
"#;
        let mut tmpl = Template::parse(raw);
        tmpl.insert_variable("project", "MyProject");

        assert_eq!(tmpl.variables.get("project"), Some(&"MyProject".to_string()));
        assert_eq!(tmpl.variables.get("existing"), Some(&"value".to_string()));
    }

    #[test]
    fn test_template_insert_variable_overwrites() {
        let raw = r#"
[VARIABLES]
key = "old"
[LAYOUT]
"#;
        let mut tmpl = Template::parse(raw);
        tmpl.insert_variable("key", "new");

        assert_eq!(tmpl.variables.get("key"), Some(&"new".to_string()));
    }

    #[test]
    fn test_template_nested_variable_resolution() {
        let raw = r#"
[VARIABLES]
a = "A"
b = "{{a}}B"
c = "{{b}}C"
[LAYOUT]
"#;
        let tmpl = Template::parse(raw);
        // After 2 passes of resolution
        assert_eq!(tmpl.variables.get("c"), Some(&"ABC".to_string()));
    }

    #[test]
    fn test_template_parse_full_example() {
        let raw = r#"
[VARIABLES]
tree_start = "<TREE>"
tree_end = "</TREE>"
file_start = "<FILE>"
file_end = "</FILE>"

[PARTIAL:TREE_ENTRY]
{{{node.type_char}}} {{{node.path}}} (ID: {{{node.id}}})

[PARTIAL:FILE_BLOCK]
{{{file_start}}} id="{{{file.id}}}"
{{{file.content}}}
{{{file_end}}}

[LAYOUT]
{{{tree_start}}}
{{{comp.tree}}}
{{{tree_end}}}

{{{comp.files}}}
"#;
        let tmpl = Template::parse(raw);

        // Check variables were resolved
        assert_eq!(tmpl.variables.get("tree_start"), Some(&"<TREE>".to_string()));
        assert_eq!(tmpl.variables.get("file_end"), Some(&"</FILE>".to_string()));

        // Check partials contain expected placeholders
        assert!(tmpl.partial_tree_entry.contains("{{{node.type_char}}}"));
        assert!(tmpl.partial_file_block.contains("{{{file.content}}}"));

        // Check layout structure
        assert!(tmpl.layout.contains("{{{tree_start}}}"));
        assert!(tmpl.layout.contains("{{{comp.tree}}}"));
        assert!(tmpl.layout.contains("{{{comp.files}}}"));
    }
}
