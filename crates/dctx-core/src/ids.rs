//! Identifier newtypes: confined relative paths, view keys, timestamps and snapshot digests.
//! Implements the identity rules of contracts/IDENTITY-v1.md and decision 078: content and snapshot
//! identity come only from the first-party digest in `dctx-hash`, never from a VCS object id.
//! Depends on `dctx-hash` and [`crate::error`]. Used by `crate::facts`, dctx-ports and dctx-pack.

#![deny(missing_docs)]

use crate::error::{DctxError, Resource};
use std::fmt;

pub use dctx_hash::ContentDigest;

/// Longest accepted relative path or view key, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;

/// A root-relative path with `/` separators. The empty path is the root.
///
/// Invariants: no leading or trailing `/`, no empty, `.` or `..` segments,
/// no `\` and no NUL, at most [`MAX_PATH_BYTES`]. A value of this type can
/// never name anything outside the source root.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RelativePath(String);

impl RelativePath {
    /// The source root.
    pub fn root() -> Self {
        Self(String::new())
    }

    /// Validates `text` as a confined relative path.
    pub fn parse(text: &str) -> Result<Self, DctxError> {
        if text.len() > MAX_PATH_BYTES {
            return Err(DctxError::ResourceLimit {
                resource: Resource::PathBytes,
                limit: MAX_PATH_BYTES as u64,
            });
        }
        if text.is_empty() {
            return Ok(Self::root());
        }
        for segment in text.split('/') {
            check_segment(segment)?;
        }
        Ok(Self(text.to_string()))
    }

    /// The child `name` of this directory path. `name` must be a single segment.
    pub fn child(&self, name: &str) -> Result<Self, DctxError> {
        check_segment(name)?;
        let joined = if self.0.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", self.0, name)
        };
        if joined.len() > MAX_PATH_BYTES {
            return Err(DctxError::ResourceLimit {
                resource: Resource::PathBytes,
                limit: MAX_PATH_BYTES as u64,
            });
        }
        Ok(Self(joined))
    }

    /// The path text (empty for the root).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// True for the source root.
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// The last segment (empty for the root).
    pub fn name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or("")
    }

    /// Number of segments (0 for the root).
    pub fn depth(&self) -> usize {
        if self.0.is_empty() {
            0
        } else {
            self.0.split('/').count()
        }
    }
}

impl fmt::Display for RelativePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn check_segment(segment: &str) -> Result<(), DctxError> {
    let reason = if segment.is_empty() {
        "empty path segment"
    } else if segment == "." || segment == ".." {
        "dot path segment"
    } else if segment.contains('/') {
        "separator inside a name"
    } else if segment.contains('\\') || segment.contains('\0') {
        "backslash or NUL in path"
    } else {
        return Ok(());
    };
    Err(DctxError::InvalidRequest { reason })
}

/// Names one pack view (a project or root under one policy) in a fact store.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewKey(String);

impl ViewKey {
    /// Validates a non-empty key of printable characters, at most [`MAX_PATH_BYTES`].
    pub fn parse(text: &str) -> Result<Self, DctxError> {
        if text.is_empty() || text.chars().any(char::is_control) {
            return Err(DctxError::InvalidRequest { reason: "view key must be printable and non-empty" });
        }
        if text.len() > MAX_PATH_BYTES {
            return Err(DctxError::ResourceLimit {
                resource: Resource::PathBytes,
                limit: MAX_PATH_BYTES as u64,
            });
        }
        Ok(Self(text.to_string()))
    }

    /// The key text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Milliseconds since the Unix epoch, as supplied by a clock or source port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Timestamp(pub u64);

impl Timestamp {
    /// Milliseconds since the Unix epoch.
    pub fn millis(self) -> u64 {
        self.0
    }
}

/// Identity of one pack snapshot: the own digest of its canonical facts encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SnapshotDigest(pub ContentDigest);

impl fmt::Display for SnapshotDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_paths_parse_and_report_name_and_depth() {
        let path = RelativePath::parse("src/lib/mod.rs").unwrap();
        assert_eq!(path.name(), "mod.rs");
        assert_eq!(path.depth(), 3);
        assert!(RelativePath::parse("").unwrap().is_root());
        assert_eq!(RelativePath::root().depth(), 0);
        assert_eq!(RelativePath::parse(".hidden/x").unwrap().name(), "x");
    }

    #[test]
    fn escaping_and_malformed_paths_are_refused() {
        for bad in ["/etc", "a//b", "a/", "../x", "a/./b", "a\\b", "a\0b"] {
            assert_eq!(RelativePath::parse(bad).unwrap_err().code(), crate::error::ErrorCode::InvalidRequest, "{bad}");
        }
        let long = "a".repeat(MAX_PATH_BYTES + 1);
        assert!(matches!(RelativePath::parse(&long), Err(DctxError::ResourceLimit { .. })));
    }

    #[test]
    fn child_joins_one_segment_only() {
        let root = RelativePath::root();
        let src = root.child("src").unwrap();
        assert_eq!(src.as_str(), "src");
        assert_eq!(src.child("main.rs").unwrap().as_str(), "src/main.rs");
        assert!(src.child("a/b").is_err());
        assert!(src.child("..").is_err());
        assert!(src.child("").is_err());
    }

    #[test]
    fn view_keys_are_printable_and_non_empty() {
        assert_eq!(ViewKey::parse("project-1").unwrap().as_str(), "project-1");
        assert!(ViewKey::parse("").is_err());
        assert!(ViewKey::parse("a\nb").is_err());
    }
}
