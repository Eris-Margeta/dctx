//! Desktop directory discovery and settings command wrappers.
//! Implements ARCHITECTURE-PROPOSAL §7 row DX-E4 (second half, DX-E4b).
//! Depends on `dctx_desktop_api` for settings and on `dirs` for the config and cache directories.
//! `main.rs` calls these wrappers. Parsing and defaults stay in the seam crate.

use std::path::PathBuf;

use dctx_desktop_api::{DesktopError, DESKTOP_APP_NAME};

/// Desktop settings JSON the UI reads and writes.
pub use dctx_desktop_api::DesktopConfig;

/// Joins the desktop app directory onto a platform root from `dirs`.
fn app_dir(root: PathBuf) -> PathBuf {
    root.join(DESKTOP_APP_NAME)
}

/// Config directory under the platform config root.
pub fn desktop_config_dir() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(app_dir)
        .ok_or_else(|| "Failed to get config directory".to_string())
}

/// Cache directory under the platform cache root.
pub fn desktop_cache_dir() -> Result<PathBuf, String> {
    dirs::cache_dir()
        .map(app_dir)
        .ok_or_else(|| "Failed to get cache directory".to_string())
}

fn command_error(error: DesktopError) -> String {
    error.command_string()
}

/// Loads settings from the desktop config directory.
///
/// A missing directory is [`DesktopConfig::default`], the same outcome as a missing file.
pub fn load_desktop_config() -> DesktopConfig {
    desktop_config_dir().map_or_else(
        |_| DesktopConfig::default(),
        |dir| dctx_desktop_api::load_desktop_config(&dir),
    )
}

/// Writes `config` into the desktop config directory.
pub fn save_desktop_config(config: &DesktopConfig) -> Result<(), String> {
    let dir = desktop_config_dir()?;
    dctx_desktop_api::save_desktop_config(&dir, config).map_err(command_error)
}

/// Writes default settings into the desktop config directory.
pub fn reset_desktop_config() -> Result<(), String> {
    let dir = desktop_config_dir()?;
    dctx_desktop_api::reset_desktop_config(&dir).map_err(command_error)
}

/// Creates the desktop config and cache directories.
pub fn ensure_config_dirs() -> Result<(), String> {
    let config_dir = desktop_config_dir()?;
    let cache_dir = desktop_cache_dir()?;
    dctx_desktop_api::ensure_config_dirs(&config_dir, &cache_dir).map_err(command_error)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use dctx_desktop_api::{DesktopError, DESKTOP_APP_NAME};

    use super::{app_dir, command_error};

    #[test]
    fn app_dir_joins_the_desktop_name() {
        let root = PathBuf::from("/tmp/config");
        assert_eq!(app_dir(root.clone()), root.join(DESKTOP_APP_NAME));
    }

    #[test]
    fn settings_errors_use_the_api_command_string() {
        let error = DesktopError::CreateDesktopConfigDir {
            detail: "denied".to_string(),
        };
        assert_eq!(
            command_error(error),
            "Failed to create desktop config dir: denied"
        );
    }
}
