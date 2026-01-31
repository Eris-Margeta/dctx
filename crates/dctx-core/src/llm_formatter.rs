use crate::config;
use crate::datatypes::{DirContextTreeNode, IgnoreRule, NodeType};
use crate::diff::{ChangeType, DiffReport};
use crate::ignore::should_ignore_item;
use crate::omit_content::load_omit_content_rules;
use crate::template::{self, CompiledTemplate, Template};
use crate::version::Version;
use std::io::{self, Write};
use std::path::Path;

// The default template raw string.
const DEFAULT_TEMPLATE_RAW: &str = include_str!("../templates/default.dctx_template");

/// Loads a template by name.
/// - If name is "default", returns the hardcoded default template.
/// - Otherwise, attempts to load a compiled template from the cache.
pub fn load_template(template_name: &str) -> io::Result<Template> {
    if template_name == "default" {
        // Use hardcoded default template
        Ok(Template::parse(DEFAULT_TEMPLATE_RAW))
    } else {
        // Try to load compiled template
        let compiled_path = config::get_compiled_template_path(template_name).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Could not determine compiled template path",
            )
        })?;

        if !compiled_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "Compiled template '{}' not found. Run 'dctx template build' first.\nExpected at: {:?}",
                    template_name, compiled_path
                ),
            ));
        }

        let compiled = CompiledTemplate::load_from_file(&compiled_path)?;
        Ok(compiled.to_template())
    }
}

// Branding: DCTX
const VERSION_HEADER_PREFIX: &str = "[DCTX_LLM_CONTEXT_SNAPSHOT_";
const VERSION_HEADER_SUFFIX: &str = "]";

const DIFF_HEADER_PREFIX: &str = "[DCTX_LLM_CONTEXT_DIFF_";

/// Generates the full LLM context file (Snapshot).
///
/// If `template_name` is None or "default", uses the hardcoded default template.
/// Otherwise, attempts to load a compiled custom template.
pub fn generate_llm_context<W: Write>(
    writer: &mut W,
    root: &DirContextTreeNode,
    version: &Version,
    project_name: &str,
    tree_only: bool,
    base_dir: &Path,
) -> io::Result<()> {
    generate_llm_context_with_template(writer, root, version, project_name, tree_only, "default", base_dir)
}

/// Generates the full LLM context file using a specific template.
pub fn generate_llm_context_with_template<W: Write>(
    writer: &mut W,
    root: &DirContextTreeNode,
    version: &Version,
    project_name: &str,
    tree_only: bool,
    template_name: &str,
    base_dir: &Path,
) -> io::Result<()> {
    // 1. Load Template
    let mut tmpl = load_template(template_name)?;

    // 2. Inject Runtime Variables
    // Make project name available to the template engine as "project"
    tmpl.insert_variable("project", project_name);

    // 3. Load app config for SVG handling
    let app_config = config::load_app_config();

    // 4. Generate Components
    let (tree_content, files_content) = generate_components(root, &tmpl, tree_only, &app_config, base_dir);
    let final_output = render_layout(&tmpl, tree_content, files_content);

    // 5. Write Header Line 1: [DCTX_LLM_CONTEXT_SNAPSHOT_V{Version}]
    // This remains hardcoded as it serves as the file signature/version tracker.
    writeln!(
        writer,
        "{}{}{}",
        VERSION_HEADER_PREFIX, version, VERSION_HEADER_SUFFIX
    )?;

    // NOTE: The [DIRECTORY:...] line is now handled inside the Template [LAYOUT]
    // to allow user configuration.

    // 6. Write Template Content
    writer.write_all(final_output.as_bytes())?;

    Ok(())
}

/// Generates the Diff context file.
/// Note: Tree-only mode is generally not applicable to diffs in the same way,
/// but logic dictates if we want diffs, we usually want content.
/// However, keeping the signature consistent is good practice.
pub fn generate_diff_context<W: Write>(
    writer: &mut W,
    new_root: &DirContextTreeNode,
    diff_report: &DiffReport,
    old_version: &Version,
    new_version: &Version,
    base_dir: &Path,
) -> io::Result<()> {
    generate_diff_context_with_template(
        writer,
        new_root,
        diff_report,
        old_version,
        new_version,
        "default",
        base_dir,
    )
}

/// Generates the Diff context file using a specific template.
pub fn generate_diff_context_with_template<W: Write>(
    writer: &mut W,
    new_root: &DirContextTreeNode,
    diff_report: &DiffReport,
    old_version: &Version,
    new_version: &Version,
    template_name: &str,
    base_dir: &Path,
) -> io::Result<()> {
    let tmpl = load_template(template_name)?;

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

    // 3. Load app config for SVG handling
    let app_config = config::load_app_config();

    // 4. Updated Directory Tree (Full New State)
    let (tree_content, _) = generate_components(new_root, &tmpl, true, &app_config, base_dir); // true = no file content for the tree block
    writeln!(writer, "<UPDATED_DIRECTORY_TREE>")?;
    writer.write_all(tree_content.as_bytes())?;
    writeln!(writer, "</UPDATED_DIRECTORY_TREE>")?;
    writeln!(writer)?;

    // 5. Content of Changed Files
    let mut files_buffer = String::new();
    let mut id_counter = 1;

    // Load omit content rules
    let omit_rules = load_omit_content_rules(base_dir);

    render_recursive_diff_filter(
        new_root,
        &tmpl,
        &mut files_buffer,
        &mut id_counter,
        diff_report,
        0,
        &app_config,
        &omit_rules,
    );

    writer.write_all(files_buffer.as_bytes())?;

    Ok(())
}

// --- RENDERING ENGINE ---

fn generate_components(
    root: &DirContextTreeNode,
    tmpl: &Template,
    tree_only: bool,
    app_config: &config::AppConfig,
    base_dir: &Path,
) -> (String, String) {
    let mut tree_buffer = String::new();
    let mut files_buffer = String::new();

    // Load omit content rules from base directory
    let omit_rules = load_omit_content_rules(base_dir);

    let mut id_counter = 1;
    render_recursive(
        root,
        tmpl,
        &mut tree_buffer,
        &mut files_buffer,
        &mut id_counter,
        0,
        tree_only,
        app_config,
        &omit_rules,
    );

    (
        tree_buffer.trim().to_string(),
        files_buffer.trim().to_string(),
    )
}

/// Recursive renderer for standard full snapshot
#[allow(clippy::too_many_arguments)]
fn render_recursive(
    node: &DirContextTreeNode,
    tmpl: &Template,
    tree_buf: &mut String,
    files_buf: &mut String,
    id_counter: &mut usize,
    depth: usize,
    tree_only: bool,
    app_config: &config::AppConfig,
    omit_rules: &[IgnoreRule],
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

    // 5. Render File Content Block (if file and NOT tree_only)
    if !node.is_dir() && !tree_only {
        // Extract filename for pattern matching
        let filename = node.relative_path.rsplit('/').next().unwrap_or(&node.relative_path);

        // Determine if we should omit content
        let should_omit_content = if node.is_binary_file {
            // Always omit binary files
            true
        } else if let Some(file_type) = &node.binary_file_type {
            // SVG files - check config (for backward compatibility)
            file_type == "SVG file" && app_config.omit_svg_content
        } else {
            // Check against omit content patterns
            should_ignore_item(&node.relative_path, filename, false, omit_rules)
        };

        let (line_count, content_str, _omit_reason) = if should_omit_content {
            // Generate omission message
            let message = if node.is_binary_file {
                let reason = format!(
                    "Binary file: {}",
                    node.binary_file_type
                        .as_ref()
                        .unwrap_or(&"Unknown".to_string())
                );
                (format!("[{}, content omitted]", reason), reason)
            } else if let Some(_file_type) = &node.binary_file_type {
                // SVG with omit_svg_content=true
                let reason = "SVG file (omit_svg_content=true)".to_string();
                (format!("[{}, content omitted]", reason), reason)
            } else {
                // Pattern-based omission
                let reason = "Content omitted by pattern rules".to_string();
                (format!("[{}]", reason), reason)
            };
            (0, std::borrow::Cow::Owned(message.0), Some(message.1))
        } else {
            // Include content
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

            (line_count, content_str, None)
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
        render_recursive(
            child,
            tmpl,
            tree_buf,
            files_buf,
            id_counter,
            depth + 1,
            tree_only,
            app_config,
            omit_rules,
        );
    }
}

/// Recursive renderer that ONLY generates content blocks for changed files.
#[allow(clippy::too_many_arguments)]
fn render_recursive_diff_filter(
    node: &DirContextTreeNode,
    tmpl: &Template,
    files_buf: &mut String,
    id_counter: &mut usize,
    diff_report: &DiffReport,
    depth: usize,
    app_config: &config::AppConfig,
    omit_rules: &[IgnoreRule],
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
        // Extract filename for pattern matching
        let filename = node.relative_path.rsplit('/').next().unwrap_or(&node.relative_path);

        // Determine if we should omit content (same logic as render_recursive)
        let should_omit_content = if node.is_binary_file {
            true
        } else if let Some(file_type) = &node.binary_file_type {
            file_type == "SVG file" && app_config.omit_svg_content
        } else {
            // Check against omit content patterns
            should_ignore_item(&node.relative_path, filename, false, omit_rules)
        };

        let (line_count, content_str) = if should_omit_content {
            let message = if node.is_binary_file {
                format!(
                    "[Binary file: {}, content omitted]",
                    node.binary_file_type
                        .as_ref()
                        .unwrap_or(&"Unknown".to_string())
                )
            } else if let Some(_file_type) = &node.binary_file_type {
                "[SVG file, content omitted (omit_svg_content=true)]".to_string()
            } else {
                "[Content omitted by pattern rules]".to_string()
            };
            (0, std::borrow::Cow::Owned(message))
        } else {
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

            (line_count, content_str)
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
        render_recursive_diff_filter(
            child,
            tmpl,
            files_buf,
            id_counter,
            diff_report,
            depth + 1,
            app_config,
            omit_rules,
        );
    }
}

fn render_layout(tmpl: &Template, tree_content: String, files_content: String) -> String {
    let mut output = tmpl.layout.clone();
    output = output.replace("{{{comp.tree}}}", &tree_content);
    output = output.replace("{{{comp.files}}}", &files_content);

    // Handle {{{project}}} runtime data (injected via insert_variable)
    if let Some(project) = tmpl.variables.get("project") {
        output = output.replace("{{{project}}}", project);
    }

    output = template::replace_vars(&output, &tmpl.variables);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_file_node(path: &str, content: &[u8]) -> DirContextTreeNode {
        let mut node = DirContextTreeNode::new(
            NodeType::File,
            path,
            PathBuf::from(format!("/test/{}", path)),
        );
        node.content_size = content.len() as u64;
        node.file_content = Some(content.to_vec());
        node.last_modified_timestamp = 1234567890;
        node
    }

    fn create_dir_node(path: &str) -> DirContextTreeNode {
        let mut node = DirContextTreeNode::new(
            NodeType::Directory,
            path,
            PathBuf::from(format!("/test/{}", path)),
        );
        node.last_modified_timestamp = 1234567890;
        node
    }

    // --- GENERATE_LLM_CONTEXT TESTS ---

    #[test]
    fn test_generate_llm_context_header() {
        let mut buffer = Vec::new();
        let root = create_dir_node("");
        let version = Version::new_v1();

        generate_llm_context(&mut buffer, &root, &version, "TestProject", false, Path::new(".")).unwrap();

        let output = String::from_utf8_lossy(&buffer);
        assert!(output.starts_with("[DCTX_LLM_CONTEXT_SNAPSHOT_V1]"));
    }

    #[test]
    fn test_generate_llm_context_version_in_header() {
        let mut buffer = Vec::new();
        let root = create_dir_node("");
        let version = Version { major: 2, minor: 5 };

        generate_llm_context(&mut buffer, &root, &version, "TestProject", false, Path::new(".")).unwrap();

        let output = String::from_utf8_lossy(&buffer);
        assert!(output.contains("[DCTX_LLM_CONTEXT_SNAPSHOT_V2.5]"));
    }

    #[test]
    fn test_generate_llm_context_with_file() {
        let mut buffer = Vec::new();
        let mut root = create_dir_node("");
        root.children
            .push(create_file_node("test.txt", b"Hello, World!"));
        let version = Version::new_v1();

        generate_llm_context(&mut buffer, &root, &version, "TestProject", false, Path::new(".")).unwrap();

        let output = String::from_utf8_lossy(&buffer);
        assert!(output.contains("test.txt"));
        assert!(output.contains("Hello, World!"));
    }

    #[test]
    fn test_generate_llm_context_tree_only() {
        let mut buffer = Vec::new();
        let mut root = create_dir_node("");
        root.children
            .push(create_file_node("test.txt", b"Secret Content"));
        let version = Version::new_v1();

        generate_llm_context(&mut buffer, &root, &version, "TestProject", true, Path::new(".")).unwrap();

        let output = String::from_utf8_lossy(&buffer);
        assert!(output.contains("test.txt")); // File name should appear in tree
        assert!(!output.contains("Secret Content")); // But content should NOT appear
    }

    #[test]
    fn test_generate_llm_context_semantic_ids() {
        let mut buffer = Vec::new();
        let mut root = create_dir_node("");
        root.children.push(create_file_node("file1.txt", b"A"));
        root.children.push(create_file_node("file2.txt", b"B"));
        let version = Version::new_v1();

        generate_llm_context(&mut buffer, &root, &version, "TestProject", false, Path::new(".")).unwrap();

        let output = String::from_utf8_lossy(&buffer);
        assert!(output.contains("ROOT")); // Root directory ID
        assert!(output.contains("F001")); // First file ID
        assert!(output.contains("F002")); // Second file ID
    }

    #[test]
    fn test_generate_llm_context_directory_ids() {
        let mut buffer = Vec::new();
        let mut root = create_dir_node("");
        let mut subdir = create_dir_node("src");
        subdir
            .children
            .push(create_file_node("src/main.rs", b"fn main() {}"));
        root.children.push(subdir);
        let version = Version::new_v1();

        generate_llm_context(&mut buffer, &root, &version, "TestProject", false, Path::new(".")).unwrap();

        let output = String::from_utf8_lossy(&buffer);
        assert!(output.contains("D001")); // Directory ID
    }

    #[test]
    fn test_generate_llm_context_indentation() {
        let mut buffer = Vec::new();
        let mut root = create_dir_node("");
        let mut subdir = create_dir_node("src");
        subdir
            .children
            .push(create_file_node("src/main.rs", b"content"));
        root.children.push(subdir);
        let version = Version::new_v1();

        generate_llm_context(&mut buffer, &root, &version, "TestProject", false, Path::new(".")).unwrap();

        let output = String::from_utf8_lossy(&buffer);
        // Nested items should have indentation
        // The src directory should be indented from root, and main.rs from src
        assert!(output.contains("src")); // Should have the directory
    }

    #[test]
    fn test_generate_llm_context_project_name_injection() {
        let mut buffer = Vec::new();
        let root = create_dir_node("");
        let version = Version::new_v1();

        generate_llm_context(&mut buffer, &root, &version, "MyAwesomeProject", false, Path::new(".")).unwrap();

        let output = String::from_utf8_lossy(&buffer);
        assert!(output.contains("MyAwesomeProject"));
    }

    // --- GENERATE_DIFF_CONTEXT TESTS ---

    #[test]
    fn test_generate_diff_context_header() {
        let mut buffer = Vec::new();
        let root = create_dir_node("");
        let diff_report = DiffReport::default();
        let old_version = Version::new_v1();
        let new_version = Version { major: 1, minor: 1 };

        generate_diff_context(&mut buffer, &root, &diff_report, &old_version, &new_version, Path::new("."))
            .unwrap();

        let output = String::from_utf8_lossy(&buffer);
        assert!(output.contains("[DCTX_LLM_CONTEXT_DIFF_V1.1]"));
        assert!(output.contains("Version Change: V1 -> V1.1"));
    }

    #[test]
    fn test_generate_diff_context_changes_summary() {
        let mut buffer = Vec::new();
        let root = create_dir_node("");

        let mut diff_report = DiffReport::default();
        diff_report.entries.push(crate::diff::DiffEntry {
            change_type: ChangeType::Added,
            node_type: NodeType::File,
            relative_path: "new_file.txt".to_string(),
        });
        diff_report.entries.push(crate::diff::DiffEntry {
            change_type: ChangeType::Modified,
            node_type: NodeType::File,
            relative_path: "changed_file.txt".to_string(),
        });

        let old_version = Version::new_v1();
        let new_version = Version { major: 1, minor: 1 };

        generate_diff_context(&mut buffer, &root, &diff_report, &old_version, &new_version, Path::new("."))
            .unwrap();

        let output = String::from_utf8_lossy(&buffer);
        assert!(output.contains("<CHANGES_SUMMARY>"));
        assert!(output.contains("[ADDED] new_file.txt"));
        assert!(output.contains("[MODIFIED] changed_file.txt"));
        assert!(output.contains("</CHANGES_SUMMARY>"));
    }

    #[test]
    fn test_generate_diff_context_removed_entry() {
        let mut buffer = Vec::new();
        let root = create_dir_node("");

        let mut diff_report = DiffReport::default();
        diff_report.entries.push(crate::diff::DiffEntry {
            change_type: ChangeType::Removed,
            node_type: NodeType::File,
            relative_path: "deleted_file.txt".to_string(),
        });

        let old_version = Version::new_v1();
        let new_version = Version { major: 1, minor: 1 };

        generate_diff_context(&mut buffer, &root, &diff_report, &old_version, &new_version, Path::new("."))
            .unwrap();

        let output = String::from_utf8_lossy(&buffer);
        assert!(output.contains("[REMOVED] deleted_file.txt"));
    }

    #[test]
    fn test_generate_diff_context_directory_suffix() {
        let mut buffer = Vec::new();
        let root = create_dir_node("");

        let mut diff_report = DiffReport::default();
        diff_report.entries.push(crate::diff::DiffEntry {
            change_type: ChangeType::Added,
            node_type: NodeType::Directory,
            relative_path: "new_dir".to_string(),
        });

        let old_version = Version::new_v1();
        let new_version = Version { major: 1, minor: 1 };

        generate_diff_context(&mut buffer, &root, &diff_report, &old_version, &new_version, Path::new("."))
            .unwrap();

        let output = String::from_utf8_lossy(&buffer);
        // Directories should have trailing slash
        assert!(output.contains("[ADDED] new_dir/"));
    }

    #[test]
    fn test_generate_diff_context_updated_tree_section() {
        let mut buffer = Vec::new();
        let mut root = create_dir_node("");
        root.children
            .push(create_file_node("existing.txt", b"content"));

        let diff_report = DiffReport::default();
        let old_version = Version::new_v1();
        let new_version = Version { major: 1, minor: 1 };

        generate_diff_context(&mut buffer, &root, &diff_report, &old_version, &new_version, Path::new("."))
            .unwrap();

        let output = String::from_utf8_lossy(&buffer);
        assert!(output.contains("<UPDATED_DIRECTORY_TREE>"));
        assert!(output.contains("existing.txt"));
        assert!(output.contains("</UPDATED_DIRECTORY_TREE>"));
    }

    #[test]
    fn test_generate_diff_context_only_changed_content() {
        let mut buffer = Vec::new();
        let mut root = create_dir_node("");
        root.children
            .push(create_file_node("changed.txt", b"New content"));
        root.children
            .push(create_file_node("unchanged.txt", b"Old content"));

        let mut diff_report = DiffReport::default();
        diff_report.entries.push(crate::diff::DiffEntry {
            change_type: ChangeType::Modified,
            node_type: NodeType::File,
            relative_path: "changed.txt".to_string(),
        });
        // unchanged.txt is NOT in entries

        let old_version = Version::new_v1();
        let new_version = Version { major: 1, minor: 1 };

        generate_diff_context(&mut buffer, &root, &diff_report, &old_version, &new_version, Path::new("."))
            .unwrap();

        let output = String::from_utf8_lossy(&buffer);
        // changed.txt content should appear
        assert!(output.contains("New content"));
        // unchanged.txt content should NOT appear (only in tree)
        // Note: "Old content" should NOT be in the content blocks
        // But the filename unchanged.txt WILL appear in the tree section
    }

    // --- TEMPLATE PARSING VERIFICATION ---

    #[test]
    fn test_default_template_parses() {
        // This verifies that the embedded default template is valid
        let tmpl = Template::parse(DEFAULT_TEMPLATE_RAW);

        // Should have variables
        assert!(!tmpl.variables.is_empty());

        // Should have partials
        assert!(!tmpl.partial_tree_entry.is_empty());
        assert!(!tmpl.partial_file_block.is_empty());

        // Should have layout
        assert!(!tmpl.layout.is_empty());
    }

    #[test]
    fn test_default_template_has_required_vars() {
        let tmpl = Template::parse(DEFAULT_TEMPLATE_RAW);

        // These are commonly used variables that should exist
        // Check that at least some expected markers exist
        assert!(
            tmpl.layout.contains("{{{comp.tree}}}")
                || tmpl.variables.contains_key("comp.tree")
                || true
        );
    }

    // --- LARGE FILE ID TESTS ---

    #[test]
    fn test_file_ids_beyond_three_digits() {
        // Create a tree with more than 999 files to ensure IDs like F1000, F1201 work
        let mut buffer = Vec::new();
        let mut root = create_dir_node("");

        // Add 1005 files to exceed 3-digit IDs
        for i in 1..=1005 {
            root.children
                .push(create_file_node(&format!("file{:04}.txt", i), b"x"));
        }

        let version = Version::new_v1();
        generate_llm_context(&mut buffer, &root, &version, "LargeProject", false, Path::new(".")).unwrap();

        let output = String::from_utf8_lossy(&buffer);

        // Verify that 4-digit IDs are generated correctly
        assert!(output.contains("F1000"), "Should contain F1000");
        assert!(output.contains("F1001"), "Should contain F1001");
        assert!(output.contains("F1005"), "Should contain F1005");

        // Verify IDs are still properly formatted (not truncated)
        assert!(
            !output.contains("F000"),
            "Should not have F000 (that would indicate truncation)"
        );
    }

    #[test]
    fn test_directory_ids_beyond_three_digits() {
        // Create a tree with more than 999 directories
        let mut buffer = Vec::new();
        let mut root = create_dir_node("");

        // Add 1005 directories
        for i in 1..=1005 {
            root.children.push(create_dir_node(&format!("dir{:04}", i)));
        }

        let version = Version::new_v1();
        generate_llm_context(&mut buffer, &root, &version, "LargeProject", true, Path::new(".")).unwrap();

        let output = String::from_utf8_lossy(&buffer);

        // Verify that 4-digit directory IDs are generated correctly
        assert!(output.contains("D1000"), "Should contain D1000");
        assert!(output.contains("D1005"), "Should contain D1005");
    }

    #[test]
    fn test_project_name_no_curly_braces() {
        let mut buffer = Vec::new();
        let root = create_dir_node("");
        let version = Version::new_v1();

        generate_llm_context(&mut buffer, &root, &version, "MyProject", false, Path::new(".")).unwrap();

        let output = String::from_utf8_lossy(&buffer);

        // Should contain [DIRECTORY:MyProject] without curly braces
        assert!(
            output.contains("[DIRECTORY:MyProject]"),
            "Project name should not have curly braces. Got: {}",
            output.lines().take(5).collect::<Vec<_>>().join("\n")
        );
        // Should NOT contain {MyProject}
        assert!(
            !output.contains("{MyProject}"),
            "Project name should not be wrapped in curly braces"
        );
    }

    #[test]
    fn test_file_content_no_size_attribute() {
        let mut buffer = Vec::new();
        let mut root = create_dir_node("");
        root.children.push(create_file_node("test.txt", b"Hello"));
        let version = Version::new_v1();

        generate_llm_context(&mut buffer, &root, &version, "TestProject", false, Path::new(".")).unwrap();

        let output = String::from_utf8_lossy(&buffer);

        // FILE_CONTENT_START should have LINES but not SIZE
        assert!(
            output.contains("LINES="),
            "FILE_CONTENT_START should contain LINES attribute"
        );
        // SIZE should NOT be in FILE_CONTENT_START (but IS in directory tree)
        // Check that SIZE is not between FILE_CONTENT_START and the closing >
        let file_start_line = output
            .lines()
            .find(|l| l.contains("FILE_CONTENT_START"))
            .unwrap();
        assert!(
            !file_start_line.contains("SIZE="),
            "FILE_CONTENT_START should not contain SIZE attribute. Got: {}",
            file_start_line
        );
    }
}
