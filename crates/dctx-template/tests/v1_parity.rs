// SPDX-License-Identifier: Apache-2.0
//! Port of the dctx-core template unit tests. Assertions stay byte-level.

use std::collections::HashMap;

use dctx_template::{CompiledTemplate, Template, TemplateValidationError, replace_vars};

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
    assert_eq!(result, "{{missing}} variable");
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
    let raw = r"
[VARIABLES]
[PARTIAL:TREE_ENTRY]
{{{node.type_char}}} {{{node.path}}}
[LAYOUT]
";
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
    let raw = r"
[VARIABLES]
[LAYOUT]
=== TREE ===
{{{comp.tree}}}
=== FILES ===
{{{comp.files}}}
";
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
    assert_eq!(
        tmpl.variables.get("tree_start"),
        Some(&"<TREE>".to_string())
    );
    assert_eq!(tmpl.variables.get("file_end"), Some(&"</FILE>".to_string()));
    assert!(tmpl.partial_tree_entry.contains("{{{node.type_char}}}"));
    assert!(tmpl.partial_file_block.contains("{{{file.content}}}"));
    assert!(tmpl.layout.contains("{{{tree_start}}}"));
    assert!(tmpl.layout.contains("{{{comp.tree}}}"));
    assert!(tmpl.layout.contains("{{{comp.files}}}"));
}

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
    assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
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
    assert!(
        errors
            .iter()
            .any(|err| matches!(err, TemplateValidationError::EmptySection(_)))
    );
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
    assert!(
        errors
            .iter()
            .any(|err| matches!(err, TemplateValidationError::UnresolvedVariable(_)))
    );
}

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
    let compiled = tmpl.compile("test").expect("compile");
    assert!(compiled.partial_tree_entry.contains("START"));
    assert!(compiled.partial_tree_entry.contains("END"));
    assert!(!compiled.partial_tree_entry.contains("{{prefix}}"));
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
    let compiled = tmpl.compile("test").expect("compile");
    assert!(compiled.partial_tree_entry.contains("{{{node.path}}}"));
    assert!(compiled.layout.contains("{{{comp.tree}}}"));
}

#[test]
fn test_compile_fails_on_invalid_template() {
    let raw = r"
[VARIABLES]
[LAYOUT]
";
    let tmpl = Template::parse(raw);
    assert!(tmpl.compile("test").is_err());
}

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
    let original = tmpl.compile("my_template").expect("compile");
    let mut buffer = Vec::new();
    original.serialize(&mut buffer).expect("serialize");
    let mut cursor = std::io::Cursor::new(buffer);
    let loaded = CompiledTemplate::deserialize(&mut cursor).expect("deserialize");
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
    assert!(CompiledTemplate::deserialize(&mut cursor).is_err());
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
    let compiled = tmpl.compile("test").expect("compile");
    let back = compiled.to_template();
    assert_eq!(back.partial_tree_entry, compiled.partial_tree_entry);
    assert_eq!(back.partial_file_block, compiled.partial_file_block);
    assert_eq!(back.layout, compiled.layout);
}
