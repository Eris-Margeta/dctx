// SPDX-License-Identifier: Apache-2.0
//! Relative source path components.
//! Implements DX-B3 confinement for caller paths and directory names.
//! Depends on [`super::error`]. Walk and single-path lookup both use it.

use super::error::SourceError;
use std::{ffi::OsStr, path::Path};

pub(crate) fn join_rel(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

pub(crate) fn component_name<'a>(name: &'a OsStr, owner: &Path) -> Result<&'a str, SourceError> {
    let text = name.to_str().ok_or_else(|| SourceError::InvalidPath {
        path: owner.join(name),
    })?;
    if text.is_empty()
        || text == "."
        || text == ".."
        || text.ends_with(['.', ' '])
        || text.contains('/')
        || text.contains('\\')
        || text.contains(':')
        || text.chars().any(char::is_control)
    {
        return Err(SourceError::InvalidPath {
            path: owner.join(name),
        });
    }
    Ok(text)
}

pub(crate) fn split_relative(relative: &str) -> Result<Vec<&str>, SourceError> {
    if relative.len() > 4096
        || relative.is_empty()
        || relative.starts_with('/')
        || relative.ends_with('/')
    {
        return Err(SourceError::InvalidPath {
            path: Path::new(relative).to_path_buf(),
        });
    }
    let mut parts = Vec::new();
    for part in relative.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part.contains('\\')
            || part.contains(':')
            || part.chars().any(char::is_control)
        {
            return Err(SourceError::InvalidPath {
                path: Path::new(relative).to_path_buf(),
            });
        }
        parts.push(part);
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_and_absolute_relative_paths() {
        for bad in [
            "../secret",
            "C:/outside",
            "a:stream",
            "a\u{1b}b",
            ".. /outside",
        ] {
            assert!(split_relative(bad).is_err());
        }
        assert!(split_relative(&"a".repeat(4097)).is_err());
        let error = component_name(OsStr::new("escape\u{1b}.rs"), Path::new("root")).unwrap_err();
        assert!(!error.to_string().contains('\u{1b}'));
        assert!(split_relative("/etc/passwd").is_err());
        assert!(split_relative("src/main.rs").is_ok());
        assert_eq!(join_rel("src", "main.rs"), "src/main.rs");
    }
}
