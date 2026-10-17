//! Build capabilities. These describe this binary, not the caller's host grants.
//! Implements contracts/API-PORTS-v1.md ("Typed error set and capabilities").
//! Depends on the crate's feature flags. [`crate::Session::capabilities`] returns [`Capabilities::current`].
//! A strict build reports no git executable, no native parser and no subprocess.

/// Facade, wire and parser facts for this build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// Facade crate version.
    pub facade_version: &'static str,
    /// `strict` or `enhanced`.
    pub profile: &'static str,
    /// JSON protocol version.
    pub wire_version: u32,
    /// DTS version this facade compiles.
    pub dts_version: u32,
    /// Segment envelope version.
    pub segment_version: u32,
    /// Languages the structural extractor recognises.
    pub languages: &'static [&'static str],
    /// Parser ids linked into this build. Answer precision stays structural unless a plan selects syntax coverage.
    pub parsers: Vec<&'static str>,
    /// `buffered-segments` or `caller-ports`.
    pub storage: &'static str,
    /// Cancellation granularity.
    pub cancellation: &'static str,
    /// True only when this build links a native extension. Strict builds are false.
    pub native_code: bool,
    /// Subprocess mechanism. The core uses none.
    pub subprocess: &'static str,
    /// True only when a git executable or a `.git` reader is linked. Always false here.
    pub git: bool,
    /// `none` or `syn-dependency`. Rust does not imply a transitive-unsafe-free build.
    pub unsafe_boundary: &'static str,
    /// Cargo features enabled in this build.
    pub features: Vec<&'static str>,
    /// Freshness modes the session can record.
    pub freshness_modes: &'static [&'static str],
}

impl Capabilities {
    /// Capabilities of the build that is running, independent of session grants.
    pub fn current() -> Self {
        let enhanced = cfg!(feature = "syntax-rust") || cfg!(feature = "wire-json");
        let mut parsers = vec!["dctx-structural/1", "dctx-lex-js:1", "dctx-lex-md:1"];
        if cfg!(feature = "syntax-rust") {
            parsers.push("dctx-lex-rust/syn-3.0.9");
        }
        let mut features = Vec::new();
        for (enabled, name) in [
            (cfg!(feature = "pack"), "pack"),
            (cfg!(feature = "map"), "map"),
            (cfg!(feature = "templates"), "templates"),
            (cfg!(feature = "fs"), "fs"),
            (cfg!(feature = "segments"), "segments"),
            (cfg!(feature = "cache"), "cache"),
            (cfg!(feature = "syntax-rust"), "syntax-rust"),
            (cfg!(feature = "wire-json"), "wire-json"),
        ] {
            if enabled {
                features.push(name);
            }
        }
        Self {
            facade_version: crate::FACADE_VERSION,
            profile: if enhanced { "enhanced" } else { "strict" },
            wire_version: crate::WIRE_VERSION,
            dts_version: 2,
            segment_version: 1,
            languages: &["rust", "javascript", "typescript", "markdown"],
            parsers,
            storage: if cfg!(feature = "segments") { "buffered-segments" } else { "caller-ports" },
            cancellation: "checkpoint",
            native_code: false,
            subprocess: "none",
            git: false,
            unsafe_boundary: if cfg!(feature = "syntax-rust") { "syn-dependency" } else { "none" },
            features,
            freshness_modes: &["immutable_snapshot", "strict", "metadata"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_build_is_strict_and_has_no_git() {
        let caps = Capabilities::current();
        assert!(!caps.git);
        assert!(!caps.native_code);
        assert_eq!(caps.subprocess, "none");
        assert_eq!(caps.wire_version, 2);
        assert!(caps.languages.contains(&"rust"));
        if cfg!(feature = "syntax-rust") || cfg!(feature = "wire-json") {
            assert_eq!(caps.profile, "enhanced");
        } else {
            assert_eq!(caps.profile, "strict");
            assert_eq!(caps.unsafe_boundary, "none");
        }
    }
}
