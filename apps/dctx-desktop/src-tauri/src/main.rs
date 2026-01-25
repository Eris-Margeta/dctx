// apps/dctx-desktop/src-tauri/src/main.rs
//
// Tauri backend for dctx Desktop
// This is PROPRIETARY software - see LICENSE in this directory

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use dctx_core::{datatypes::DirContextTreeNode, ignore, walker, FilterConfig};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
struct SnapshotRequest {
    path: String,
    filter_extensions: Vec<String>,
    include_binary: bool,
    only_empty: bool,
    only_non_empty: bool,
    min_size_mb: Option<f64>,
    max_size_mb: Option<f64>,
    only_hidden: bool,
    tree_only: bool,
}

#[derive(Debug, Serialize)]
struct SnapshotResponse {
    success: bool,
    output: String,
    error: Option<String>,
}

/// Generate a directory context snapshot
#[tauri::command]
fn generate_snapshot(request: SnapshotRequest) -> Result<SnapshotResponse, String> {
    let path = Path::new(&request.path);

    if !path.exists() {
        return Ok(SnapshotResponse {
            success: false,
            output: String::new(),
            error: Some("Path does not exist".to_string()),
        });
    }

    // Build filter config
    let mut filter_config = FilterConfig::default()
        .with_extensions(request.filter_extensions)
        .with_include_binary(request.include_binary);

    if request.only_empty {
        filter_config = filter_config.with_only_empty(true);
    }
    if request.only_non_empty {
        filter_config = filter_config.with_only_non_empty(true);
    }
    if let Some(min) = request.min_size_mb {
        filter_config = filter_config.with_min_size_mb(min);
    }
    if let Some(max) = request.max_size_mb {
        filter_config = filter_config.with_max_size_mb(max);
    }
    if request.only_hidden {
        filter_config = filter_config.with_only_hidden(true);
    }

    // Load ignore rules
    let ignore_rules = ignore::load_ignore_rules(path);

    // Walk directory
    let mut tree = match walker::walk_directory_structure(path, &ignore_rules) {
        Ok(tree) => tree,
        Err(e) => {
            return Ok(SnapshotResponse {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to walk directory: {}", e)),
            });
        }
    };

    // Populate file contents in parallel
    walker::populate_file_contents_parallel(&mut tree);

    // Apply filters
    let tree_for_output = if filter_config.has_active_filters() {
        match walker::filter_tree(&tree, &filter_config) {
            Some(filtered) => filtered,
            None => {
                return Ok(SnapshotResponse {
                    success: false,
                    output: String::new(),
                    error: Some("All files filtered out".to_string()),
                });
            }
        }
    } else {
        tree
    };

    // For now, return a success message
    // TODO: Implement proper formatting using llm_formatter::generate_llm_context
    let file_count = count_files(&tree_for_output);
    let output = format!(
        "Snapshot generated successfully!\n\nPath: {}\nFiles: {}\nTree-only: {}\nFilters active: {}",
        path.display(),
        file_count,
        request.tree_only,
        filter_config.has_active_filters()
    );

    Ok(SnapshotResponse {
        success: true,
        output,
        error: None,
    })
}

/// Count files in tree
fn count_files(node: &DirContextTreeNode) -> usize {
    use dctx_core::datatypes::NodeType;

    let mut count = 0;
    if matches!(node.node_type, NodeType::File) {
        count = 1;
    }
    for child in &node.children {
        count += count_files(child);
    }
    count
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![generate_snapshot])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
