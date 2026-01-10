use std::fmt;

/// Predstavlja verzioniranje projekta (V{Major}.{Minor}).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

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
    pub fn calculate_next(
        &self,
        files_unchanged_count: usize,
        total_files_in_new_state: usize,
        max_diff_files: u32,
    ) -> (Version, bool) {
        
        // TRIGGER A: Massive Change Event
        // Ako niti jedan fajl nije ostao nepromijenjen, a imamo fajlove u novom stanju.
        // (Ako je total_files 0, to je prazan folder, ne radimo major bump nužno, ili da?
        //  Recimo da ako je sve obrisano, to je veliki change, ali ako je unchanged=0 i total=0...
        //  Držimo se pravila: Ako unchanged == 0 i imali smo neki history -> Major Bump).
        // Za sada jednostavna logika:
        let is_massive_change = files_unchanged_count == 0 && total_files_in_new_state > 0;

        // TRIGGER B: Diff Limit Exceeded
        let is_limit_exceeded = self.minor >= max_diff_files;

        if is_massive_change || is_limit_exceeded {
            // MAJOR BUMP (Soft Reset)
            return (
                Version {
                    major: self.major + 1,
                    minor: 0,
                },
                true, // Should delete old diffs
            );
        }

        // STANDARD INCREMENT
        (
            Version {
                major: self.major,
                minor: self.minor + 1,
            },
            false, // Keep old diffs
        )
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

    #[test]
    fn test_parsing() {
        assert_eq!(Version::parse("V1"), Version { major: 1, minor: 0 });
        assert_eq!(Version::parse("V1.5"), Version { major: 1, minor: 5 });
        assert_eq!(Version::parse("Garbage"), Version { major: 1, minor: 0 });
    }

    #[test]
    fn test_standard_increment() {
        let current = Version::parse("V1.5");
        let (next, reset) = current.calculate_next(10, 20, 999); // 10 unchanged, limit not reached
        assert_eq!(next, Version { major: 1, minor: 6 });
        assert_eq!(reset, false);
    }

    #[test]
    fn test_massive_change_trigger() {
        let current = Version::parse("V1.5");
        let (next, reset) = current.calculate_next(0, 20, 999); // 0 unchanged!
        assert_eq!(next, Version { major: 2, minor: 0 });
        assert_eq!(reset, true);
    }

    #[test]
    fn test_limit_trigger() {
        let current = Version::parse("V1.999");
        let (next, reset) = current.calculate_next(5, 20, 999); // limit reached
        assert_eq!(next, Version { major: 2, minor: 0 });
        assert_eq!(reset, true);
    }
}
