// SPDX-License-Identifier: Apache-2.0
//! Validated names: plugin IDs, safe file names, key IDs, versions and host targets.
//! Implements common.schema.json `id`/`file`/`token`/`semver`, the manifest target enum and the
//! PLUGINS.md §5.4 rule "encode IDs as validated path components". Pure; used by every domain module.
//! Names that are reserved devices on any supported OS are refused so one registry works everywhere.

use crate::error::{DistributionError, Document};

const MAX_NAME: usize = 128;
const RESERVED: [&str; 4] = ["con", "prn", "aux", "nul"];

fn component_ok(text: &str, first: fn(u8) -> bool, rest: fn(u8) -> bool) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_NAME
        && first(bytes[0])
        && bytes[1..].iter().all(|&b| rest(b))
        && !text.ends_with('.')
        && !reserved_device(text)
}

fn reserved_device(text: &str) -> bool {
    let stem = text.split('.').next().unwrap_or(text).to_ascii_lowercase();
    if RESERVED.contains(&stem.as_str()) {
        return true;
    }
    let digit =
        stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit() && stem.as_bytes()[3] != b'0';
    digit && (stem.starts_with("com") || stem.starts_with("lpt"))
}

fn lower_alnum(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit()
}

fn lower_rest(b: u8) -> bool {
    lower_alnum(b) || matches!(b, b'.' | b'_' | b'-')
}

fn any_alnum(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

fn file_rest(b: u8) -> bool {
    any_alnum(b) || matches!(b, b'.' | b'_' | b'-')
}

fn token_rest(b: u8) -> bool {
    file_rest(b) || b == b':'
}

/// Canonical `publisher/name` plugin identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginId(String);

impl PluginId {
    /// Validates `publisher/name` with path-safe halves.
    /// # Errors
    /// Refuses text outside the plugin ID pattern or length bounds.
    pub fn parse(text: &str) -> Result<Self, DistributionError> {
        let valid = text.len() <= MAX_NAME
            && text.split_once('/').is_some_and(|(publisher, name)| {
                component_ok(publisher, lower_alnum, lower_rest)
                    && component_ok(name, lower_alnum, lower_rest)
            });
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(DistributionError::malformed(Document::Locator, "plugin-id"))
        }
    }

    /// The ID as written.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Publisher and name as two validated directory components.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn components(&self) -> (&str, &str) {
        self.0.split_once('/').unwrap_or((&self.0, ""))
    }
}

/// A single safe file name: no separators, no traversal, no device names.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SafeFileName(String);

impl SafeFileName {
    /// Validates one file name component.
    /// # Errors
    /// Refuses separators, traversal/device names, or text outside the safe filename pattern/length bounds.
    pub fn parse(text: &str) -> Result<Self, DistributionError> {
        if component_ok(text, any_alnum, file_rest) {
            Ok(Self(text.to_owned()))
        } else {
            Err(DistributionError::malformed(Document::Locator, "file-name"))
        }
    }

    /// The name as written.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Case-folded form used to refuse case aliases on case-insensitive file systems.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn folded(&self) -> String {
        self.0.to_ascii_lowercase()
    }
}

/// Local trust-record ID named by a manifest's `publisher_key`; never a path or URL.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(String);

impl KeyId {
    /// Validates the token grammar.
    /// # Errors
    /// Refuses text outside the key-token grammar or length bounds.
    pub fn parse(text: &str) -> Result<Self, DistributionError> {
        let bytes = text.as_bytes();
        let valid = !bytes.is_empty()
            && bytes.len() <= MAX_NAME
            && any_alnum(bytes[0])
            && bytes[1..].iter().all(|&b| token_rest(b));
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(DistributionError::malformed(Document::Manifest, "key-id"))
        }
    }

    /// The ID as written.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exact semantic version string; ordering uses the signed release sequence instead.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(String);

impl Version {
    /// Validates `MAJOR.MINOR.PATCH[-pre][+build]` without leading zeros.
    /// # Errors
    /// Refuses text outside the supported semantic-version syntax or length bounds.
    pub fn parse(text: &str) -> Result<Self, DistributionError> {
        let refuse = || DistributionError::malformed(Document::Manifest, "version");
        if text.is_empty() || text.len() > MAX_NAME {
            return Err(refuse());
        }
        let (core, build) = split_opt(text, '+');
        let (core, pre) = split_opt(core, '-');
        let numbers: Vec<&str> = core.split('.').collect();
        let numeric = |part: &&str| {
            !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        };
        let ident = |part: &str| {
            part.split('.')
                .all(|p| !p.is_empty() && p.bytes().all(|b| any_alnum(b) || b == b'-'))
        };
        let valid = numbers.len() == 3
            && numbers.iter().all(numeric)
            && pre.is_none_or(ident)
            && pre.is_none_or(|pre| {
                pre.split('.').all(|part| {
                    !part.bytes().all(|b| b.is_ascii_digit())
                        || part.len() == 1
                        || !part.starts_with('0')
                })
            })
            && build.is_none_or(ident);
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(refuse())
        }
    }

    /// The version as written; also a safe directory component.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn split_opt(text: &str, separator: char) -> (&str, Option<&str>) {
    match text.split_once(separator) {
        Some((head, tail)) => (head, Some(tail)),
        None => (text, None),
    }
}

/// Host targets the manifest schema admits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    /// macOS arm64.
    AppleArm64,
    /// macOS `x86_64`.
    AppleX64,
    /// Linux `x86_64` glibc.
    LinuxX64,
    /// Linux aarch64 glibc.
    LinuxArm64,
    /// Windows `x86_64` MSVC.
    WindowsX64,
}

impl Target {
    const ALL: [(Self, &'static str); 5] = [
        (Self::AppleArm64, "aarch64-apple-darwin"),
        (Self::AppleX64, "x86_64-apple-darwin"),
        (Self::LinuxX64, "x86_64-unknown-linux-gnu"),
        (Self::LinuxArm64, "aarch64-unknown-linux-gnu"),
        (Self::WindowsX64, "x86_64-pc-windows-msvc"),
    ];

    /// Parses a Rust target triple from the closed set.
    /// # Errors
    /// Returns an unsupported-target document error for a triple outside the closed target set.
    pub fn parse(text: &str) -> Result<Self, DistributionError> {
        Self::ALL
            .iter()
            .find(|(_, name)| *name == text)
            .map(|(target, _)| *target)
            .ok_or(DistributionError::malformed(Document::Manifest, "target"))
    }

    /// The target triple.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn as_str(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(target, _)| *target == self)
            .map_or("", |(_, name)| name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn plugin_ids_are_two_safe_components() {
        assert_eq!(
            PluginId::parse("dctx/git").unwrap().components(),
            ("dctx", "git")
        );
        for bad in [
            "dctx",
            "dctx/",
            "/git",
            "../git",
            "dctx/..",
            "dctx/git/x",
            "Dctx/git",
            "dctx/con",
            "a/b.",
        ] {
            assert!(PluginId::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn file_names_refuse_traversal_and_devices() {
        assert!(SafeFileName::parse("abc.bin").is_ok());
        assert_eq!(SafeFileName::parse("LICENSE").unwrap().folded(), "license");
        for bad in [
            "", ".", "..", "../x", "a/b", "a\\b", "/abs", ".hidden", "NUL.txt", "com1", "x.", "a b",
        ] {
            assert!(SafeFileName::parse(bad).is_err(), "{bad}");
        }
        assert!(SafeFileName::parse("com0").is_ok());
    }

    #[test]
    fn versions_follow_semver_without_leading_zeros() {
        for good in ["1.0.0", "0.1.2-rc.1", "1.2.3+build.5", "10.20.30-alpha-1+x"] {
            assert!(Version::parse(good).is_ok(), "{good}");
        }
        for bad in [
            "1.0",
            "01.0.0",
            "1.0.0-",
            "1.0.0+",
            "1.0.0-a..b",
            "v1.0.0",
            "1.0.0/x",
        ] {
            assert!(Version::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn targets_round_trip_and_key_ids_refuse_paths() {
        for (target, name) in Target::ALL {
            assert_eq!(Target::parse(name).unwrap(), target);
            assert_eq!(target.as_str(), name);
        }
        assert!(Target::parse("wasm32-unknown-unknown").is_err());
        assert!(KeyId::parse("fixture-key-1").is_ok());
        assert!(KeyId::parse("../keys/a").is_err());
    }

    proptest! {
        #[test]
        fn accepted_file_names_never_contain_separators(text in "[ -~]{0,20}") {
            if let Ok(name) = SafeFileName::parse(&text) {
                prop_assert!(!name.as_str().contains(['/', '\\', ':']));
                prop_assert!(name.as_str() != "." && name.as_str() != "..");
            }
        }
    }
}
