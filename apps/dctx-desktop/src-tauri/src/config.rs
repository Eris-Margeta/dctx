// apps/dctx-desktop/src-tauri/src/config.rs
//
// Desktop-specific configuration that extends dctx-core config

use dctx_core::config::{get_config_dir as get_core_config_dir, load_app_config, AppConfig};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

/// Desktop app name (separate from core "dctx")
pub const DESKTOP_APP_NAME: &str = "dctx-desktop";

/// Desktop config filename
pub const DESKTOP_CONFIG_FILENAME: &str = "config.toml";

#[derive(Debug, Clone)]
pub struct DesktopConfig {
    /// Inherit core dctx config (filters, ignore rules, etc.)
    /// Not serialized - loaded from ~/.config/dctx/config.toml
    pub core: AppConfig,

    /// Desktop-specific: Last used directory
    pub last_directory: Option<String>,

    /// Desktop-specific: Window size
    pub window_width: u32,
    pub window_height: u32,

    /// Desktop-specific: File count warning threshold
    pub file_count_limit: usize,

    /// Desktop-specific: Theme preference (system, light, dark)
    pub theme: String,
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            core: AppConfig::default(),
            last_directory: None,
            window_width: 1200,
            window_height: 800,
            file_count_limit: 10_000,
            theme: "system".to_string(),
        }
    }
}

/// Get desktop app config directory
/// ~/.config/dctx-desktop/ (Unix)
/// %APPDATA%/dctx-desktop/ (Windows)
pub fn get_desktop_config_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        env::var("APPDATA")
            .ok()
            .map(|p| PathBuf::from(p).join(DESKTOP_APP_NAME))
    } else {
        if let Ok(xdg_config) = env::var("XDG_CONFIG_HOME") {
            Some(PathBuf::from(xdg_config).join(DESKTOP_APP_NAME))
        } else {
            env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".config").join(DESKTOP_APP_NAME))
        }
    }
}

/// Get desktop app cache directory
pub fn get_desktop_cache_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        env::var("LOCALAPPDATA")
            .ok()
            .map(|p| PathBuf::from(p).join(DESKTOP_APP_NAME))
    } else {
        if let Ok(xdg_cache) = env::var("XDG_CACHE_HOME") {
            Some(PathBuf::from(xdg_cache).join(DESKTOP_APP_NAME))
        } else {
            env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".cache").join(DESKTOP_APP_NAME))
        }
    }
}

/// Load desktop config, inheriting from core dctx config
pub fn load_desktop_config() -> DesktopConfig {
    let mut config = DesktopConfig::default();

    // 1. Load core dctx config first (if exists)
    config.core = load_app_config();

    // 2. Load desktop-specific config (overrides core where applicable)
    if let Some(config_dir) = get_desktop_config_dir() {
        let config_path = config_dir.join(DESKTOP_CONFIG_FILENAME);

        if config_path.exists() {
            if let Ok(content) = fs::read_to_string(config_path) {
                for line in content.lines() {
                    parse_desktop_config_line(line, &mut config);
                }
            }
        }
    }

    config
}

fn parse_desktop_config_line(line: &str, config: &mut DesktopConfig) {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return;
    }

    if let Some((key, val)) = line.split_once('=') {
        let key = key.trim();
        let val = val.trim().trim_matches('"').trim_matches('\'');

        match key {
            "last_directory" => {
                config.last_directory = Some(val.to_string());
            }
            "window_width" => {
                if let Ok(num) = val.parse::<u32>() {
                    config.window_width = num;
                }
            }
            "window_height" => {
                if let Ok(num) = val.parse::<u32>() {
                    config.window_height = num;
                }
            }
            "file_count_limit" => {
                if let Ok(num) = val.parse::<usize>() {
                    config.file_count_limit = num;
                }
            }
            "theme" => {
                config.theme = val.to_string();
            }
            _ => {
                // Unknown key - ignore
            }
        }
    }
}

/// Save desktop config to disk
pub fn save_desktop_config(config: &DesktopConfig) -> Result<(), String> {
    let config_dir = get_desktop_config_dir().ok_or("Failed to get config directory")?;

    // Ensure directory exists
    fs::create_dir_all(&config_dir).map_err(|e| format!("Failed to create config dir: {}", e))?;

    let config_path = config_dir.join(DESKTOP_CONFIG_FILENAME);

    let mut content = String::new();
    content.push_str("# dctx Desktop Configuration\n");
    content.push_str("# This file is auto-generated\n\n");

    content.push_str("# Desktop-specific settings\n");
    if let Some(ref dir) = config.last_directory {
        content.push_str(&format!("last_directory = \"{}\"\n", dir));
    }
    content.push_str(&format!("window_width = {}\n", config.window_width));
    content.push_str(&format!("window_height = {}\n", config.window_height));
    content.push_str(&format!("file_count_limit = {}\n", config.file_count_limit));
    content.push_str(&format!("theme = \"{}\"\n", config.theme));

    fs::write(config_path, content).map_err(|e| format!("Failed to write config: {}", e))?;

    Ok(())
}

/// Initialize config directories on first run
pub fn ensure_config_dirs() -> Result<(), String> {
    if let Some(config_dir) = get_desktop_config_dir() {
        fs::create_dir_all(&config_dir)
            .map_err(|e| format!("Failed to create desktop config dir: {}", e))?;
    }

    if let Some(cache_dir) = get_desktop_cache_dir() {
        fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create desktop cache dir: {}", e))?;
    }

    // Also ensure core dctx directories exist
    if let Some(core_config_dir) = get_core_config_dir() {
        fs::create_dir_all(&core_config_dir)
            .map_err(|e| format!("Failed to create core config dir: {}", e))?;
    }

    Ok(())
}
