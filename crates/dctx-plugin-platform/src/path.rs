// SPDX-License-Identifier: Apache-2.0
//! Literal resource names independent of IO and authorization.
//! Implements the spelling subset of S-FILE/S-CAPABILITY; Linux resolves through handles.
//! A validated name is not a consent grant or a filesystem identity.

use crate::PlatformError;

/// Canonical portable project-relative resource name, with no traversal or aliases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelativePath(String);

impl RelativePath {
    /// Validates a bounded literal name. Unicode names are refused in this initial profile.
    /// The host separately rejects case-fold collisions across its resource declarations.
    ///
    /// # Errors
    /// Returns an invalid-path error for empty, oversized, non-ASCII, reserved or ambiguous path components.
    pub fn new(value: &str) -> Result<Self, PlatformError> {
        if value.is_empty() || value.len() > 4096 || !value.is_ascii() {
            return Err(PlatformError::InvalidPath);
        }
        for part in value.split('/') {
            let stem = part
                .split('.')
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.len() == 4
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'));
            if part.is_empty()
                || part == "."
                || part == ".."
                || part.len() > 255
                || part.ends_with(['.', ' '])
                || device
                || part
                    .bytes()
                    .any(|b| b < 0x20 || b == 0x7f || b"\\:*?\"<>|".contains(&b))
            {
                return Err(PlatformError::InvalidPath);
            }
        }
        Ok(Self(value.into()))
    }

    /// The validated spelling; resolution still requires a retained root handle.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn rejects_aliases_and_accepts_literal_names() {
        for path in [
            "", "/a", "a/", "a//b", ".", "..", "a/../b", "a\\b", "C:x", "nul.txt", "COM1", "a.",
            "a ", "a\0",
        ] {
            assert!(RelativePath::new(path).is_err(), "{path:?}");
        }
        for path in [".gitignore", "src/main.rs", "a-b_9", "dir/file"] {
            assert_eq!(RelativePath::new(path).unwrap().as_str(), path);
        }
    }

    proptest! {
        #[test]
        fn traversal_never_becomes_a_resource(name in "[a-z]{1,32}") {
            let traversal = format!("{name}/../secret");
            let absolute = format!("/{name}");
            let relative = format!("dir/{name}");
            prop_assert!(RelativePath::new(&traversal).is_err());
            prop_assert!(RelativePath::new(&absolute).is_err());
            prop_assert!(RelativePath::new(&relative).is_ok());
        }
    }
}
