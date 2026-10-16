//! Application settings as data: file-name constants, `AppConfig` and its line parser.
//! Implements the `config.toml` key set of the current CLI (ADR-005, Phase 5.2/10.8.2 keys).
//! Depends on nothing; the caller supplies the text, this module never discovers or reads files.
//! Used by the legacy `crate::config` (which locates and reads the file) and by frontends.

/// Magic Bytes identifying the format (8 bytes).
pub const DIRCONTXT_MAGIC_BYTES: &[u8; 8] = b"DIRCTXTV";

/// Binary file extension (always .dctx).
pub const BINARY_FILE_EXTENSION: &str = "dctx";

/// Text context extension.
pub const LLM_CONTEXT_EXTENSION: &str = "llmcontext.txt";

/// Local ignore filename.
pub const DEFAULT_IGNORE_FILENAME: &str = ".dctxignore";

/// Local omit content filename.
pub const DEFAULT_OMIT_CONTENT_FILENAME: &str = ".dctxomitcontent";

/// Application name (folder for config/cache).
pub const APP_NAME: &str = "dctx";

/// Global ignore filename inside config dir.
pub const GLOBAL_IGNORE_FILENAME: &str = "ignore";

/// Global omit content filename inside config dir.
pub const GLOBAL_OMIT_CONTENT_FILENAME: &str = "omitcontent";

/// Configuration filename inside config dir.
pub const CONFIG_FILENAME: &str = "config.toml";

/// Application settings read from `config.toml`.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Controls whether text output files (.llmcontext.txt) are written to the
    /// local .dctx/ directory. Binary state is ALWAYS written to system cache.
    /// Default is TRUE for backward compatibility.
    pub text_output_enabled: bool,
    /// Minor versions before a major version bump.
    pub max_diff_files: u32,
    /// Template used when none is requested.
    pub default_template: String,
    /// Controls whether dctx automatically updates the local .gitignore file
    /// with dctx-specific rules. Defined in ADR-005.
    pub update_local_git_repo_gitignore_rules: bool,
    /// Controls whether diff files (*-diff.txt) are generated on disk.
    /// Added in Phase 5.2. Default is FALSE to save disk space.
    pub text_output_mode_generates_diff_files: bool,
    /// Controls whether SVG file content is omitted from output.
    /// SVGs are technically text but often large/unreadable.
    /// Default is TRUE (omit content, show only in tree).
    pub omit_svg_content: bool,
    /// Controls whether detailed debug output is shown (ignore checks, pattern matches).
    /// When FALSE (default), only errors, warnings, and info are shown.
    /// When TRUE, comprehensive diagnostic output is displayed.
    /// Added in Phase 10.8.2 for debug output control.
    pub debug_verbose: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            text_output_enabled: true,
            max_diff_files: 999,
            default_template: "default".to_string(),
            update_local_git_repo_gitignore_rules: true,
            text_output_mode_generates_diff_files: false,
            omit_svg_content: true,
            debug_verbose: false,
        }
    }
}

/// Parses a whole `config.toml` text over the defaults.
pub fn parse_config(text: &str) -> AppConfig {
    let mut config = AppConfig::default();
    for line in text.lines() {
        parse_config_line(line, &mut config);
    }
    config
}

/// Applies one `key = value` line to `config`. Comments, blanks and unknown keys are ignored.
pub fn parse_config_line(line: &str, config: &mut AppConfig) {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return;
    }

    if let Some((key, val)) = line.split_once('=') {
        let key = key.trim();
        let val = val.trim().trim_matches('"').trim_matches('\'');

        match key {
            "output_mode" => config.text_output_enabled = val != "binary",
            "text_output_enabled" => config.text_output_enabled = val == "true",
            "max_diff_files" => {
                if let Ok(num) = val.parse::<u32>() {
                    config.max_diff_files = num;
                }
            }
            "default_template" => config.default_template = val.to_string(),
            "update_local_git_repo_gitignore_rules" => {
                set_flag(val, &mut config.update_local_git_repo_gitignore_rules)
            }
            "text_output_mode_generates_diff_files" => {
                set_flag(val, &mut config.text_output_mode_generates_diff_files)
            }
            "omit_svg_content" => set_flag(val, &mut config.omit_svg_content),
            _ => {}
        }
    }
}

fn set_flag(val: &str, flag: &mut bool) {
    match val {
        "true" => *flag = true,
        "false" => *flag = false,
        _ => {}
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
        assert!(config.omit_svg_content);
    }

    #[test]
    fn test_parse_text_output_enabled() {
        let mut config = AppConfig { text_output_enabled: false, ..Default::default() };
        parse_config_line("text_output_enabled = true", &mut config);
        assert!(config.text_output_enabled);
        parse_config_line("text_output_enabled = false", &mut config);
        assert!(!config.text_output_enabled);
    }

    #[test]
    fn test_parse_legacy_output_mode() {
        let mut config = AppConfig::default();
        parse_config_line("output_mode = \"binary\"", &mut config);
        assert!(!config.text_output_enabled);
        parse_config_line("output_mode = \"both\"", &mut config);
        assert!(config.text_output_enabled);
        config.text_output_enabled = false;
        parse_config_line("output_mode = \"text\"", &mut config);
        assert!(config.text_output_enabled);
    }

    #[test]
    fn test_parse_max_diff_files() {
        let mut config = AppConfig::default();
        parse_config_line("max_diff_files = not_a_number", &mut config);
        assert_eq!(config.max_diff_files, 999);
        parse_config_line("max_diff_files = 500", &mut config);
        assert_eq!(config.max_diff_files, 500);
    }

    #[test]
    fn test_parse_default_template_quotes() {
        let mut config = AppConfig::default();
        parse_config_line("default_template = \"custom\"", &mut config);
        assert_eq!(config.default_template, "custom");
        parse_config_line("default_template = 'my-template'", &mut config);
        assert_eq!(config.default_template, "my-template");
    }

    #[test]
    fn test_parse_flags() {
        let mut config = AppConfig::default();
        parse_config_line("update_local_git_repo_gitignore_rules = false", &mut config);
        assert!(!config.update_local_git_repo_gitignore_rules);
        parse_config_line("update_local_git_repo_gitignore_rules = maybe", &mut config);
        assert!(!config.update_local_git_repo_gitignore_rules);
        parse_config_line("update_local_git_repo_gitignore_rules = true", &mut config);
        assert!(config.update_local_git_repo_gitignore_rules);
        parse_config_line("text_output_mode_generates_diff_files = true", &mut config);
        assert!(config.text_output_mode_generates_diff_files);
        parse_config_line("omit_svg_content = false", &mut config);
        assert!(!config.omit_svg_content);
        parse_config_line("omit_svg_content = true", &mut config);
        assert!(config.omit_svg_content);
    }

    #[test]
    fn test_parse_ignores_comments_blanks_and_unknown_keys() {
        let config = parse_config("# comment\n  # indented\n\n   \nunknown_key = x\n");
        assert!(config.text_output_enabled);
        assert_eq!(config.max_diff_files, 999);
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
