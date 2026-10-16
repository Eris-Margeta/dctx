//! Legacy compatibility (DX-B0 bridge): config/cache directory discovery and `config.toml` loading.
//! Reads the environment and the filesystem, so it is IO and not part of the pure core.
//! Delegates parsing to [`crate::settings`] and re-exports the built-in rules from [`crate::selection`].
//! Used by dctx-cli, the desktop and the other legacy modules until the facade replaces them.

use std::env;
use std::fs;
use std::path::PathBuf;

pub use crate::selection::defaults::{HARDCODED_IGNORE_RULES, HARDCODED_OMIT_CONTENT_RULES};
pub use crate::settings::{
    AppConfig, APP_NAME, BINARY_FILE_EXTENSION, CONFIG_FILENAME, DEFAULT_IGNORE_FILENAME,
    DEFAULT_OMIT_CONTENT_FILENAME, DIRCONTXT_MAGIC_BYTES, GLOBAL_IGNORE_FILENAME,
    GLOBAL_OMIT_CONTENT_FILENAME, LLM_CONTEXT_EXTENSION,
};

// --- PATH HELPERS ---

pub enum StorageStrategy {
    Local,       // .dctx inside project
    SystemCache, // ~/.cache/dctx/HASH.dctx
}

pub fn get_config_dir() -> Option<PathBuf> {
    // Unit tests never read the machine's global config: they get a
    // process-wide directory seeded from the shipped templates.
    #[cfg(test)]
    {
        return Some(test_config_dir().clone());
    }
    #[allow(unreachable_code)]
    if cfg!(target_os = "windows") {
        env::var("APPDATA")
            .ok()
            .map(|p| PathBuf::from(p).join(APP_NAME))
    } else {
        // Unix-like (Linux, macOS)
        if let Ok(xdg_config) = env::var("XDG_CONFIG_HOME") {
            Some(PathBuf::from(xdg_config).join(APP_NAME))
        } else {
            env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".config").join(APP_NAME))
        }
    }
}

/// The hermetic config dir for unit tests: the global ignore and omit files
/// are the templates `dctx` installs, so results match a fresh install on any machine.
#[cfg(test)]
fn test_config_dir() -> &'static PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("dctx-test-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test config dir");
        std::fs::write(dir.join(GLOBAL_IGNORE_FILENAME), include_str!("../../templates/global-ignore"))
            .expect("write test global ignore");
        std::fs::write(dir.join(GLOBAL_OMIT_CONTENT_FILENAME), include_str!("../../templates/global-omitcontent"))
            .expect("write test global omit");
        dir
    })
}

pub fn get_cache_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        // Windows: Local AppData is the standard for non-roaming caches.
        env::var("LOCALAPPDATA")
            .ok()
            .map(|p| PathBuf::from(p).join(APP_NAME))
    } else {
        // Unix-like (Linux, macOS, BSD)
        // Unified approach: ~/.cache/dctx
        if let Ok(xdg_cache) = env::var("XDG_CACHE_HOME") {
            Some(PathBuf::from(xdg_cache).join(APP_NAME))
        } else {
            env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".cache").join(APP_NAME))
        }
    }
}

pub fn get_global_ignore_path() -> Option<PathBuf> {
    get_config_dir().map(|p| p.join(GLOBAL_IGNORE_FILENAME))
}

pub fn get_global_omit_content_path() -> Option<PathBuf> {
    get_config_dir().map(|p| p.join(GLOBAL_OMIT_CONTENT_FILENAME))
}

pub fn get_project_registry_path() -> Option<PathBuf> {
    // UPDATED: Now uses cache dir instead of config dir
    get_cache_dir().map(|p| p.join("projects.db"))
}

pub fn get_config_file_path() -> Option<PathBuf> {
    get_config_dir().map(|p| p.join(CONFIG_FILENAME))
}

/// Returns the path to the compiled templates directory.
/// Located in cache: ~/.cache/dctx/templates/
pub fn get_compiled_templates_dir() -> Option<PathBuf> {
    get_cache_dir().map(|p| p.join("templates"))
}

/// Returns the path to a specific compiled template binary file.
/// Example: ~/.cache/dctx/templates/my_template.bin
pub fn get_compiled_template_path(template_name: &str) -> Option<PathBuf> {
    get_compiled_templates_dir().map(|p| p.join(format!("{}.bin", template_name)))
}

/// Loads `config.toml` from the config directory; a missing or unreadable file yields defaults.
pub fn load_app_config() -> AppConfig {
    let config_path = match get_config_file_path() {
        Some(p) => p,
        None => return AppConfig::default(),
    };

    if !config_path.exists() {
        return AppConfig::default();
    }

    match fs::read_to_string(config_path) {
        Ok(content) => crate::settings::parse_config(&content),
        Err(_) => AppConfig::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hardcoded_rules_reexported() {
        assert!(HARDCODED_IGNORE_RULES.contains(&".git/"));
        assert!(HARDCODED_OMIT_CONTENT_RULES.contains(&"Cargo.lock"));
    }

    #[test]
    fn test_hermetic_config_dir_holds_templates() {
        let ignore = get_global_ignore_path().unwrap();
        let omit = get_global_omit_content_path().unwrap();
        assert!(ignore.exists());
        assert!(omit.exists());
        assert_eq!(get_config_file_path().unwrap().file_name().unwrap(), CONFIG_FILENAME);
    }

    #[test]
    fn test_compiled_template_path_shape() {
        if let Some(path) = get_compiled_template_path("x") {
            assert!(path.ends_with("templates/x.bin"));
        }
    }
}
