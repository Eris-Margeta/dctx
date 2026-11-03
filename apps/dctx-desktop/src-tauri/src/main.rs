// SPDX-License-Identifier: Apache-2.0
//! Tauri commands for dctx Desktop.
//! Implements ARCHITECTURE-PROPOSAL §7 row DX-E4 (second half, DX-E4b).
//! Depends on Tauri, tokio and `dctx_desktop_api`. The TypeScript UI invokes these command names.
//! `generate_snapshot` calls the seam inside `spawn_blocking`. No tree walk lives here.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]

mod config;
mod plugin_commands;
mod plugin_ports;
mod recent_projects;

use std::path::Path;

use config::DesktopConfig;
use dctx_desktop_api::SnapshotResponse;
use recent_projects::RecentProject;
use serde::Deserialize;

/// Path payload `main.ts` sends to `generate_snapshot`.
#[derive(Debug, Deserialize)]
struct SnapshotRequest {
    path: String,
}

/// Generate a directory context snapshot.
#[tauri::command]
async fn generate_snapshot(request: SnapshotRequest) -> Result<SnapshotResponse, String> {
    tokio::task::spawn_blocking(move || {
        let desktop_config = config::load_desktop_config();
        dctx_desktop_api::generate_snapshot(Path::new(&request.path), &desktop_config)
    })
    .await
    .map_err(|error| format!("Task failed: {error}"))
}

/// Copy text to the system clipboard.
#[tauri::command]
async fn copy_to_clipboard(app: tauri::AppHandle, text: String) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    app.clipboard()
        .write_text(text)
        .map_err(|error| format!("Failed to copy to clipboard: {error}"))
}

/// Get current desktop settings.
#[tauri::command]
async fn get_desktop_settings() -> Result<DesktopConfig, String> {
    Ok(config::load_desktop_config())
}

/// Save desktop settings to disk.
#[tauri::command]
async fn save_desktop_settings(config: DesktopConfig) -> Result<(), String> {
    config::save_desktop_config(&config)
}

/// Reset desktop settings to defaults.
#[tauri::command]
async fn reset_desktop_settings() -> Result<(), String> {
    config::reset_desktop_config()
}

/// Get the recent projects list.
#[tauri::command]
async fn get_recent_projects() -> Result<Vec<RecentProject>, String> {
    recent_projects::load_recent_projects()
}

/// Add a project to recent projects.
#[tauri::command]
async fn add_recent_project(path: String) -> Result<(), String> {
    recent_projects::add_recent_project(&path)
}

/// Remove a project from recent projects.
#[tauri::command]
async fn remove_recent_project(path: String) -> Result<(), String> {
    recent_projects::remove_recent_project(&path)
}

fn main() {
    if let Err(error) = config::ensure_config_dirs() {
        eprintln!("Warning: Failed to initialize config directories: {error}");
    }

    if let Err(error) = tauri::Builder::default()
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
            remove_recent_project,
            plugin_commands::list_plugins,
            plugin_commands::enable_plugin,
            plugin_commands::disable_plugin,
            plugin_commands::consent_plugin,
            plugin_commands::check_plugin_update
        ])
        .run(tauri::generate_context!())
    {
        eprintln!("error while running tauri application: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::SnapshotRequest;

    #[test]
    fn snapshot_request_reads_the_path_field() {
        let request: SnapshotRequest =
            serde_json::from_str(r#"{"path":"/work/app"}"#).expect("json");
        assert_eq!(request.path, "/work/app");
    }
}
