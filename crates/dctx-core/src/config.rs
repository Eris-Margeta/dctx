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
    pub output_mode: OutputMode,
    pub max_diff_files: u32,
    pub default_template: String,
    /// Controls whether dctx automatically updates the local .gitignore file
    /// with dctx-specific rules. Defined in ADR-005.
    pub update_local_git_repo_gitignore_rules: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OutputMode {
    Both,
    TextOnly,
    BinaryOnly,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            output_mode: OutputMode::Both,
            max_diff_files: 999,
            default_template: "default".to_string(),
            update_local_git_repo_gitignore_rules: true, // Safe default per ADR-005
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
        env::var("LOCALAPPDATA")
            .ok()
            .map(|p| PathBuf::from(p).join(APP_NAME))
    } else if cfg!(target_os = "macos") {
        env::var("HOME").ok().map(|h| {
            PathBuf::from(h)
                .join("Library")
                .join("Caches")
                .join(APP_NAME)
        })
    } else {
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
    get_config_dir().map(|p| p.join("projects.db"))
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
            "output_mode" => match val {
                "text" => config.output_mode = OutputMode::TextOnly,
                "binary" => config.output_mode = OutputMode::BinaryOnly,
                _ => config.output_mode = OutputMode::Both,
            },
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
            _ => {}
        }
    }
}
