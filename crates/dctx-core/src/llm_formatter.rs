// crates/dctx-core/src/llm_formatter.rs

use crate::datatypes::{DirContextTreeNode, NodeType};
use crate::diff::{ChangeType, DiffReport};
use crate::template::{self, Template};
use crate::version::Version;
use std::io::{self, Write};

// The default template raw string.
const DEFAULT_TEMPLATE_RAW: &str = include_str!("../templates/default.dctx_template");

// Branding: DCTX
const VERSION_HEADER_PREFIX: &str = "[DCTX_LLM_CONTEXT_SNAPSHOT_";
const VERSION_HEADER_SUFFIX: &str = "]";

const DIFF_HEADER_PREFIX: &str = "[DCTX_LLM_CONTEXT_DIFF_";

/// Generates the full LLM context file (Snapshot).
pub fn generate_llm_context<W: Write>(
    writer: &mut W,
    root: &DirContextTreeNode,
    version: &Version,
    project_name: &str,
) -> io::Result<()> {
    // 1. Parse Template
    let mut tmpl = Template::parse(DEFAULT_TEMPLATE_RAW);

    // 2. Inject Runtime Variables
    // Make project name available to the template engine as "project"
    tmpl.insert_variable("project", project_name);

    // 3. Generate Components
    let (tree_content, files_content) = generate_components(root, &tmpl);
    let final_output = render_layout(&tmpl, tree_content, files_content);

    // 4. Write Header Line 1: [DCTX_LLM_CONTEXT_SNAPSHOT_V{Version}]
    writeln!(
        writer,
        "{}{}{}",
        VERSION_HEADER_PREFIX, version, VERSION_HEADER_SUFFIX
    )?;

    // 5. Write Header Line 2: [DIRECTORY:{DirectoryName}]
    writeln!(writer, "[DIRECTORY:{}]", project_name)?;
    writeln!(writer)?;

    // 6. Write Template Content
    writer.write_all(final_output.as_bytes())?;

    Ok(())
}

/// Generates the Diff context file.
pub fn generate_diff_context<W: Write>(
    writer: &mut W,
    new_root: &DirContextTreeNode,
    diff_report: &DiffReport,
    old_version: &Version,
    new_version: &Version,
) -> io::Result<()> {
    let tmpl = Template::parse(DEFAULT_TEMPLATE_RAW);

    // 1. Write Diff Header
    // [DCTX_LLM_CONTEXT_DIFF_V{Version}]
    writeln!(writer, "{}{}]", DIFF_HEADER_PREFIX, new_version)?;
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
    let (tree_content, _) = generate_components(new_root, &tmpl);
    writeln!(writer, "<UPDATED_DIRECTORY_TREE>")?;
    writer.write_all(tree_content.as_bytes())?;
    writeln!(writer, "</UPDATED_DIRECTORY_TREE>")?;
    writeln!(writer)?;

    // 4. Content of Changed Files
    let mut files_buffer = String::new();
    let mut id_counter = 1;

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

// --- RENDERING ENGINE ---

fn generate_components(root: &DirContextTreeNode, tmpl: &Template) -> (String, String) {
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
    tmpl: &Template,
    tree_buf: &mut String,
    files_buf: &mut String,
    id_counter: &mut usize,
    depth: usize,
) {
    // 1. Generate Semantic ID
    // If depth is 0 and it's a directory, it's ROOT.
    // Otherwise: D{03d} or F{03d}
    // Note: We check if path is empty for root, but depth 0 is safer for recursion base.
    let current_id = if depth == 0 && node.is_dir() {
        "ROOT".to_string()
    } else if node.is_dir() {
        format!("D{:03}", id_counter)
    } else {
        format!("F{:03}", id_counter)
    };

    if !(depth == 0 && node.is_dir()) {
        *id_counter += 1;
    }

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

    // 3. Indentation logic (2 spaces per level)
    let indentation = "  ".repeat(depth);

    // 4. Render Tree Entry
    let mut entry = tmpl.partial_tree_entry.clone();

    // Inject indentation at the start
    entry = format!("{}{}", indentation, entry);

    entry = entry.replace("{{{node.type_char}}}", type_char);
    entry = entry.replace("{{{node.path}}}", &node.relative_path);
    entry = entry.replace("{{{node.id}}}", &current_id);
    entry = entry.replace("{{{node.mod}}}", &node.last_modified_timestamp.to_string());
    entry = entry.replace("{{{node.size_part}}}", &size_part);

    entry = template::replace_vars(&entry, &tmpl.variables);

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

        block = template::replace_vars(&block, &tmpl.variables);

        files_buf.push_str(&block);
        files_buf.push('\n');
    }

    // 6. Recurse
    for child in &node.children {
        render_recursive(child, tmpl, tree_buf, files_buf, id_counter, depth + 1);
    }
}

/// Recursive renderer that ONLY generates content blocks for changed files.
fn render_recursive_diff_filter(
    node: &DirContextTreeNode,
    tmpl: &Template,
    files_buf: &mut String,
    id_counter: &mut usize,
    diff_report: &DiffReport,
    depth: usize,
) {
    // 1. Generate ID (Must happen to increment counter correctly to match full tree)
    let current_id = if depth == 0 && node.is_dir() {
        "ROOT".to_string()
    } else if node.is_dir() {
        format!("D{:03}", id_counter)
    } else {
        format!("F{:03}", id_counter)
    };

    if !(depth == 0 && node.is_dir()) {
        *id_counter += 1;
    }

    // 2. Check if this node is in the Diff Report as Added or Modified
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

        block = template::replace_vars(&block, &tmpl.variables);

        files_buf.push_str(&block);
        files_buf.push('\n');
    }

    // Recurse
    for child in &node.children {
        render_recursive_diff_filter(child, tmpl, files_buf, id_counter, diff_report, depth + 1);
    }
}

fn render_layout(tmpl: &Template, tree_content: String, files_content: String) -> String {
    let mut output = tmpl.layout.clone();
    output = output.replace("{{{comp.tree}}}", &tree_content);
    output = output.replace("{{{comp.files}}}", &files_content);
    output = template::replace_vars(&output, &tmpl.variables);
    output
}
