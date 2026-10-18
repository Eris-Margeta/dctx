//! Pack versions (`V{major}.{minor}`): parsing, display and the next-version rule.
//! Implements the pack versioning of the current CLI (minor per change, major on massive change or limit).
//! Depends on [`crate::diff`] for the change statistics; pure.
//! Used by the legacy reader/writer/state, dctx-cli and the dctx-pack engine.

use crate::diff::DiffReport;
use std::fmt;

/// Predstavlja verzioniranje projekta (V{Major}.{Minor}).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u32,
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
    pub fn new_v1() -> Self {
        Self { major: 1, minor: 0 }
    }

    /// Parsira string poput "V1", "V1.5", "V2.0".
    /// Ako je format neispravan, vraća V1.
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

    /// Odlučuje koja je sljedeća verzija na temelju stanja promjena.
    ///
    /// Params:
    /// - `files_unchanged_count`: Broj datoteka koje su ostale identične (po timestampu/sadržaju).
    /// - `max_diff_files`: Konfiguracijski limit (default 999).
    ///
    /// Returns:
    /// - `(NextVersion, ShouldSoftReset)`
    /// - `ShouldSoftReset` je true ako se desio Major Bump (pa treba brisati diffove).
    ///
    /// Exhaustion cannot be a `Result` here: existing callers destructure the tuple.
    /// Major 0 is the legacy unknown-version sentinel, so a later encode refuses it.
    /// Callers that publish use [`Self::try_calculate_next`] and stop before publication.
    pub fn calculate_next(
        &self,
        files_unchanged_count: usize,
        total_files_in_new_state: usize,
        max_diff_files: u32,
    ) -> (Version, bool) {
        match self.try_calculate_next(files_unchanged_count, total_files_in_new_state, max_diff_files) {
            Ok(next) => next,
            Err(_) => (Version { major: 0, minor: 0 }, true),
        }
    }

    /// [`Self::calculate_next`] with checked arithmetic.
    ///
    /// A massive change or a minor at `max_diff_files` bumps the major.
    /// Either addition that does not fit is [`VersionExhausted`].
    pub fn try_calculate_next(
        &self,
        files_unchanged_count: usize,
        total_files_in_new_state: usize,
        max_diff_files: u32,
    ) -> Result<(Version, bool), VersionExhausted> {
        // TRIGGER A: Massive Change Event
        // Ako niti jedan fajl nije ostao nepromijenjen, a imamo fajlove u novom stanju.
        // (Ako je total_files 0, to je prazan folder, ne radimo major bump nužno, ili da?
        //  Recimo da ako je sve obrisano, to je veliki change, ali ako je unchanged=0 i total=0...
        //  Držimo se pravila: Ako unchanged == 0 i imali smo neki history -> Major Bump).
        // Za sada jednostavna logika:
        let is_massive_change = files_unchanged_count == 0 && total_files_in_new_state > 0;

        // TRIGGER B: Diff Limit Exceeded
        let is_limit_exceeded = self.minor >= max_diff_files;
        let exhausted = VersionExhausted { major: self.major, minor: self.minor };

        if is_massive_change || is_limit_exceeded {
            let major = self.major.checked_add(1).ok_or(exhausted)?;
            return Ok((Version { major, minor: 0 }, true));
        }

        let minor = self.minor.checked_add(1).ok_or(exhausted)?;
        Ok((Version { major: self.major, minor }, false))
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

/// The version after a pack run, and whether it is a major reset (old diffs dropped).
///
/// No previous snapshot starts at V1 with a reset; an unchanged tree keeps the
/// previous version; otherwise [`Version::calculate_next`] decides.
pub fn version_after_diff(
    previous: Option<&Version>,
    report: &DiffReport,
    max_diff_files: u32,
) -> (Version, bool) {
    match previous {
        None => (Version::new_v1(), true),
        Some(old) if report.has_changes => old.calculate_next(
            report.stats.files_unchanged,
            report.stats.total_files_in_new_state,
            max_diff_files,
        ),
        Some(old) => (old.clone(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_after_diff_mirrors_the_cli_rules() {
        let mut report = DiffReport::default();
        assert_eq!(version_after_diff(None, &report, 999), (Version::new_v1(), true));
        let v13 = Version { major: 1, minor: 3 };
        assert_eq!(version_after_diff(Some(&v13), &report, 999), (v13.clone(), false));
        report.has_changes = true;
        report.stats.files_unchanged = 1;
        report.stats.total_files_in_new_state = 2;
        assert_eq!(version_after_diff(Some(&v13), &report, 999), (Version { major: 1, minor: 4 }, false));
        report.stats.files_unchanged = 0;
        assert_eq!(version_after_diff(Some(&v13), &report, 999), (Version { major: 2, minor: 0 }, true));
    }

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

    // --- CALCULATE_NEXT TESTS ---

    #[test]
    fn test_standard_increment() {
        let current = Version::parse("V1.5");
        let (next, reset) = current.calculate_next(10, 20, 999);
        assert_eq!(next, Version { major: 1, minor: 6 });
        assert!(!reset);
    }

    #[test]
    fn test_increment_from_v1() {
        let current = Version::new_v1();
        let (next, reset) = current.calculate_next(5, 10, 999);
        assert_eq!(next, Version { major: 1, minor: 1 });
        assert!(!reset);
    }

    #[test]
    fn test_massive_change_trigger() {
        let current = Version::parse("V1.5");
        let (next, reset) = current.calculate_next(0, 20, 999);
        assert_eq!(next, Version { major: 2, minor: 0 });
        assert!(reset);
    }

    #[test]
    fn test_massive_change_from_v1() {
        let current = Version::new_v1();
        let (next, reset) = current.calculate_next(0, 10, 999);
        assert_eq!(next, Version { major: 2, minor: 0 });
        assert!(reset);
    }

    #[test]
    fn test_limit_trigger_exact() {
        let current = Version::parse("V1.999");
        let (next, reset) = current.calculate_next(5, 20, 999);
        assert_eq!(next, Version { major: 2, minor: 0 });
        assert!(reset);
    }

    #[test]
    fn test_limit_trigger_exceeded() {
        let current = Version::parse("V1.1000");
        let (next, reset) = current.calculate_next(5, 20, 999);
        assert_eq!(next, Version { major: 2, minor: 0 });
        assert!(reset);
    }

    #[test]
    fn test_limit_trigger_custom_limit() {
        let current = Version::parse("V1.10");
        let (next, reset) = current.calculate_next(5, 20, 10);
        assert_eq!(next, Version { major: 2, minor: 0 });
        assert!(reset);
    }

    #[test]
    fn test_no_trigger_below_limit() {
        let current = Version::parse("V1.998");
        let (next, reset) = current.calculate_next(5, 20, 999);
        assert_eq!(
            next,
            Version {
                major: 1,
                minor: 999
            }
        );
        assert!(!reset);
    }

    #[test]
    fn test_empty_new_state_no_massive_change() {
        // If unchanged=0 but total_files=0, it's an empty folder, not massive change
        let current = Version::parse("V1.5");
        let (next, reset) = current.calculate_next(0, 0, 999);
        // is_massive_change requires total_files > 0
        assert_eq!(next, Version { major: 1, minor: 6 });
        assert!(!reset);
    }

    // --- DISPLAY TESTS ---

    #[test]
    fn test_display_v1() {
        let v = Version::new_v1();
        assert_eq!(format!("{}", v), "V1");
    }

    #[test]
    fn test_display_v1_with_minor() {
        let v = Version { major: 1, minor: 5 };
        assert_eq!(format!("{}", v), "V1.5");
    }

    #[test]
    fn test_display_v2() {
        let v = Version { major: 2, minor: 0 };
        assert_eq!(format!("{}", v), "V2");
    }

    #[test]
    fn test_display_v2_with_minor() {
        let v = Version {
            major: 2,
            minor: 10,
        };
        assert_eq!(format!("{}", v), "V2.10");
    }

    #[test]
    fn test_display_large_version() {
        let v = Version {
            major: 100,
            minor: 999,
        };
        assert_eq!(format!("{}", v), "V100.999");
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

    #[test]
    fn maximum_revision_refuses_and_the_prior_revision_advances() {
        // Direct arithmetic reproducer: calculate_next(0, 1, 999) on the maximum major.
        // The tuple API cannot carry the error; try_calculate_next is the typed refusal.
        let max = Version { major: u32::MAX, minor: 7 };
        let (sentinel, reset) = max.calculate_next(0, 1, 999);
        assert_eq!(sentinel, Version { major: 0, minor: 0 });
        assert!(reset);
        assert_eq!(max.try_calculate_next(0, 1, 999), Err(VersionExhausted { major: u32::MAX, minor: 7 }));
        let prior = Version { major: u32::MAX - 1, minor: 7 };
        assert_eq!(prior.try_calculate_next(0, 1, 999), Ok((Version { major: u32::MAX, minor: 0 }, true)));
        assert_eq!(prior.try_calculate_next(3, 4, 999), Ok((Version { major: u32::MAX - 1, minor: 8 }, false)));
    }
}
