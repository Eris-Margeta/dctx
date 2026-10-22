//! Checked next-version rules and the legacy exhaustion sentinel.
//! Implements pack versioning; docs/reviews/dctx-legacy-removal.md F11.
//! Depends on the parent version types; pure, no IO.
//! Used by Version callers and the pack diff version decision.
//! Major changes and the diff limit reset the minor revision.

use super::{Version, VersionExhausted};

impl Version {
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
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
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
    /// # Errors
    /// Returns `VersionExhausted` if the selected major or minor increment overflows.
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

#[cfg(test)]
mod tests {
    use super::*;

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
