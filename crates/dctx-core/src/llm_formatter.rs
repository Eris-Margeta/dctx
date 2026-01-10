use crate::datatypes::DirContextTreeNode; // Maknut NodeType
use crate::version::Version;
// Maknut DiffReport import
use std::collections::HashMap;
use std::io::{self, Write};

const DEFAULT_TEMPLATE_RAW: &str = include_str!("../templates/default.dctx_template");

struct ParsedTemplate {
    variables: HashMap<String, String>,
    partial_tree_entry: String,
    partial_file_block: String,
    layout: String,
}

pub fn generate_llm_context<W: Write>(
    writer: &mut W,
    root: &DirContextTreeNode,
    version: &Version,
    project_name: &str,
) -> io::Result<()> {
    let tmpl = parse_template(DEFAULT_TEMPLATE_RAW);
    let (tree_content, files_content) = generate_components(root, &tmpl);
    let final_output = render_layout(&tmpl, tree_content, files_content);

    writeln!(
        writer,
        "[{}_SNAPSHOT_{}]",
        project_name.to_uppercase(),
        version
    )?;
    writeln!(writer)?;
    writer.write_all(final_output.as_bytes())?;

    Ok(())
}

// --- PARSING ENGINE ---

fn parse_template(raw: &str) -> ParsedTemplate {
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

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_section = trimmed;
            continue;
        }

        match current_section {
            "[VARIABLES]" => {
                if let Some((key, val)) = trimmed.split_once('=') {
                    let key = key.trim().to_string();
                    let val = val.trim().trim_matches('"').to_string();
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

    for _ in 0..2 {
        let keys: Vec<String> = variables.keys().cloned().collect();
        for key in keys {
            let val = variables[&key].clone();
            let resolved = replace_vars(&val, &variables);
            variables.insert(key, resolved);
        }
    }

    ParsedTemplate {
        variables,
        partial_tree_entry: partial_tree.trim().to_string(),
        partial_file_block: partial_file.trim().to_string(),
        layout: layout.trim().to_string(),
    }
}

// --- RENDERING ENGINE ---

fn generate_components(root: &DirContextTreeNode, tmpl: &ParsedTemplate) -> (String, String) {
    let mut tree_buffer = String::new();
    let mut files_buffer = String::new();

    let mut id_counter = 1;
    render_recursive(
        root,
        tmpl,
        &mut tree_buffer,
        &mut files_buffer,
        &mut id_counter,
    );

    (
        tree_buffer.trim().to_string(),
        files_buffer.trim().to_string(),
    )
}

fn render_recursive(
    node: &DirContextTreeNode,
    tmpl: &ParsedTemplate,
    tree_buf: &mut String,
    files_buf: &mut String,
    id_counter: &mut usize,
) {
    let current_id = id_counter.to_string();
    *id_counter += 1;

    let type_char = if node.is_dir() { "[D]" } else { "[F]" };
    let size_part = if node.is_dir() {
        String::new()
    } else {
        format!(
            ", {}:{}",
            tmpl.variables
                .get("attr_size")
                .unwrap_or(&"SIZE".to_string()),
            node.content_size
        )
    };

    let mut entry = tmpl.partial_tree_entry.clone();

    entry = entry.replace("{{{node.type_char}}}", type_char);
    entry = entry.replace("{{{node.path}}}", &node.relative_path);
    entry = entry.replace("{{{node.id}}}", &current_id);
    entry = entry.replace("{{{node.mod}}}", &node.last_modified_timestamp.to_string());
    entry = entry.replace("{{{node.size_part}}}", &size_part);

    entry = replace_vars(&entry, &tmpl.variables);

    tree_buf.push_str(&entry);
    tree_buf.push('\n');

    if !node.is_dir() {
        let line_count = if let Some(content) = &node.file_content {
            content.iter().filter(|&&b| b == b'\n').count() + 1
        } else {
            0
        };

        let content_str = if let Some(content) = &node.file_content {
            String::from_utf8_lossy(content)
        } else {
            std::borrow::Cow::Borrowed("")
        };

        let mut block = tmpl.partial_file_block.clone();
        block = block.replace("{{{file.id}}}", &current_id);
        block = block.replace("{{{file.path}}}", &node.relative_path);
        block = block.replace("{{{file.size}}}", &node.content_size.to_string());
        block = block.replace("{{{file.lines}}}", &line_count.to_string());
        block = block.replace("{{{file.content}}}", &content_str);

        block = replace_vars(&block, &tmpl.variables);

        files_buf.push_str(&block);
        files_buf.push('\n');
    }

    for child in &node.children {
        render_recursive(child, tmpl, tree_buf, files_buf, id_counter);
    }
}

fn render_layout(tmpl: &ParsedTemplate, tree_content: String, files_content: String) -> String {
    let mut output = tmpl.layout.clone();

    output = output.replace("{{{comp.tree}}}", &tree_content);
    output = output.replace("{{{comp.files}}}", &files_content);

    output = replace_vars(&output, &tmpl.variables);

    output
}

fn replace_vars(text: &str, vars: &HashMap<String, String>) -> String {
    let mut result = text.to_string();
    for (key, val) in vars {
        let pattern = format!("{{{{{}}}}}", key);
        result = result.replace(&pattern, val);
    }
    result
}
