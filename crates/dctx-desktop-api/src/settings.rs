// SPDX-License-Identifier: Apache-2.0
//! Desktop settings: defaults, escaped line format, save, reset and `validate`.
//! Implements ARCHITECTURE-PROPOSAL §7 row DX-E4a and review F17.
//! Depends on [`crate::error::DesktopError`] and serde. The Tauri layer injects both directories.
//! Call `validate` before save. v2 quotes escape `\`, quotes and newlines so a value cannot inject a setting.
//! A file without the v2 mark keeps the old unescaped parse, so existing paths still load.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::DesktopError;

/// Directory name the desktop uses under an injected config root.
pub const DESKTOP_APP_NAME: &str = "dctx-desktop";

/// File name inside the injected config directory.
pub const DESKTOP_CONFIG_FILENAME: &str = "config.toml";

/// Comment mark written by [`save_desktop_config`]. Readers unescape only when it is present.
const SETTINGS_V2_MARK: &str = "# dctx-desktop-settings-v2";

const THEMES: [&str; 3] = ["system", "light", "dark"];
const FILE_COUNT_MIN: usize = 100;
const FILE_COUNT_MAX: usize = 1_000_000;
const WINDOW_EDGE_MIN: u32 = 1;
const WINDOW_EDGE_MAX: u32 = 100_000;
const SIZE_MB_MAX: f64 = 1_048_576.0;
const EXTENSION_MAX_LEN: usize = 32;
const EXTENSION_MAX_COUNT: usize = 64;

/// Desktop window, filter and snapshot preferences.
///
/// The core `dctx` config stays with the caller. This struct is the desktop file only,
/// which is the JSON shape `settings-manager.ts` reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools, reason = "These serialized public settings are independent user preferences and filters; replacing them with state enums would change the configuration format and API.")]
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
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn has_active_filters(&self) -> bool {
        self.filter_extensions.as_ref().is_some_and(|ext| !ext.is_empty())
            || self.include_binary
            || self.only_empty
            || self.only_non_empty
            || self.min_size_mb.is_some()
            || self.max_size_mb.is_some()
            || self.only_hidden
    }

    /// Refuses a theme, window edge, file count, size or extension the desktop will not keep.
    ///
    /// Theme is `system`, `light` or `dark`. Window edges are `1..=100_000`. File count is
    /// `100..=1_000_000`. Sizes are finite, greater than 0 and at most `1_048_576`, with min <= max.
    /// An extension is 1..=32 of `A-Za-z0-9_+-` and contains a letter or digit; at most 64.
    /// `only_empty` and `only_non_empty` together are refused. The error names the field and
    /// `enum`, `range` or `extension`, never the rejected value. This does not read the disk.
    ///
    /// # Errors
    /// Refuses unknown themes, out-of-range dimensions/counts/sizes, malformed extensions or conflicting empty/non-empty filters.
    pub fn validate(&self) -> Result<(), DesktopError> {
        if !THEMES.contains(&self.theme.as_str()) {
            return Err(invalid_setting("theme", "enum"));
        }
        if !(WINDOW_EDGE_MIN..=WINDOW_EDGE_MAX).contains(&self.window_width) {
            return Err(invalid_setting("window_width", "range"));
        }
        if !(WINDOW_EDGE_MIN..=WINDOW_EDGE_MAX).contains(&self.window_height) {
            return Err(invalid_setting("window_height", "range"));
        }
        if !(FILE_COUNT_MIN..=FILE_COUNT_MAX).contains(&self.file_count_limit) {
            return Err(invalid_setting("file_count_limit", "range"));
        }
        if let Some(min) = self.min_size_mb {
            if !size_ok(min) {
                return Err(invalid_setting("min_size_mb", "range"));
            }
        }
        if let Some(max) = self.max_size_mb {
            if !size_ok(max) {
                return Err(invalid_setting("max_size_mb", "range"));
            }
        }
        if let (Some(min), Some(max)) = (self.min_size_mb, self.max_size_mb) {
            if min > max {
                return Err(invalid_setting("size_bounds", "range"));
            }
        }
        if let Some(exts) = &self.filter_extensions {
            if exts.len() > EXTENSION_MAX_COUNT || exts.iter().any(|ext| !extension_ok(ext)) {
                return Err(invalid_setting("filter_extensions", "extension"));
            }
        }
        if self.only_empty && self.only_non_empty {
            return Err(invalid_setting("content_filter", "range"));
        }
        Ok(())
    }
}

fn invalid_setting(field: &'static str, reason: &'static str) -> DesktopError {
    DesktopError::InvalidSetting { field, reason }
}

fn size_ok(value: f64) -> bool {
    value.is_finite() && value > 0.0 && value <= SIZE_MB_MAX
}

fn extension_ok(ext: &str) -> bool {
    let mut len = 0usize;
    let mut alphanumeric = false;
    for ch in ext.chars() {
        len += 1;
        if len > EXTENSION_MAX_LEN {
            return false;
        }
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => alphanumeric = true,
            '_' | '+' | '-' => {}
            _ => return false,
        }
    }
    alphanumeric
}

/// Creates the injected config and cache directories.
///
/// # Errors
/// Returns the named config-directory or cache-directory creation failure.
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
/// A file carrying the `dctx-desktop-settings-v2` mark unescapes quotes, newlines and backslashes.
/// Older files keep the original unescaped parse.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn load_desktop_config(config_dir: &Path) -> DesktopConfig {
    let mut config = DesktopConfig::default();
    let path = config_dir.join(DESKTOP_CONFIG_FILENAME);
    let Ok(content) = fs::read_to_string(path) else {
        return config;
    };
    let escaped = content.lines().any(|line| line.trim() == SETTINGS_V2_MARK);
    for line in content.lines() {
        apply_config_line(line, &mut config, escaped);
    }
    config
}

/// Writes `config` into `config_dir/config.toml`, creating the directory when needed.
///
/// Quoted strings escape quotes, newlines and backslashes, so a value cannot become another
/// setting. This does not call [`DesktopConfig::validate`]; the native command does that.
///
/// # Errors
/// Returns config-directory creation or configuration-file write failures.
pub fn save_desktop_config(config_dir: &Path, config: &DesktopConfig) -> Result<(), DesktopError> {
    fs::create_dir_all(config_dir)
        .map_err(|error| DesktopError::CreateConfigDir { detail: error.to_string() })?;
    let path = config_dir.join(DESKTOP_CONFIG_FILENAME);
    fs::write(path, render_config(config))
        .map_err(|error| DesktopError::WriteConfig { detail: error.to_string() })
}

/// Saves [`DesktopConfig::default`] into `config_dir`.
///
/// # Errors
/// Propagates config-directory creation or configuration-file write failures from saving the defaults.
pub fn reset_desktop_config(config_dir: &Path) -> Result<(), DesktopError> {
    save_desktop_config(config_dir, &DesktopConfig::default())
}

fn apply_config_line(line: &str, config: &mut DesktopConfig, escaped: bool) {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return;
    }
    let Some((key, raw_val)) = line.split_once('=') else {
        return;
    };
    let key = key.trim();
    let raw_val = raw_val.trim();
    if key == "filter_extensions" {
        assign_extensions(raw_val, config, escaped);
        return;
    }
    let val = if escaped { decode_scalar(raw_val) } else { legacy_scalar(raw_val) };
    match key {
        "last_directory" => config.last_directory = Some(val),
        "window_width" => assign_parsed(&val, &mut config.window_width),
        "window_height" => assign_parsed(&val, &mut config.window_height),
        "file_count_limit" => assign_parsed(&val, &mut config.file_count_limit),
        "theme" => config.theme = val,
        "auto_copy_to_clipboard" => config.auto_copy_to_clipboard = val == "true",
        "include_binary" => config.include_binary = val == "true",
        "only_hidden" => config.only_hidden = val == "true",
        "min_size_mb" => assign_parsed_some(&val, &mut config.min_size_mb),
        "max_size_mb" => assign_parsed_some(&val, &mut config.max_size_mb),
        "only_empty" => config.only_empty = val == "true",
        "only_non_empty" => config.only_non_empty = val == "true",
        "tree_only" => config.tree_only = val == "true",
        _ => {}
    }
}

fn assign_extensions(raw: &str, config: &mut DesktopConfig, escaped: bool) {
    let parts = if escaped {
        split_extensions(raw)
    } else {
        legacy_scalar(raw).split(',').map(|part| part.trim().to_string()).filter(|part| !part.is_empty()).collect()
    };
    if !parts.is_empty() {
        config.filter_extensions = Some(parts);
    }
}

fn legacy_scalar(raw: &str) -> String {
    raw.trim().trim_matches('"').trim_matches('\'').to_string()
}

fn decode_scalar(raw: &str) -> String {
    unescape_setting(strip_quotes(raw))
}

fn strip_quotes(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        let quote = bytes[0];
        if (quote == b'"' || quote == b'\'') && bytes[trimmed.len() - 1] == quote {
            return &trimmed[1..trimmed.len() - 1];
        }
    }
    trimmed
}

fn split_extensions(raw: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = strip_quotes(raw).chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            push_unescaped(&mut current, chars.next());
            continue;
        }
        if ch == ',' {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

fn unescape_setting(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            push_unescaped(&mut out, chars.next());
        } else {
            out.push(ch);
        }
    }
    out
}

fn push_unescaped(out: &mut String, next: Option<char>) {
    match next {
        Some('\\') | None => out.push('\\'),
        Some('"') => out.push('"'),
        Some('n') => out.push('\n'),
        Some('r') => out.push('\r'),
        Some(',') => out.push(','),
        Some(other) => {
            out.push('\\');
            out.push(other);
        }
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
    content.push_str(SETTINGS_V2_MARK);
    content.push('\n');
    content.push_str("# This file is auto-generated\n\n");
    content.push_str("# Desktop-specific settings\n");
    if let Some(dir) = &config.last_directory {
        push_quoted(&mut content, "last_directory", dir);
    }
    push_plain(&mut content, "window_width", &config.window_width.to_string());
    push_plain(&mut content, "window_height", &config.window_height.to_string());
    push_plain(&mut content, "file_count_limit", &config.file_count_limit.to_string());
    push_quoted(&mut content, "theme", &config.theme);
    push_plain(&mut content, "auto_copy_to_clipboard", bool_word(config.auto_copy_to_clipboard));
    content.push_str("\n# Filter settings\n");
    if let Some(exts) = &config.filter_extensions {
        push_extensions(&mut content, exts);
    }
    push_plain(&mut content, "include_binary", bool_word(config.include_binary));
    push_plain(&mut content, "only_hidden", bool_word(config.only_hidden));
    if let Some(min) = config.min_size_mb {
        push_plain(&mut content, "min_size_mb", &min.to_string());
    }
    if let Some(max) = config.max_size_mb {
        push_plain(&mut content, "max_size_mb", &max.to_string());
    }
    push_plain(&mut content, "only_empty", bool_word(config.only_empty));
    push_plain(&mut content, "only_non_empty", bool_word(config.only_non_empty));
    push_plain(&mut content, "tree_only", bool_word(config.tree_only));
    content
}

fn bool_word(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn push_plain(content: &mut String, key: &str, value: &str) {
    content.push_str(key);
    content.push_str(" = ");
    content.push_str(value);
    content.push('\n');
}

fn push_quoted(content: &mut String, key: &str, value: &str) {
    content.push_str(key);
    content.push_str(" = \"");
    push_escaped(content, value);
    content.push_str("\"\n");
}

fn push_extensions(content: &mut String, exts: &[String]) {
    content.push_str("filter_extensions = \"");
    for (index, ext) in exts.iter().enumerate() {
        if index > 0 {
            content.push(',');
        }
        push_escaped(content, ext);
    }
    content.push_str("\"\n");
}

fn push_escaped(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            ',' => out.push_str("\\,"),
            _ => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests;
