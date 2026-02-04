// apps/dctx-desktop/src-tauri/src/main.rs
//
// Tauri backend for dctx Desktop
// This is PROPRIETARY software - see LICENSE in this directory

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod recent_projects;

use config::{ensure_config_dirs, load_desktop_config, save_desktop_config, DesktopConfig};
use recent_projects::RecentProject;
use dctx_core::{
    content_reader::ContentReader,
    datatypes::DirContextTreeNode,
    ignore,
    llm_formatter,
    version::Version,
    walker,
    FilterConfig,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
struct SnapshotRequest {
    path: String,
}

#[derive(Debug, Serialize, Default)]
struct SnapshotResponse {
    success: bool,
    output: String,
    error: Option<String>,
    is_formatted: bool,
}

/// Generate a directory context snapshot
#[tauri::command]
async fn generate_snapshot(request: SnapshotRequest) -> Result<SnapshotResponse, String> {
    // Run the heavy work in a blocking task to avoid freezing the UI
    tokio::task::spawn_blocking(move || {
        // Load config (inherits from ~/.config/dctx/config.toml)
        let desktop_config = load_desktop_config();

        let path = Path::new(&request.path);

        if !path.exists() {
            return Ok(SnapshotResponse {
                success: false,
                output: String::new(),
                error: Some("Path does not exist".to_string()),
                is_formatted: false,
            });
        }

        // Safety check: prevent loading massive directories (from config)
        let file_count_limit = desktop_config.file_count_limit;

        // Build filter config from desktop config settings
        let mut filter_config = FilterConfig::default()
            .with_extensions(desktop_config.filter_extensions.clone().unwrap_or_default())
            .with_include_binary(desktop_config.include_binary);

        if desktop_config.only_empty {
            filter_config = filter_config.with_only_empty(true);
        }
        if desktop_config.only_non_empty {
            filter_config = filter_config.with_only_non_empty(true);
        }
        if let Some(min) = desktop_config.min_size_mb {
            filter_config = filter_config.with_min_size_mb(min);
        }
        if let Some(max) = desktop_config.max_size_mb {
            filter_config = filter_config.with_max_size_mb(max);
        }
        if desktop_config.only_hidden {
            filter_config = filter_config.with_only_hidden(true);
        }

        // Load ignore rules
        // Build IgnoreEngine (Phase 10.8.2: debug_verbose = false for GUI)
        let engine = match ignore::IgnoreEngine::build(path, false) {
            Ok(e) => e,
            Err(e) => {
                return Ok(SnapshotResponse {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Error loading ignore rules: {}", e)),
                    is_formatted: false,
                });
            }
        };

        // Walk directory (structure only, no contents yet)
        let tree = match walker::walk_directory_structure(path, &engine, false) {
            Ok(tree) => tree,
            Err(e) => {
                return Ok(SnapshotResponse {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to walk directory: {}", e)),
                    is_formatted: false,
                });
            }
        };

        // Count files BEFORE loading contents
        let total_files = count_files(&tree);

        // Safety check: prevent memory exhaustion
        if total_files > file_count_limit {
            return Ok(SnapshotResponse {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Directory too large: {} files (limit: {})\n\nPlease use filters or select a smaller directory.",
                    total_files,
                    file_count_limit
                )),
                is_formatted: false,
            });
        }

        // Apply filters first (before loading contents)
        let tree_for_output = if filter_config.has_active_filters() {
            match walker::filter_tree(&tree, &filter_config) {
                Some(filtered) => filtered,
                None => {
                    return Ok(SnapshotResponse {
                        success: false,
                        output: String::new(),
                        error: Some("All files filtered out".to_string()),
                        is_formatted: false,
                    });
                }
            }
        } else {
            tree
        };

        // Read file contents (respects handling_decision for privacy)
        let mut tree_with_content = tree_for_output.clone();
        if !desktop_config.tree_only {
            ContentReader::populate(&mut tree_with_content);
        }

        // Generate formatted output using llm_formatter
        let mut output_buffer = Vec::new();
        let version = Version::new_v1(); // Desktop doesn't use version tracking yet
        let project_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project");

        let result = llm_formatter::generate_llm_context(
            &mut output_buffer,
            &tree_with_content,
            &version,
            project_name,
            desktop_config.tree_only,
            path,
        );

        match result {
            Ok(_) => {
                let output = String::from_utf8_lossy(&output_buffer).to_string();
                Ok(SnapshotResponse {
                    success: true,
                    output,
                    error: None,
                    is_formatted: true,
                })
            }
            Err(e) => Ok(SnapshotResponse {
                success: false,
                error: Some(format!("Failed to format output: {}", e)),
                ..Default::default()
            }),
        }
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
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

/// Copy text to system clipboard (cross-platform)
#[tauri::command]
async fn copy_to_clipboard(
    app: tauri::AppHandle,
    text: String,
) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    app.clipboard()
        .write_text(text)
        .map_err(|e| format!("Failed to copy to clipboard: {}", e))
}

/// Get current desktop settings
#[tauri::command]
async fn get_desktop_settings() -> Result<DesktopConfig, String> {
    Ok(load_desktop_config())
}

/// Save desktop settings to disk
#[tauri::command]
async fn save_desktop_settings(config: DesktopConfig) -> Result<(), String> {
    save_desktop_config(&config)
}

/// Reset desktop settings to defaults
#[tauri::command]
async fn reset_desktop_settings() -> Result<(), String> {
    let default_config = DesktopConfig::default();
    save_desktop_config(&default_config)
}

/// Get recent projects list
#[tauri::command]
async fn get_recent_projects() -> Result<Vec<RecentProject>, String> {
    recent_projects::load_recent_projects()
}

/// Add a project to recent projects
#[tauri::command]
async fn add_recent_project(path: String) -> Result<(), String> {
    recent_projects::add_recent_project(&path)
}

/// Remove a project from recent projects
#[tauri::command]
async fn remove_recent_project(path: String) -> Result<(), String> {
    recent_projects::remove_recent_project(&path)
}

fn main() {
    // Initialize config directories on startup
    if let Err(e) = ensure_config_dirs() {
        eprintln!("Warning: Failed to initialize config directories: {}", e);
    }

    // Load config to verify it works
    let config = load_desktop_config();
    println!("Loaded desktop config:");
    println!("  Config dir: {:?}", config::get_desktop_config_dir());
    println!("  Cache dir: {:?}", config::get_desktop_cache_dir());
    println!("  File limit: {}", config.file_count_limit);
    println!(
        "  Core config inherited: text_output={}",
        config.core.text_output_enabled
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            generate_snapshot,
            copy_to_clipboard,
            get_desktop_settings,
            save_desktop_settings,
            reset_desktop_settings,
            get_recent_projects,
            add_recent_project,
            remove_recent_project
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
