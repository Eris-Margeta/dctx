use std::env;
use std::fs;
use std::path::PathBuf;

// --- CONSTANTS ---

/// Magic Bytes identifying the format (8 bytes).
pub const DIRCONTXT_MAGIC_BYTES: &[u8; 8] = b"DIRCTXTV";

/// Binary file extension (always .dctx).
pub const BINARY_FILE_EXTENSION: &str = "dctx";

/// Text context extension.
pub const LLM_CONTEXT_EXTENSION: &str = "llmcontext.txt";

/// Local ignore filename.
pub const DEFAULT_IGNORE_FILENAME: &str = ".dctxignore";

/// Application name (folder for config/cache).
pub const APP_NAME: &str = "dctx";

/// Global ignore filename inside config dir.
pub const GLOBAL_IGNORE_FILENAME: &str = "ignore";

/// Configuration filename inside config dir.
pub const CONFIG_FILENAME: &str = "config.toml";

// --- LAYER 1: HARDCODED RULES ---
// Base rules that apply universally unless negated.
pub const HARDCODED_IGNORE_RULES: &[&str] = &[
    // Version Control
    ".git/",
    ".svn/",
    ".hg/",
    // Dependencies
    "node_modules/",
    "bower_components/",
    // System & IDE
    ".DS_Store",
    "Thumbs.db",
    // Build Output (Generic)
    "dist/",
    "build/",
    "target/", // Rust specific
    "bin/",
    "obj/",
    // Python
    ".venv/",
    "venv/",
    "__pycache__/",
    "*.pyc",
    // Self-referential (prevent tools scanning themselves)
    ".dctxignore",
    "*.dctx",
];

// --- CONFIG STRUCT ---

#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Controls whether text output files (.llmcontext.txt) are written to the
    /// local .dctx/ directory. Binary state is ALWAYS written to system cache.
    /// Default is TRUE for backward compatibility.
    pub text_output_enabled: bool,
    pub max_diff_files: u32,
    pub default_template: String,
    /// Controls whether dctx automatically updates the local .gitignore file
    /// with dctx-specific rules. Defined in ADR-005.
    pub update_local_git_repo_gitignore_rules: bool,
    /// Controls whether diff files (*-diff.txt) are generated on disk.
    /// Added in Phase 5.2. Default is FALSE to save disk space.
    pub text_output_mode_generates_diff_files: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            text_output_enabled: true, // Default: write text files to .dctx/
            max_diff_files: 999,
            default_template: "default".to_string(),
            update_local_git_repo_gitignore_rules: true, // Safe default per ADR-005
            text_output_mode_generates_diff_files: false, // Default per Phase 5.2
        }
    }
}

// --- PATH HELPERS ---

pub enum StorageStrategy {
    Local,       // .dctx inside project
    SystemCache, // ~/.cache/dctx/HASH.dctx
}

pub fn get_config_dir() -> Option<PathBuf> {
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

pub fn get_project_registry_path() -> Option<PathBuf> {
    // UPDATED: Now uses cache dir instead of config dir
    get_cache_dir().map(|p| p.join("projects.db"))
}

pub fn get_config_file_path() -> Option<PathBuf> {
    get_config_dir().map(|p| p.join(CONFIG_FILENAME))
}

// --- PARSER (Zero Dep) ---

pub fn load_app_config() -> AppConfig {
    let mut config = AppConfig::default();

    let config_path = match get_config_file_path() {
        Some(p) => p,
        None => return config,
    };

    if !config_path.exists() {
        return config;
    }

    if let Ok(content) = fs::read_to_string(config_path) {
        for line in content.lines() {
            parse_config_line(line, &mut config);
        }
    }

    config
}

fn parse_config_line(line: &str, config: &mut AppConfig) {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return;
    }

    if let Some((key, val)) = line.split_once('=') {
        let key = key.trim();
        let val = val.trim().trim_matches('"').trim_matches('\''); // Strip quotes

        match key {
            // Legacy support: map old output_mode values to new boolean
            "output_mode" => match val {
                "binary" => config.text_output_enabled = false,
                _ => config.text_output_enabled = true, // "both" or "text" => enable text
            },
            // New config key (preferred)
            "text_output_enabled" => {
                config.text_output_enabled = val == "true";
            }
            "max_diff_files" => {
                if let Ok(num) = val.parse::<u32>() {
                    config.max_diff_files = num;
                }
            }
            "default_template" => {
                config.default_template = val.to_string();
            }
            "update_local_git_repo_gitignore_rules" => {
                if val == "true" {
                    config.update_local_git_repo_gitignore_rules = true;
                } else if val == "false" {
                    config.update_local_git_repo_gitignore_rules = false;
                }
            }
            "text_output_mode_generates_diff_files" => {
                if val == "true" {
                    config.text_output_mode_generates_diff_files = true;
                } else if val == "false" {
                    config.text_output_mode_generates_diff_files = false;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert!(config.text_output_enabled);
        assert_eq!(config.max_diff_files, 999);
        assert_eq!(config.default_template, "default");
        assert!(config.update_local_git_repo_gitignore_rules);
        assert!(!config.text_output_mode_generates_diff_files);
    }

    #[test]
    fn test_parse_text_output_enabled_true() {
        let mut config = AppConfig::default();
        config.text_output_enabled = false; // Start with false
        parse_config_line("text_output_enabled = true", &mut config);
        assert!(config.text_output_enabled);
    }

    #[test]
    fn test_parse_text_output_enabled_false() {
        let mut config = AppConfig::default();
        parse_config_line("text_output_enabled = false", &mut config);
        assert!(!config.text_output_enabled);
    }

    #[test]
    fn test_parse_legacy_output_mode_binary() {
        let mut config = AppConfig::default();
        parse_config_line("output_mode = \"binary\"", &mut config);
        assert!(!config.text_output_enabled);
    }

    #[test]
    fn test_parse_legacy_output_mode_both() {
        let mut config = AppConfig::default();
        config.text_output_enabled = false;
        parse_config_line("output_mode = \"both\"", &mut config);
        assert!(config.text_output_enabled);
    }

    #[test]
    fn test_parse_legacy_output_mode_text() {
        let mut config = AppConfig::default();
        config.text_output_enabled = false;
        parse_config_line("output_mode = \"text\"", &mut config);
        assert!(config.text_output_enabled);
    }

    #[test]
    fn test_parse_max_diff_files() {
        let mut config = AppConfig::default();
        parse_config_line("max_diff_files = 500", &mut config);
        assert_eq!(config.max_diff_files, 500);
    }

    #[test]
    fn test_parse_max_diff_files_invalid() {
        let mut config = AppConfig::default();
        parse_config_line("max_diff_files = not_a_number", &mut config);
        assert_eq!(config.max_diff_files, 999); // Should remain default
    }

    #[test]
    fn test_parse_default_template() {
        let mut config = AppConfig::default();
        parse_config_line("default_template = \"custom\"", &mut config);
        assert_eq!(config.default_template, "custom");
    }

    #[test]
    fn test_parse_default_template_single_quotes() {
        let mut config = AppConfig::default();
        parse_config_line("default_template = 'my-template'", &mut config);
        assert_eq!(config.default_template, "my-template");
    }

    #[test]
    fn test_parse_gitignore_rules_true() {
        let mut config = AppConfig::default();
        config.update_local_git_repo_gitignore_rules = false;
        parse_config_line("update_local_git_repo_gitignore_rules = true", &mut config);
        assert!(config.update_local_git_repo_gitignore_rules);
    }

    #[test]
    fn test_parse_gitignore_rules_false() {
        let mut config = AppConfig::default();
        parse_config_line("update_local_git_repo_gitignore_rules = false", &mut config);
        assert!(!config.update_local_git_repo_gitignore_rules);
    }

    #[test]
    fn test_parse_diff_files_toggle() {
        let mut config = AppConfig::default();
        parse_config_line("text_output_mode_generates_diff_files = true", &mut config);
        assert!(config.text_output_mode_generates_diff_files);
    }

    #[test]
    fn test_parse_ignores_comments() {
        let mut config = AppConfig::default();
        parse_config_line("# This is a comment", &mut config);
        parse_config_line("  # Indented comment", &mut config);
        // Should not change anything
        assert!(config.text_output_enabled);
        assert_eq!(config.max_diff_files, 999);
    }

    #[test]
    fn test_parse_ignores_empty_lines() {
        let mut config = AppConfig::default();
        parse_config_line("", &mut config);
        parse_config_line("   ", &mut config);
        // Should not change anything
        assert!(config.text_output_enabled);
    }

    #[test]
    fn test_parse_ignores_unknown_keys() {
        let mut config = AppConfig::default();
        parse_config_line("unknown_key = some_value", &mut config);
        // Should not panic or change known values
        assert!(config.text_output_enabled);
    }

    #[test]
    fn test_hardcoded_ignore_rules_not_empty() {
        assert!(!HARDCODED_IGNORE_RULES.is_empty());
        assert!(HARDCODED_IGNORE_RULES.contains(&".git/"));
        assert!(HARDCODED_IGNORE_RULES.contains(&"node_modules/"));
        assert!(HARDCODED_IGNORE_RULES.contains(&"target/"));
    }

    #[test]
    fn test_constants() {
        assert_eq!(DIRCONTXT_MAGIC_BYTES, b"DIRCTXTV");
        assert_eq!(BINARY_FILE_EXTENSION, "dctx");
        assert_eq!(LLM_CONTEXT_EXTENSION, "llmcontext.txt");
        assert_eq!(DEFAULT_IGNORE_FILENAME, ".dctxignore");
        assert_eq!(APP_NAME, "dctx");
        assert_eq!(GLOBAL_IGNORE_FILENAME, "ignore");
        assert_eq!(CONFIG_FILENAME, "config.toml");
    }
}
