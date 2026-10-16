//! Ignore pattern value types.
//! Implements SPEC-MAP-MODE section 6.1 and ADR-014/015b.
//! Uses standard-library types; consumed by scan clients.

/// Compiled pattern strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternType {
    /// Never matches.
    Invalid,
    /// Matches names at any depth.
    Basename,
    /// Matches an exact relative path.
    Path,
    /// Matches a name suffix.
    Suffix,
    /// Matches a path prefix.
    Prefix,
    /// Matches wildcard bytes.
    Glob,
}
/// Parsed ignore or omit rule.
#[derive(Debug, Clone)]
pub struct IgnoreRule {
    /// Pattern without negation and directory markers.
    pub pattern: String,
    /// Matching strategy.
    pub pattern_type: PatternType,
    /// Restricts matching to directories.
    pub is_dir_only: bool,
    /// Re-includes a previously matched item.
    pub is_negation: bool,
}
impl Default for IgnoreRule {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            pattern_type: PatternType::Invalid,
            is_dir_only: false,
            is_negation: false,
        }
    }
}
impl IgnoreRule {
    /// Creates an empty, invalid rule.
    pub fn new() -> Self {
        Self::default()
    }
}
