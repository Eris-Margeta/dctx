// crates/dctx-core/src/template.rs

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

// Magic bytes for compiled template files
const COMPILED_TEMPLATE_MAGIC: &[u8; 8] = b"DCTXTMPL";
const COMPILED_TEMPLATE_VERSION: u32 = 1;

/// Validation errors that can occur when parsing/validating a template.
#[derive(Debug, Clone)]
pub enum TemplateValidationError {
    MissingSection(String),
    EmptySection(String),
    UnresolvedVariable(String),
    InvalidSyntax(String),
}

impl std::fmt::Display for TemplateValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSection(s) => write!(f, "Missing required section: {}", s),
            Self::EmptySection(s) => write!(f, "Section is empty: {}", s),
            Self::UnresolvedVariable(s) => write!(f, "Unresolved variable: {{{{{}}}}}", s),
            Self::InvalidSyntax(s) => write!(f, "Invalid syntax: {}", s),
        }
    }
}

/// A compiled (pre-processed) template ready for runtime use.
/// All {{VAR}} references have been resolved during the build phase.
/// Only {{{DATA}}} placeholders remain for runtime injection.
#[derive(Debug, Clone)]
pub struct CompiledTemplate {
    /// Template name (derived from filename)
    pub name: String,
    /// Pre-resolved variables (kept for reference, mostly resolved into partials/layout)
    pub variables: HashMap<String, String>,
    /// Tree entry partial with {{VAR}} already resolved
    pub partial_tree_entry: String,
    /// File block partial with {{VAR}} already resolved
    pub partial_file_block: String,
    /// Layout with {{VAR}} already resolved
    pub layout: String,
}

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

    /// Validates the template structure.
    /// Returns a list of validation errors (empty if valid).
    pub fn validate(&self) -> Vec<TemplateValidationError> {
        let mut errors = Vec::new();

        // Check required sections exist and are non-empty
        if self.layout.is_empty() {
            errors.push(TemplateValidationError::EmptySection(
                "[LAYOUT]".to_string(),
            ));
        }

        // Check for unresolved {{VAR}} references in partials and layout
        // A {{VAR}} is unresolved if it doesn't exist in the variables map
        // IMPORTANT: We must skip {{{DATA}}} patterns (triple braces) which are runtime data
        let check_unresolved =
            |text: &str,
             vars: &HashMap<String, String>,
             errors: &mut Vec<TemplateValidationError>| {
                let mut i = 0;
                let chars: Vec<char> = text.chars().collect();
                while i < chars.len().saturating_sub(4) {
                    // Check for {{ pattern
                    if chars[i] == '{' && chars[i + 1] == '{' {
                        // Skip if this is {{{ (triple brace - data placeholder)
                        if chars.get(i + 2) == Some(&'{') {
                            // This is a {{{ data placeholder, skip past it
                            // Find the closing }}}
                            if let Some(rest) = text.get(i + 3..) {
                                if let Some(end) = rest.find("}}}") {
                                    i += 3 + end + 3; // Skip past {{{ content }}}
                                    continue;
                                }
                            }
                            i += 3;
                            continue;
                        }

                        // This is a {{ config variable reference
                        // Find closing }}
                        if let Some(rest) = text.get(i + 2..) {
                            if let Some(end) = rest.find("}}") {
                                let var_name = &rest[..end];
                                // Check if the variable exists in the map
                                if !var_name.is_empty()
                                    && !var_name.contains('{')
                                    && !var_name.contains('}')
                                    && !vars.contains_key(var_name)
                                {
                                    errors.push(TemplateValidationError::UnresolvedVariable(
                                        var_name.to_string(),
                                    ));
                                }
                                i += 2 + end + 2; // Skip past {{ content }}
                                continue;
                            }
                        }
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            };

        check_unresolved(&self.partial_tree_entry, &self.variables, &mut errors);
        check_unresolved(&self.partial_file_block, &self.variables, &mut errors);
        check_unresolved(&self.layout, &self.variables, &mut errors);

        errors
    }

    /// Compiles the template into a CompiledTemplate.
    /// This resolves all {{VAR}} references and prepares the template for runtime.
    pub fn compile(&self, name: &str) -> Result<CompiledTemplate, Vec<TemplateValidationError>> {
        // Validate first
        let errors = self.validate();
        if !errors.is_empty() {
            return Err(errors);
        }

        // Resolve all {{VAR}} in partials and layout
        let resolved_tree = replace_vars(&self.partial_tree_entry, &self.variables);
        let resolved_file = replace_vars(&self.partial_file_block, &self.variables);
        let resolved_layout = replace_vars(&self.layout, &self.variables);

        Ok(CompiledTemplate {
            name: name.to_string(),
            variables: self.variables.clone(),
            partial_tree_entry: resolved_tree,
            partial_file_block: resolved_file,
            layout: resolved_layout,
        })
    }
}

impl CompiledTemplate {
    /// Serializes the compiled template to a binary format.
    /// Format:
    /// - Magic bytes (8 bytes): "DCTXTMPL"
    /// - Version (u32 LE)
    /// - Name length (u32 LE) + Name bytes
    /// - Variables count (u32 LE) + [key_len (u16) + key + val_len (u32) + val] ...
    /// - Partial tree length (u32 LE) + bytes
    /// - Partial file length (u32 LE) + bytes
    /// - Layout length (u32 LE) + bytes
    pub fn serialize<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        // Magic bytes
        writer.write_all(COMPILED_TEMPLATE_MAGIC)?;

        // Version
        writer.write_all(&COMPILED_TEMPLATE_VERSION.to_le_bytes())?;

        // Name
        let name_bytes = self.name.as_bytes();
        writer.write_all(&(name_bytes.len() as u32).to_le_bytes())?;
        writer.write_all(name_bytes)?;

        // Variables count
        writer.write_all(&(self.variables.len() as u32).to_le_bytes())?;

        // Each variable
        for (key, val) in &self.variables {
            let key_bytes = key.as_bytes();
            let val_bytes = val.as_bytes();

            writer.write_all(&(key_bytes.len() as u16).to_le_bytes())?;
            writer.write_all(key_bytes)?;
            writer.write_all(&(val_bytes.len() as u32).to_le_bytes())?;
            writer.write_all(val_bytes)?;
        }

        // Partial tree entry
        let tree_bytes = self.partial_tree_entry.as_bytes();
        writer.write_all(&(tree_bytes.len() as u32).to_le_bytes())?;
        writer.write_all(tree_bytes)?;

        // Partial file block
        let file_bytes = self.partial_file_block.as_bytes();
        writer.write_all(&(file_bytes.len() as u32).to_le_bytes())?;
        writer.write_all(file_bytes)?;

        // Layout
        let layout_bytes = self.layout.as_bytes();
        writer.write_all(&(layout_bytes.len() as u32).to_le_bytes())?;
        writer.write_all(layout_bytes)?;

        Ok(())
    }

    /// Deserializes a compiled template from binary format.
    pub fn deserialize<R: Read>(reader: &mut R) -> io::Result<Self> {
        // Magic bytes
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;
        if &magic != COMPILED_TEMPLATE_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Invalid template magic bytes. Expected {:?}, got {:?}",
                    COMPILED_TEMPLATE_MAGIC, magic
                ),
            ));
        }

        // Version
        let mut version_buf = [0u8; 4];
        reader.read_exact(&mut version_buf)?;
        let version = u32::from_le_bytes(version_buf);
        if version != COMPILED_TEMPLATE_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Unsupported template version. Expected {}, got {}",
                    COMPILED_TEMPLATE_VERSION, version
                ),
            ));
        }

        // Name
        let mut len_buf = [0u8; 4];
        reader.read_exact(&mut len_buf)?;
        let name_len = u32::from_le_bytes(len_buf) as usize;
        let mut name_bytes = vec![0u8; name_len];
        reader.read_exact(&mut name_bytes)?;
        let name = String::from_utf8(name_bytes).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid UTF-8 name: {}", e),
            )
        })?;

        // Variables count
        reader.read_exact(&mut len_buf)?;
        let var_count = u32::from_le_bytes(len_buf) as usize;

        let mut variables = HashMap::new();
        for _ in 0..var_count {
            // Key length (u16)
            let mut key_len_buf = [0u8; 2];
            reader.read_exact(&mut key_len_buf)?;
            let key_len = u16::from_le_bytes(key_len_buf) as usize;

            // Key bytes
            let mut key_bytes = vec![0u8; key_len];
            reader.read_exact(&mut key_bytes)?;
            let key = String::from_utf8(key_bytes).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid UTF-8 key: {}", e),
                )
            })?;

            // Value length (u32)
            reader.read_exact(&mut len_buf)?;
            let val_len = u32::from_le_bytes(len_buf) as usize;

            // Value bytes
            let mut val_bytes = vec![0u8; val_len];
            reader.read_exact(&mut val_bytes)?;
            let val = String::from_utf8(val_bytes).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid UTF-8 value: {}", e),
                )
            })?;

            variables.insert(key, val);
        }

        // Partial tree entry
        reader.read_exact(&mut len_buf)?;
        let tree_len = u32::from_le_bytes(len_buf) as usize;
        let mut tree_bytes = vec![0u8; tree_len];
        reader.read_exact(&mut tree_bytes)?;
        let partial_tree_entry = String::from_utf8(tree_bytes).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid UTF-8 tree partial: {}", e),
            )
        })?;

        // Partial file block
        reader.read_exact(&mut len_buf)?;
        let file_len = u32::from_le_bytes(len_buf) as usize;
        let mut file_bytes = vec![0u8; file_len];
        reader.read_exact(&mut file_bytes)?;
        let partial_file_block = String::from_utf8(file_bytes).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid UTF-8 file partial: {}", e),
            )
        })?;

        // Layout
        reader.read_exact(&mut len_buf)?;
        let layout_len = u32::from_le_bytes(len_buf) as usize;
        let mut layout_bytes = vec![0u8; layout_len];
        reader.read_exact(&mut layout_bytes)?;
        let layout = String::from_utf8(layout_bytes).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid UTF-8 layout: {}", e),
            )
        })?;

        Ok(Self {
            name,
            variables,
            partial_tree_entry,
            partial_file_block,
            layout,
        })
    }

    /// Saves the compiled template to a file.
    pub fn save_to_file(&self, path: &Path) -> io::Result<()> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        self.serialize(&mut writer)?;
        writer.flush()?;
        Ok(())
    }

    /// Loads a compiled template from a file.
    pub fn load_from_file(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        Self::deserialize(&mut reader)
    }

    /// Converts back to a Template for use with the rendering engine.
    /// This is useful because the renderer expects Template, not CompiledTemplate.
    pub fn to_template(&self) -> Template {
        Template {
            variables: self.variables.clone(),
            partial_tree_entry: self.partial_tree_entry.clone(),
            partial_file_block: self.partial_file_block.clone(),
            layout: self.layout.clone(),
        }
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
        assert_eq!(
            tmpl.variables.get("multiline"),
            Some(&"line1\nline2".to_string())
        );
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

        assert_eq!(
            tmpl.variables.get("project"),
            Some(&"MyProject".to_string())
        );
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
        assert_eq!(
            tmpl.variables.get("tree_start"),
            Some(&"<TREE>".to_string())
        );
        assert_eq!(tmpl.variables.get("file_end"), Some(&"</FILE>".to_string()));

        // Check partials contain expected placeholders
        assert!(tmpl.partial_tree_entry.contains("{{{node.type_char}}}"));
        assert!(tmpl.partial_file_block.contains("{{{file.content}}}"));

        // Check layout structure
        assert!(tmpl.layout.contains("{{{tree_start}}}"));
        assert!(tmpl.layout.contains("{{{comp.tree}}}"));
        assert!(tmpl.layout.contains("{{{comp.files}}}"));
    }

    // --- VALIDATION TESTS ---

    #[test]
    fn test_validate_valid_template() {
        let raw = r#"
[VARIABLES]
marker = "TEST"
[PARTIAL:TREE_ENTRY]
{{{node.path}}}
[PARTIAL:FILE_BLOCK]
{{{file.content}}}
[LAYOUT]
{{marker}}
{{{comp.tree}}}
"#;
        let tmpl = Template::parse(raw);
        let errors = tmpl.validate();
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_validate_empty_layout() {
        let raw = r#"
[VARIABLES]
key = "value"
[LAYOUT]
"#;
        let tmpl = Template::parse(raw);
        let errors = tmpl.validate();
        assert!(errors
            .iter()
            .any(|e| matches!(e, TemplateValidationError::EmptySection(_))));
    }

    #[test]
    fn test_validate_unresolved_variable() {
        let raw = r#"
[VARIABLES]
defined = "yes"
[LAYOUT]
{{undefined_var}}
"#;
        let tmpl = Template::parse(raw);
        let errors = tmpl.validate();
        assert!(errors
            .iter()
            .any(|e| matches!(e, TemplateValidationError::UnresolvedVariable(_))));
    }

    // --- COMPILATION TESTS ---

    #[test]
    fn test_compile_resolves_variables() {
        let raw = r#"
[VARIABLES]
prefix = "START"
suffix = "END"
[PARTIAL:TREE_ENTRY]
{{prefix}} {{{node.path}}} {{suffix}}
[PARTIAL:FILE_BLOCK]
{{{file.content}}}
[LAYOUT]
{{prefix}}---{{suffix}}
{{{comp.tree}}}
"#;
        let tmpl = Template::parse(raw);
        let compiled = tmpl.compile("test").unwrap();

        // Variables should be resolved in partials
        assert!(compiled.partial_tree_entry.contains("START"));
        assert!(compiled.partial_tree_entry.contains("END"));
        assert!(!compiled.partial_tree_entry.contains("{{prefix}}"));

        // Variables should be resolved in layout
        assert!(compiled.layout.contains("START---END"));
        assert!(!compiled.layout.contains("{{prefix}}"));
    }

    #[test]
    fn test_compile_preserves_data_placeholders() {
        let raw = r#"
[VARIABLES]
marker = "X"
[PARTIAL:TREE_ENTRY]
{{marker}} {{{node.path}}}
[PARTIAL:FILE_BLOCK]
{{{file.content}}}
[LAYOUT]
{{{comp.tree}}}
"#;
        let tmpl = Template::parse(raw);
        let compiled = tmpl.compile("test").unwrap();

        // Data placeholders should remain
        assert!(compiled.partial_tree_entry.contains("{{{node.path}}}"));
        assert!(compiled.layout.contains("{{{comp.tree}}}"));
    }

    #[test]
    fn test_compile_fails_on_invalid_template() {
        let raw = r#"
[VARIABLES]
[LAYOUT]
"#;
        let tmpl = Template::parse(raw);
        let result = tmpl.compile("test");
        assert!(result.is_err());
    }

    // --- SERIALIZATION TESTS ---

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let raw = r#"
[VARIABLES]
key1 = "value1"
key2 = "value2"
[PARTIAL:TREE_ENTRY]
{{{node.type_char}}} {{{node.path}}}
[PARTIAL:FILE_BLOCK]
{{{file.content}}}
[LAYOUT]
HEADER
{{{comp.tree}}}
{{{comp.files}}}
"#;
        let tmpl = Template::parse(raw);
        let original = tmpl.compile("my_template").unwrap();

        // Serialize
        let mut buffer = Vec::new();
        original.serialize(&mut buffer).unwrap();

        // Deserialize
        let mut cursor = std::io::Cursor::new(buffer);
        let loaded = CompiledTemplate::deserialize(&mut cursor).unwrap();

        // Verify
        assert_eq!(loaded.name, "my_template");
        assert_eq!(loaded.variables.get("key1"), Some(&"value1".to_string()));
        assert_eq!(loaded.variables.get("key2"), Some(&"value2".to_string()));
        assert_eq!(loaded.partial_tree_entry, original.partial_tree_entry);
        assert_eq!(loaded.partial_file_block, original.partial_file_block);
        assert_eq!(loaded.layout, original.layout);
    }

    #[test]
    fn test_deserialize_invalid_magic() {
        let invalid_data = b"INVALID_";
        let mut cursor = std::io::Cursor::new(invalid_data);
        let result = CompiledTemplate::deserialize(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn test_to_template_conversion() {
        let raw = r#"
[VARIABLES]
v = "test"
[PARTIAL:TREE_ENTRY]
entry
[PARTIAL:FILE_BLOCK]
block
[LAYOUT]
layout
"#;
        let tmpl = Template::parse(raw);
        let compiled = tmpl.compile("test").unwrap();
        let back_to_template = compiled.to_template();

        assert_eq!(
            back_to_template.partial_tree_entry,
            compiled.partial_tree_entry
        );
        assert_eq!(
            back_to_template.partial_file_block,
            compiled.partial_file_block
        );
        assert_eq!(back_to_template.layout, compiled.layout);
    }
}
