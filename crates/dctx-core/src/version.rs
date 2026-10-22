//! Pack versions (`V{major}.{minor}`): parsing, display and the next-version rule.
//! Implements the pack versioning of the current CLI (minor per change, major on massive change or limit).
//! Depends on [`crate::diff`] for the change statistics; pure.
//! Used by the legacy reader/writer/state, dctx-cli and the dctx-pack engine.

use std::fmt;

mod advance;
mod after_diff;

pub use after_diff::version_after_diff;

/// Predstavlja verzioniranje projekta (V{Major}.{Minor}).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// Major snapshot revision, displayed after the `V` prefix.
    pub major: u32,
    /// Minor snapshot revision; zero is omitted from the displayed form.
    pub minor: u32,
}

/// The next major or minor does not fit in a `u32`.
///
/// Decoding a stored revision still succeeds. Publishers refuse this before
/// writing the next snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionExhausted {
    /// Major of the revision that could not advance.
    pub major: u32,
    /// Minor of the revision that could not advance.
    pub minor: u32,
}

impl fmt::Display for VersionExhausted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "version {}.{} cannot advance", self.major, self.minor)
    }
}

impl std::error::Error for VersionExhausted {}

impl Version {
    /// Defaultni početak: V1
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new_v1() -> Self {
        Self { major: 1, minor: 0 }
    }

    /// Parsira string poput "V1", "V1.5", "V2.0".
    /// Ako je format neispravan, vraća V1.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn parse(version_str: &str) -> Self {
        if !version_str.starts_with('V') {
            return Self::new_v1();
        }

        let clean_str = &version_str[1..]; // Makni 'V'

        // Pokušaj splitati na točki
        let parts: Vec<&str> = clean_str.split('.').collect();

        if parts.is_empty() {
            return Self::new_v1();
        }

        let major = parts[0].parse::<u32>().unwrap_or(1);
        let minor = if parts.len() > 1 {
            parts[1].parse::<u32>().unwrap_or(0)
        } else {
            0
        };

        Self { major, minor }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.minor == 0 {
            write!(f, "V{}", self.major)
        } else {
            write!(f, "V{}.{}", self.major, self.minor)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- PARSING TESTS ---

    #[test]
    fn test_parse_v1() {
        assert_eq!(Version::parse("V1"), Version { major: 1, minor: 0 });
    }

    #[test]
    fn test_parse_v1_with_minor() {
        assert_eq!(Version::parse("V1.5"), Version { major: 1, minor: 5 });
    }

    #[test]
    fn test_parse_v2() {
        assert_eq!(Version::parse("V2"), Version { major: 2, minor: 0 });
    }

    #[test]
    fn test_parse_v2_with_minor() {
        assert_eq!(
            Version::parse("V2.10"),
            Version {
                major: 2,
                minor: 10
            }
        );
    }

    #[test]
    fn test_parse_garbage_returns_v1() {
        assert_eq!(Version::parse("Garbage"), Version { major: 1, minor: 0 });
    }

    #[test]
    fn test_parse_empty_returns_v1() {
        assert_eq!(Version::parse(""), Version { major: 1, minor: 0 });
    }

    #[test]
    fn test_parse_no_v_prefix() {
        assert_eq!(Version::parse("1.5"), Version { major: 1, minor: 0 });
    }

    #[test]
    fn test_parse_invalid_numbers() {
        assert_eq!(Version::parse("Vx.y"), Version { major: 1, minor: 0 });
    }

    #[test]
    fn test_parse_large_numbers() {
        assert_eq!(
            Version::parse("V100.999"),
            Version {
                major: 100,
                minor: 999
            }
        );
    }

    #[test]
    fn test_parse_zero_minor() {
        assert_eq!(Version::parse("V3.0"), Version { major: 3, minor: 0 });
    }

    // --- NEW_V1 TESTS ---

    #[test]
    fn test_new_v1() {
        let v = Version::new_v1();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 0);
    }

    // --- DISPLAY TESTS ---

    #[test]
    fn test_display_v1() {
        let v = Version::new_v1();
        assert_eq!(format!("{v}"), "V1");
    }

    #[test]
    fn test_display_v1_with_minor() {
        let v = Version { major: 1, minor: 5 };
        assert_eq!(format!("{v}"), "V1.5");
    }

    #[test]
    fn test_display_v2() {
        let v = Version { major: 2, minor: 0 };
        assert_eq!(format!("{v}"), "V2");
    }

    #[test]
    fn test_display_v2_with_minor() {
        let v = Version {
            major: 2,
            minor: 10,
        };
        assert_eq!(format!("{v}"), "V2.10");
    }

    #[test]
    fn test_display_large_version() {
        let v = Version {
            major: 100,
            minor: 999,
        };
        assert_eq!(format!("{v}"), "V100.999");
    }

    // --- EQUALITY TESTS ---

    #[test]
    fn test_equality() {
        let v1 = Version { major: 1, minor: 5 };
        let v2 = Version { major: 1, minor: 5 };
        assert_eq!(v1, v2);
    }

    #[test]
    fn test_inequality_major() {
        let v1 = Version { major: 1, minor: 5 };
        let v2 = Version { major: 2, minor: 5 };
        assert_ne!(v1, v2);
    }

    #[test]
    fn test_inequality_minor() {
        let v1 = Version { major: 1, minor: 5 };
        let v2 = Version { major: 1, minor: 6 };
        assert_ne!(v1, v2);
    }

    // --- CLONE TESTS ---

    #[test]
    fn test_clone() {
        let v1 = Version { major: 3, minor: 7 };
        let v2 = v1.clone();
        assert_eq!(v1, v2);
    }
}
