//! Desktop settings: defaults, line parse, save and reset.
//! Implements ARCHITECTURE-PROPOSAL §7 row DX-E4a, matching `config.rs` field defaults.
//! Depends on [`crate::error::DesktopError`] and serde. The Tauri layer injects both directories.
//! This module does not read environment variables or a home directory.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::DesktopError;

/// Directory name the desktop uses under an injected config root.
pub const DESKTOP_APP_NAME: &str = "dctx-desktop";

/// File name inside the injected config directory.
pub const DESKTOP_CONFIG_FILENAME: &str = "config.toml";

/// Desktop window, filter and snapshot preferences.
///
/// The core `dctx` config stays with the caller. This struct is the desktop file only,
/// which is the JSON shape `settings-manager.ts` reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesktopConfig {
    /// Last directory the user opened.
    pub last_directory: Option<String>,
    /// Window width in pixels.
    pub window_width: u32,
    /// Window height in pixels.
    pub window_height: u32,
    /// Maximum files a snapshot may count before it refuses.
    pub file_count_limit: usize,
    /// `system`, `light` or `dark`.
    pub theme: String,
    /// Copy a successful snapshot to the clipboard. The crate does not touch the clipboard.
    pub auto_copy_to_clipboard: bool,
    /// Extensions to keep, without dots. `None` or empty means no extension filter.
    pub filter_extensions: Option<Vec<String>>,
    /// When other filters are active, also keep files the caller marked binary.
    pub include_binary: bool,
    /// Keep only names that start with `.`.
    pub only_hidden: bool,
    /// Minimum file size in megabytes.
    pub min_size_mb: Option<f64>,
    /// Maximum file size in megabytes.
    pub max_size_mb: Option<f64>,
    /// Keep only empty files.
    pub only_empty: bool,
    /// Keep only non-empty files.
    pub only_non_empty: bool,
    /// Ask the snapshot for a metadata page instead of pack contents.
    pub tree_only: bool,
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            last_directory: None,
            window_width: 1200,
            window_height: 800,
            file_count_limit: 10_000,
            theme: "system".to_string(),
            auto_copy_to_clipboard: false,
            filter_extensions: None,
            include_binary: false,
            only_hidden: false,
            min_size_mb: None,
            max_size_mb: None,
            only_empty: false,
            only_non_empty: false,
            tree_only: false,
        }
    }
}

impl DesktopConfig {
    /// True when the snapshot should apply the desktop filter predicates.
    pub fn has_active_filters(&self) -> bool {
        self.filter_extensions.as_ref().is_some_and(|ext| !ext.is_empty())
            || self.include_binary
            || self.only_empty
            || self.only_non_empty
            || self.min_size_mb.is_some()
            || self.max_size_mb.is_some()
            || self.only_hidden
    }
}

/// Creates the injected config and cache directories.
pub fn ensure_config_dirs(config_dir: &Path, cache_dir: &Path) -> Result<(), DesktopError> {
    fs::create_dir_all(config_dir)
        .map_err(|error| DesktopError::CreateDesktopConfigDir { detail: error.to_string() })?;
    fs::create_dir_all(cache_dir)
        .map_err(|error| DesktopError::CreateDesktopCacheDir { detail: error.to_string() })?;
    Ok(())
}

/// Loads `config.toml` from `config_dir`. A missing or unreadable file is the default.
///
/// Blank lines, comments, unknown keys and values that do not parse are ignored.
pub fn load_desktop_config(config_dir: &Path) -> DesktopConfig {
    let mut config = DesktopConfig::default();
    let path = config_dir.join(DESKTOP_CONFIG_FILENAME);
    let Ok(content) = fs::read_to_string(path) else {
        return config;
    };
    for line in content.lines() {
        apply_config_line(line, &mut config);
    }
    config
}

/// Writes `config` into `config_dir/config.toml`, creating the directory when needed.
pub fn save_desktop_config(config_dir: &Path, config: &DesktopConfig) -> Result<(), DesktopError> {
    fs::create_dir_all(config_dir)
        .map_err(|error| DesktopError::CreateConfigDir { detail: error.to_string() })?;
    let path = config_dir.join(DESKTOP_CONFIG_FILENAME);
    fs::write(path, render_config(config))
        .map_err(|error| DesktopError::WriteConfig { detail: error.to_string() })
}

/// Saves [`DesktopConfig::default`] into `config_dir`.
pub fn reset_desktop_config(config_dir: &Path) -> Result<(), DesktopError> {
    save_desktop_config(config_dir, &DesktopConfig::default())
}

fn apply_config_line(line: &str, config: &mut DesktopConfig) {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return;
    }
    let Some((key, val)) = line.split_once('=') else {
        return;
    };
    let key = key.trim();
    let val = val.trim().trim_matches('"').trim_matches('\'');
    match key {
        "last_directory" => config.last_directory = Some(val.to_string()),
        "window_width" => assign_parsed(val, &mut config.window_width),
        "window_height" => assign_parsed(val, &mut config.window_height),
        "file_count_limit" => assign_parsed(val, &mut config.file_count_limit),
        "theme" => config.theme = val.to_string(),
        "auto_copy_to_clipboard" => config.auto_copy_to_clipboard = val == "true",
        "filter_extensions" => {
            if !val.is_empty() {
                config.filter_extensions = Some(
                    val.split(',').map(|part| part.trim().to_string()).filter(|part| !part.is_empty()).collect(),
                );
            }
        }
        "include_binary" => config.include_binary = val == "true",
        "only_hidden" => config.only_hidden = val == "true",
        "min_size_mb" => assign_parsed_some(val, &mut config.min_size_mb),
        "max_size_mb" => assign_parsed_some(val, &mut config.max_size_mb),
        "only_empty" => config.only_empty = val == "true",
        "only_non_empty" => config.only_non_empty = val == "true",
        "tree_only" => config.tree_only = val == "true",
        _ => {}
    }
}

fn assign_parsed<T: std::str::FromStr>(val: &str, slot: &mut T) {
    if let Ok(parsed) = val.parse::<T>() {
        *slot = parsed;
    }
}

fn assign_parsed_some<T: std::str::FromStr>(val: &str, slot: &mut Option<T>) {
    if let Ok(parsed) = val.parse::<T>() {
        *slot = Some(parsed);
    }
}

fn render_config(config: &DesktopConfig) -> String {
    let mut content = String::new();
    content.push_str("# dctx Desktop Configuration\n");
    content.push_str("# This file is auto-generated\n\n");
    content.push_str("# Desktop-specific settings\n");
    if let Some(dir) = &config.last_directory {
        content.push_str(&format!("last_directory = \"{dir}\"\n"));
    }
    content.push_str(&format!("window_width = {}\n", config.window_width));
    content.push_str(&format!("window_height = {}\n", config.window_height));
    content.push_str(&format!("file_count_limit = {}\n", config.file_count_limit));
    content.push_str(&format!("theme = \"{}\"\n", config.theme));
    content.push_str(&format!("auto_copy_to_clipboard = {}\n", config.auto_copy_to_clipboard));
    content.push_str("\n# Filter settings\n");
    if let Some(exts) = &config.filter_extensions {
        content.push_str(&format!("filter_extensions = \"{}\"\n", exts.join(",")));
    }
    content.push_str(&format!("include_binary = {}\n", config.include_binary));
    content.push_str(&format!("only_hidden = {}\n", config.only_hidden));
    if let Some(min) = config.min_size_mb {
        content.push_str(&format!("min_size_mb = {min}\n"));
    }
    if let Some(max) = config.max_size_mb {
        content.push_str(&format!("max_size_mb = {max}\n"));
    }
    content.push_str(&format!("only_empty = {}\n", config.only_empty));
    content.push_str(&format!("only_non_empty = {}\n", config.only_non_empty));
    content.push_str(&format!("tree_only = {}\n", config.tree_only));
    content
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let path = std::env::temp_dir().join(format!("dctx-desktop-api-settings-{label}-{n}-{nanos}"));
        fs::create_dir_all(&path).expect("temp");
        path
    }

    #[test]
    fn malformed_lines_are_ignored_and_valid_lines_apply() {
        let dir = scratch("malformed");
        let raw = "\
# comment
not a pair
unknown_key = 1
window_width = nope
window_height = 640
theme = \"dark\"
# theme = \"light\"
file_count_limit = 42
";
        fs::write(dir.join(DESKTOP_CONFIG_FILENAME), raw).expect("write");
        let loaded = load_desktop_config(&dir);
        assert_eq!(loaded.window_width, 1200);
        assert_eq!(loaded.window_height, 640);
        assert_eq!(loaded.theme, "dark");
        assert_eq!(loaded.file_count_limit, 42);
        assert!(!loaded.has_active_filters());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_load_and_reset_round_trip() {
        let dir = scratch("round");
        let mut config = DesktopConfig::default();
        config.last_directory = Some("/work/app".to_string());
        config.window_width = 1024;
        config.window_height = 768;
        config.file_count_limit = 50;
        config.theme = "light".to_string();
        config.auto_copy_to_clipboard = true;
        config.filter_extensions = Some(vec!["rs".to_string(), "toml".to_string()]);
        config.include_binary = true;
        config.only_hidden = true;
        config.min_size_mb = Some(1.5);
        config.max_size_mb = Some(4.0);
        config.only_empty = true;
        config.only_non_empty = false;
        config.tree_only = true;
        save_desktop_config(&dir, &config).expect("save");
        let loaded = load_desktop_config(&dir);
        assert_eq!(loaded, config);
        assert!(loaded.has_active_filters());
        reset_desktop_config(&dir).expect("reset");
        assert_eq!(load_desktop_config(&dir), DesktopConfig::default());
        let _ = fs::remove_dir_all(&dir);
    }
}
