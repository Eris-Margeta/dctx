// crates/dctx-core/src/llm_formatter.rs

use crate::datatypes::{DirContextTreeNode, NodeType};
use crate::diff::{ChangeType, DiffReport};
use crate::version::Version;
use std::collections::HashMap;
use std::io::{self, Write};

// The default template raw string.
const DEFAULT_TEMPLATE_RAW: &str = include_str!("../templates/default.dctx_template");

// Header format matching Legacy C implementation for parity
const VERSION_HEADER_PREFIX: &str = "[DIRCONTXT_LLM_SNAPSHOT_";
const VERSION_HEADER_SUFFIX: &str = "]";

const DIFF_HEADER_PREFIX: &str = "[DIRCONTXT_LLM_DIFF_";

struct ParsedTemplate {
    variables: HashMap<String, String>,
    partial_tree_entry: String,
    partial_file_block: String,
    layout: String,
}

/// Generates the full LLM context file (Snapshot).
/// Corresponds to legacy `generate_llm_context_file`.
pub fn generate_llm_context<W: Write>(
    writer: &mut W,
    root: &DirContextTreeNode,
    version: &Version,
    project_name: &str, // Kept for future template use, though header uses fixed format now
) -> io::Result<()> {
    let tmpl = parse_template(DEFAULT_TEMPLATE_RAW);
    let (tree_content, files_content) = generate_components(root, &tmpl);
    let final_output = render_layout(&tmpl, tree_content, files_content);

    // Write Header: [DIRCONTXT_LLM_SNAPSHOT_V{X}.{Y}]
    writeln!(
        writer,
        "{}{}{}",
        VERSION_HEADER_PREFIX, version, VERSION_HEADER_SUFFIX
    )?;
    writeln!(writer)?;

    // Write Template Content
    writer.write_all(final_output.as_bytes())?;

    Ok(())
}

/// Generates the Diff context file.
/// Corresponds to legacy `generate_diff_file`.
/// Format:
/// 1. Header
/// 2. Changes Summary
/// 3. Updated Directory Tree (Full)
/// 4. Content of ADDED/MODIFIED files only.
pub fn generate_diff_context<W: Write>(
    writer: &mut W,
    new_root: &DirContextTreeNode,
    diff_report: &DiffReport,
    old_version: &Version,
    new_version: &Version,
) -> io::Result<()> {
    let tmpl = parse_template(DEFAULT_TEMPLATE_RAW);

    // 1. Write Diff Header
    // [DIRCONTXT_LLM_DIFF_V1] (Legacy hardcoded version for format, but we use actual versioning logic)
    // Legacy C used hardcoded [DIRCONTXT_LLM_DIFF_V1], but put the actual version change below.
    // We will stick to the aesthetic:
    writeln!(writer, "{}]", DIFF_HEADER_PREFIX)?; // e.g., [DIRCONTXT_LLM_DIFF_] - wait, legacy was V1 hardcoded format version.
                                                  // Let's use a clean header:
                                                  // [DIRCONTXT_LLM_DIFF]
                                                  // Version Change: V1.1 -> V1.2
    writeln!(writer, "Version Change: {} -> {}", old_version, new_version)?;
    writeln!(writer)?;

    // 2. Changes Summary
    writeln!(writer, "<CHANGES_SUMMARY>")?;
    for entry in &diff_report.entries {
        let type_str = match entry.change_type {
            ChangeType::Added => "ADDED",
            ChangeType::Removed => "REMOVED",
            ChangeType::Modified => "MODIFIED",
        };
        let suffix = if entry.node_type == NodeType::Directory {
            "/"
        } else {
            ""
        };
        writeln!(writer, "[{}] {}{}", type_str, entry.relative_path, suffix)?;
    }
    writeln!(writer, "</CHANGES_SUMMARY>")?;
    writeln!(writer)?;

    // 3. Updated Directory Tree (Full New State)
    // We reuse the Tree Partial from the template
    let (tree_content, _) = generate_components(new_root, &tmpl);
    writeln!(writer, "<UPDATED_DIRECTORY_TREE>")?;
    writer.write_all(tree_content.as_bytes())?;
    writeln!(writer, "</UPDATED_DIRECTORY_TREE>")?;
    writeln!(writer)?;

    // 4. Content of Changed Files
    // We filter the files based on the DiffReport
    let mut files_buffer = String::new();
    let mut id_counter = 1; // Must match the counter used in tree generation to ensure IDs align?
                            // CAUTION: generate_components resets counter to 1.
                            // To match IDs between Tree and Content, we must traverse exactly the same way.
                            // The `generate_components` function generates both at once.
                            // However, here we only want *specific* files.
                            //
                            // Strategy: We run the full recursive render to get all blocks,
                            // but we filter which ones we append to the output based on the diff report.
                            // This is slightly inefficient (generating strings we discard) but guarantees ID consistency.

    render_recursive_diff_filter(
        new_root,
        &tmpl,
        &mut files_buffer,
        &mut id_counter,
        diff_report,
        0,
    );

    writer.write_all(files_buffer.as_bytes())?;

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

    // Resolve variables (simple 2-pass for nested vars)
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
        0,
    );

    (
        tree_buffer.trim().to_string(),
        files_buffer.trim().to_string(),
    )
}

/// Recursive renderer for standard full snapshot
fn render_recursive(
    node: &DirContextTreeNode,
    tmpl: &ParsedTemplate,
    tree_buf: &mut String,
    files_buf: &mut String,
    id_counter: &mut usize,
    depth: usize,
) {
    // 1. Generate Semantic ID
    // If depth is 0 and it's a directory, it's ROOT.
    // Otherwise: D{03d} or F{03d}
    let current_id = if depth == 0 && node.is_dir() {
        "ROOT".to_string()
    } else if node.is_dir() {
        format!("D{:03}", id_counter)
    } else {
        format!("F{:03}", id_counter)
    };

    *id_counter += 1;

    // 2. Prepare Tree Entry Variables
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

    // 3. Indentation logic
    let indentation = "  ".repeat(depth);

    // 4. Render Tree Entry
    let mut entry = tmpl.partial_tree_entry.clone();
    // We prepend indentation to the entry. Note: This assumes the partial starts with the content.
    // A better way would be {{indent}} variable, but for parity we prepend.
    entry = format!("{}{}", indentation, entry);

    entry = entry.replace("{{{node.type_char}}}", type_char);
    entry = entry.replace("{{{node.path}}}", &node.relative_path);
    entry = entry.replace("{{{node.id}}}", &current_id);
    entry = entry.replace("{{{node.mod}}}", &node.last_modified_timestamp.to_string());
    entry = entry.replace("{{{node.size_part}}}", &size_part);

    entry = replace_vars(&entry, &tmpl.variables);

    tree_buf.push_str(&entry);
    tree_buf.push('\n');

    // 5. Render File Content Block (if file)
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

    // 6. Recurse
    for child in &node.children {
        render_recursive(child, tmpl, tree_buf, files_buf, id_counter, depth + 1);
    }
}

/// Recursive renderer that ONLY generates content blocks for changed files.
/// Used for Diff generation. It still generates IDs sequentially to ensure match.
fn render_recursive_diff_filter(
    node: &DirContextTreeNode,
    tmpl: &ParsedTemplate,
    files_buf: &mut String,
    id_counter: &mut usize,
    diff_report: &DiffReport,
    depth: usize,
) {
    // 1. Generate ID (Must happen to increment counter correctly)
    let current_id = if depth == 0 && node.is_dir() {
        "ROOT".to_string()
    } else if node.is_dir() {
        format!("D{:03}", id_counter)
    } else {
        format!("F{:03}", id_counter)
    };
    *id_counter += 1;

    // 2. Check if this node is in the Diff Report as Added or Modified
    // We search the diff entries for a match on relative_path
    // This is O(N*M), but for typical project sizes it's acceptable vs building a hashmap
    // (Optimization: Pass a HashSet of changed paths)
    let is_changed = diff_report.entries.iter().any(|e| {
        e.relative_path == node.relative_path
            && (e.change_type == ChangeType::Added || e.change_type == ChangeType::Modified)
    });

    if !node.is_dir() && is_changed {
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

    // Recurse
    for child in &node.children {
        render_recursive_diff_filter(child, tmpl, files_buf, id_counter, diff_report, depth + 1);
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
