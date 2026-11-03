// SPDX-License-Identifier: Apache-2.0
//! Project rule files for a desktop snapshot.
//! Implements ADR-014 and decision 078: plain-text `.gitignore`, `.dctxignore` and `.dctxomitcontent`
//! are host inputs. The facade does not discover them.
//! Depends on [`crate::error::DesktopError`] and the standard library. [`crate::snapshot`] supplies
//! the loaded texts to the selection policy before any project content read.
//! Symlinks and non-regular files are refused. The opened handle is confined to the root.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use crate::error::DesktopError;

/// Largest rule file accepted from one opened handle, in bytes.
///
/// Matches the plugin `.gitignore` cap (1 MiB). A longer file is refused.
/// `fstat` on the opened handle runs first; the following read stops at this many
/// bytes plus one and is never parsed.
pub const MAX_RULE_FILE_BYTES: u64 = 1_048_576;

/// Owned project ignore and omit texts. `None` means that layer had no rules.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ProjectRules {
    /// `.gitignore` followed by `.dctxignore`, with literal builtin negations removed.
    pub(crate) ignore: Option<String>,
    /// `.dctxomitcontent`.
    pub(crate) omit: Option<String>,
}

impl ProjectRules {
    /// Borrows the texts in facade layer order (global layers stay absent).
    pub(crate) fn texts(&self) -> dctx::RuleTexts<'_> {
        dctx::RuleTexts { local_ignore: self.ignore.as_deref(), local_omit: self.omit.as_deref(), ..dctx::RuleTexts::default() }
    }
}

/// Loads the project rule files. Missing files are absent, not an error.
pub(crate) fn load_project_rules(root: &Path) -> Result<ProjectRules, DesktopError> {
    let gitignore = read_rule(root, ".gitignore")?;
    let dctxignore = read_rule(root, ".dctxignore")?;
    let omit = read_rule(root, ".dctxomitcontent")?;
    Ok(ProjectRules { ignore: combine_ignore(gitignore.as_deref(), dctxignore.as_deref()), omit })
}

/// Drops negations that would re-include a builtin-excluded literal name.
///
/// Wildcard forms such as `!.g*/` and nested forms such as `!nested/.git/` stay.
/// Those cannot reopen `.git`: the desktop source refuses that path component.
pub(crate) fn without_builtin_negations(text: &str) -> String {
    let mut kept = String::new();
    for line in text.lines() {
        if negation_lifts_builtin(line) {
            continue;
        }
        kept.push_str(line);
        kept.push('\n');
    }
    kept
}

fn combine_ignore(gitignore: Option<&str>, dctxignore: Option<&str>) -> Option<String> {
    let mut combined = String::new();
    if let Some(text) = gitignore {
        push_text(&mut combined, text);
    }
    if let Some(text) = dctxignore {
        push_text(&mut combined, text);
    }
    if combined.is_empty() {
        return None;
    }
    let cleaned = without_builtin_negations(&combined);
    if cleaned.trim().is_empty() { None } else { Some(cleaned) }
}

fn push_text(buf: &mut String, text: &str) {
    if !buf.is_empty() && !buf.ends_with('\n') {
        buf.push('\n');
    }
    buf.push_str(text);
    if !text.ends_with('\n') {
        buf.push('\n');
    }
}

fn negation_lifts_builtin(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(rest) = trimmed.strip_prefix('!') else {
        return false;
    };
    let token = rest.split('#').next().unwrap_or(rest).trim().trim_matches('/');
    let name = token.split('/').next().unwrap_or(token);
    excluded_by_builtin(name, true) || excluded_by_builtin(name, false)
}

/// Names the survey skips. The set matches the facade builtin ignore list.
pub(crate) fn excluded_by_builtin(name: &str, is_dir: bool) -> bool {
    if is_dir {
        return matches!(
            name,
            ".git" | ".svn" | ".hg" | "node_modules" | "bower_components" | ".turbo" | ".pnpm-store" | ".localstack"
                | "dist" | "build" | "target" | "bin" | "obj" | ".venv" | "venv" | "__pycache__" | ".dctx" | ".cursor" | ".aider" | ".eris-code"
                | ".continue"
        );
    }
    matches!(name, ".DS_Store" | "Thumbs.db" | ".dctxignore")
        || has_extension(name, "pyc")
        || has_extension(name, "dctx")
        || has_extension(name, "dircontxt")
}

fn has_extension(name: &str, extension: &str) -> bool {
    Path::new(name).extension().and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

fn read_rule(root: &Path, name: &str) -> Result<Option<String>, DesktopError> {
    let root = fs::canonicalize(root).map_err(|error| io_error(name, &error))?;
    let path = root.join(name);
    if path.parent() != Some(root.as_path()) {
        return Err(rule_error(name, "escapes the project root"));
    }
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(name, &error)),
    };
    if meta.file_type().is_symlink() {
        return Err(rule_error(name, "is a symlink"));
    }
    if !meta.is_file() {
        return Err(rule_error(name, "is not a regular file"));
    }
    let file = open_nofollow(&path).map_err(|error| open_error(name, &error))?;
    let opened = file.metadata().map_err(|error| io_error(name, &error))?;
    if !opened.is_file() {
        return Err(rule_error(name, "is not a regular file"));
    }
    let confirmed = fs::symlink_metadata(&path).map_err(|error| io_error(name, &error))?;
    if confirmed.file_type().is_symlink() || !same_file(&opened, &confirmed) {
        return Err(rule_error(name, "escapes the project root"));
    }
    let real = fs::canonicalize(&path).map_err(|error| io_error(name, &error))?;
    if real.parent() != Some(root.as_path()) {
        return Err(rule_error(name, "escapes the project root"));
    }
    read_bounded(&file, name).map(Some)
}

fn read_bounded(file: &File, name: &str) -> Result<String, DesktopError> {
    let len = file.metadata().map_err(|error| io_error(name, &error))?.len();
    if len > MAX_RULE_FILE_BYTES {
        return Err(too_big(name));
    }
    let mut buf = Vec::new();
    file.take(MAX_RULE_FILE_BYTES + 1).read_to_end(&mut buf).map_err(|error| io_error(name, &error))?;
    if buf.len() as u64 > MAX_RULE_FILE_BYTES {
        return Err(too_big(name));
    }
    String::from_utf8(buf).map_err(|_| rule_error(name, "is not valid utf-8"))
}

fn open_nofollow(path: &Path) -> std::io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new().read(true).custom_flags(nofollow_flag()).open(path)
    }
    #[cfg(not(unix))]
    {
        File::open(path)
    }
}

/// `O_NOFOLLOW`. Linux and Android use `0x20000`; macOS and iOS use `0x100`.
#[cfg(unix)]
fn nofollow_flag() -> i32 {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        0x20000
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        0x100
    }
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios")))]
    {
        0
    }
}

fn same_file(opened: &fs::Metadata, path_meta: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        opened.dev() == path_meta.dev() && opened.ino() == path_meta.ino()
    }
    #[cfg(not(unix))]
    {
        opened.len() == path_meta.len() && opened.is_file() && path_meta.is_file()
    }
}

fn too_big(name: &str) -> DesktopError {
    rule_error(name, &format!("exceeds {MAX_RULE_FILE_BYTES} bytes"))
}

fn rule_error(name: &str, reason: &str) -> DesktopError {
    DesktopError::IgnoreRules { detail: format!("{name} {reason}") }
}

fn io_error(name: &str, error: &std::io::Error) -> DesktopError {
    rule_error(name, &error.to_string())
}

fn open_error(name: &str, error: &std::io::Error) -> DesktopError {
    // Linux ELOOP is 40. macOS and the BSDs use 62.
    if matches!(error.raw_os_error(), Some(40 | 62)) {
        rule_error(name, "is a symlink")
    } else {
        io_error(name, error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_builtin_negation_is_removed_and_wildcard_or_nested_remains() {
        let cleaned = without_builtin_negations("!.git/\n!.g*/\n!nested/.git/\n!keep.txt\n");
        assert!(!cleaned.contains("!.git/"));
        assert!(cleaned.contains("!.g*/"));
        assert!(cleaned.contains("!nested/.git/"));
        assert!(cleaned.contains("!keep.txt"));
    }
}
