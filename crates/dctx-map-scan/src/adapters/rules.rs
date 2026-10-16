//! Layered filesystem loading for the pure ignore engine.
//! Implements ADR-015b and SPEC-MAP-MODE section 6.1.
//! Uses RuleSource for deterministic, injected rule files.

use crate::domain::ignore::defaults::{HARDCODED_IGNORE_RULES, HARDCODED_OMIT_CONTENT_RULES};
use crate::domain::ignore::{EngineStats, IgnoreEngine, IgnoreRule, parse_ignore_pattern_line};
use crate::ports::RuleSource;
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

/// Failure to read a rule file.
#[derive(Debug)]
pub enum IgnoreError {
    /// IO failure with the exact rule path.
    FileReadError {
        /// Rule file that failed to load.
        path: PathBuf,
        /// Underlying IO error.
        source: io::Error,
    },
}
impl std::fmt::Display for IgnoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileReadError { path, source } => write!(
                f,
                "Failed to read ignore file '{}': {}",
                path.display(),
                source
            ),
        }
    }
}
impl std::error::Error for IgnoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FileReadError { source, .. } => Some(source),
        }
    }
}

/// Reads optional rules without existence checks that could hide errors.
pub struct FileRuleSource;
impl RuleSource for FileRuleSource {
    fn read_rules(&self, path: &Path) -> io::Result<Option<String>> {
        match fs::read_to_string(path) {
            Ok(content) => Ok(Some(content)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

fn append(
    source: &impl RuleSource,
    path: &Path,
    rules: &mut Vec<IgnoreRule>,
) -> Result<usize, IgnoreError> {
    let content = source
        .read_rules(path)
        .map_err(|source| IgnoreError::FileReadError {
            path: path.to_path_buf(),
            source,
        })?;
    let start = rules.len();
    if let Some(content) = content {
        rules.extend(content.lines().filter_map(parse_ignore_pattern_line));
    }
    Ok(rules.len() - start)
}

fn global_config() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        env::var_os("APPDATA").map(|path| PathBuf::from(path).join("dctx"))
    } else if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        Some(PathBuf::from(path).join("dctx"))
    } else {
        env::var_os("HOME").map(|path| PathBuf::from(path).join(".config/dctx"))
    }
}

impl IgnoreEngine {
    /// Loads built-in, global and project rules, preserving every read failure.
    pub fn build(base_dir: &Path, _debug_verbose: bool) -> Result<Self, IgnoreError> {
        Self::build_with_source(base_dir, global_config().as_deref(), &FileRuleSource)
    }

    /// Loads explicit layers with injected IO; `None` disables global config.
    /// Project precedence is `.gitignore`, `.dctxignore`, then `.legacyignore`.
    pub fn build_with_source(
        base_dir: &Path,
        global_dir: Option<&Path>,
        source: &impl RuleSource,
    ) -> Result<Self, IgnoreError> {
        let mut ignore: Vec<_> = HARDCODED_IGNORE_RULES
            .iter()
            .filter_map(|line| parse_ignore_pattern_line(line))
            .collect();
        let mut omit: Vec<_> = HARDCODED_OMIT_CONTENT_RULES
            .iter()
            .filter_map(|line| parse_ignore_pattern_line(line))
            .collect();
        let mut stats = EngineStats {
            hardcoded_ignore_count: ignore.len(),
            hardcoded_omit_count: omit.len(),
            ..EngineStats::default()
        };
        if let Some(dir) = global_dir {
            stats.global_ignore_count = append(source, &dir.join("ignore"), &mut ignore)?;
            stats.global_omit_count = append(source, &dir.join("omitcontent"), &mut omit)?;
        }
        for name in [".gitignore", ".dctxignore", ".legacyignore"] {
            stats.local_ignore_count += append(source, &base_dir.join(name), &mut ignore)?;
        }
        stats.local_omit_count = append(source, &base_dir.join(".dctxomitcontent"), &mut omit)?;
        Ok(Self::from_rules(ignore, omit).with_stats(stats))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FileHandlingDecision, ItemContext};

    struct Missing;
    impl RuleSource for Missing {
        fn read_rules(&self, _: &Path) -> io::Result<Option<String>> {
            Ok(None)
        }
    }
    struct Refused;
    impl RuleSource for Refused {
        fn read_rules(&self, _: &Path) -> io::Result<Option<String>> {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
    }
    #[test]
    fn absent_rules_succeed_but_unreadable_rules_refuse() {
        let root = Path::new("project");
        assert!(IgnoreEngine::build_with_source(root, None, &Missing).is_ok());
        let error = IgnoreEngine::build_with_source(root, None, &Refused)
            .err()
            .unwrap();
        assert!(std::error::Error::source(&error).is_some());
        match error {
            IgnoreError::FileReadError { path, source } => {
                assert_eq!(path, root.join(".gitignore"));
                assert_eq!(source.kind(), io::ErrorKind::PermissionDenied);
            }
        }
    }
    #[test]
    fn invalid_utf8_is_an_error_and_missing_is_optional() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("rules");
        assert!(FileRuleSource.read_rules(&path).unwrap().is_none());
        fs::write(&path, [255]).unwrap();
        assert_eq!(
            FileRuleSource.read_rules(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::write(&path, "*.log").unwrap();
        assert_eq!(
            FileRuleSource.read_rules(&path).unwrap().as_deref(),
            Some("*.log")
        );
    }
    #[test]
    fn layer_precedence_and_local_omission() {
        let temp = tempfile::tempdir().unwrap();
        let global = temp.path().join("global");
        fs::create_dir(&global).unwrap();
        fs::write(global.join("ignore"), "*.log").unwrap();
        fs::write(global.join("omitcontent"), "*.data").unwrap();
        fs::write(temp.path().join(".gitignore"), "!keep.log").unwrap();
        fs::write(temp.path().join(".dctxignore"), "keep.log").unwrap();
        fs::write(temp.path().join(".legacyignore"), "!keep.log").unwrap();
        fs::write(temp.path().join(".dctxomitcontent"), "*.txt").unwrap();
        let engine =
            IgnoreEngine::build_with_source(temp.path(), Some(&global), &FileRuleSource).unwrap();
        let decide = |name| {
            engine.decide(&ItemContext {
                relative_path: name,
                name,
                is_directory: false,
                debug_mode: false,
            })
        };
        assert_eq!(decide("other.log"), FileHandlingDecision::Exclude);
        assert_eq!(decide("keep.log"), FileHandlingDecision::Include);
        assert_eq!(decide("notes.txt"), FileHandlingDecision::OmitContent);
        assert_eq!(decide("notes.data"), FileHandlingDecision::OmitContent);
        assert_eq!(engine.stats().local_ignore_count, 3);
        assert_eq!(engine.stats().local_omit_count, 1);
    }
}
