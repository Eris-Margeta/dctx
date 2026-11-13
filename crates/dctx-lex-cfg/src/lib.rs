// SPDX-License-Identifier: Apache-2.0
//! Hand-written, zero-dependency configuration structural extractor.
//! Implements decision 077, PARSERS-v1 and SPEC-MAP-MODE 5–6.2 for JSON, TOML and YAML.
//! Depends on dctx-map-model facts and dctx-hash; never on tree-sitter, git or mmap.
//! Emits the dctx-map-parse fact shape with a precision label on every fact.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod domain;
mod json;
mod path;
mod toml;
mod yaml;

pub use domain::{
    Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexError, LexedFile, Limits,
    Precision, RawReference, body_fingerprint,
};

/// Parser identity recorded with every fact key (PARSERS-v1 provenance).
pub const PARSER_ID: &str = "dctx-lex-cfg";
/// Bumped whenever extraction output can change for the same bytes.
pub const PARSER_REVISION: u32 = 2;

/// Bounded structural extractor. Stateless apart from its limits.
///
/// Instances can be shared between threads. Repeated extraction of identical paths
/// and bytes with the same limits returns identical facts in source order.
#[derive(Debug, Clone, Copy, Default)]
pub struct CfgLexer {
    limits: Limits,
}

impl CfgLexer {
    /// Builds an extractor. Limits above the ceilings are narrowed to them.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn new(limits: Limits) -> Self {
        Self {
            limits: limits.narrowed(),
        }
    }

    /// Extracts facts from one place-relative path and its authorized bytes.
    ///
    /// The final extension selects the dialect, case-insensitively. Malformed syntax
    /// returns a [`Completion::Partial`] file with bounded diagnostics, rather than
    /// a refusal. Empty input returns the file node. A leading UTF-8 BOM is skipped.
    /// Check `completion` and `diagnostics` before relying on structural precision.
    ///
    /// # Errors
    /// Refuses unsupported paths, invalid UTF-8 or model identities, and
    /// input exceeding the byte, nesting or fact ceilings. Path extension and byte
    /// size are checked before UTF-8 decoding. Correct the path or encoding, or
    /// reduce the file's size, nesting or fact count, then retry.
    pub fn extract(&self, path: &str, bytes: &[u8]) -> Result<LexedFile, LexError> {
        let dialect = Dialect::for_path(path).ok_or(LexError::UnsupportedDialect)?;
        if bytes.len() > self.limits.max_bytes {
            return Err(LexError::TooLarge {
                bytes: bytes.len(),
                limit: self.limits.max_bytes,
            });
        }
        let decoded = std::str::from_utf8(bytes)?;
        let source = decoded.strip_prefix('\u{feff}').unwrap_or(decoded);
        let mut file = match dialect {
            Dialect::Json => json::extract(path, source, false, &self.limits),
            Dialect::Jsonc => json::extract(path, source, true, &self.limits),
            Dialect::Toml => toml::extract(path, source, &self.limits),
            Dialect::Yaml => yaml::extract(path, source, &self.limits),
        }?;
        // Walkers index the BOM-free source; callers index the supplied bytes.
        let prefix = u32::try_from(decoded.len() - source.len()).unwrap_or(0);
        for diagnostic in &mut file.diagnostics {
            diagnostic.byte += prefix;
        }
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_are_typed_with_positive_controls() {
        let lexer = CfgLexer::default();
        assert!(matches!(
            lexer.extract("x.py", b""),
            Err(LexError::UnsupportedDialect)
        ));
        assert!(matches!(
            lexer.extract("x.json", &[0xff]),
            Err(LexError::InvalidUtf8(_))
        ));
        assert!(matches!(
            lexer.extract("../x.json", b""),
            Err(LexError::InvalidId(_))
        ));
        let small = CfgLexer::new(Limits {
            max_bytes: 4,
            ..Limits::default()
        });
        assert!(matches!(
            small.extract("x.json", b"{\"a\":1}"),
            Err(LexError::TooLarge { bytes: 7, limit: 4 })
        ));
        assert!(small.extract("x.json", b"{}").is_ok());
        assert!(lexer.extract("x.json", b"").is_ok());
        assert_eq!(
            lexer.extract("x.YML", b"a: 1\n").unwrap().dialect,
            Dialect::Yaml
        );
    }

    #[test]
    fn limits_never_widen_and_bound_depth_and_facts() {
        let wide = CfgLexer::new(Limits {
            max_bytes: usize::MAX,
            max_depth: u32::MAX,
            max_facts: usize::MAX,
        });
        assert_eq!(wide.limits, Limits::CEILING);
        let tight = CfgLexer::new(Limits {
            max_facts: 2,
            ..Limits::default()
        });
        assert!(tight.extract("p.json", b"{\"a\":1}").is_ok());
        assert!(matches!(
            tight.extract("p.json", b"{\"a\":1,\"b\":2}"),
            Err(LexError::TooManyFacts { .. })
        ));
        let deep = CfgLexer::new(Limits {
            max_depth: 1,
            ..Limits::default()
        });
        assert!(deep.extract("p.json", b"{\"a\":1}").is_ok());
        assert!(matches!(
            deep.extract("p.json", b"{\"a\":{\"b\":1}}"),
            Err(LexError::TooDeep { .. })
        ));
        assert!(deep.extract("p.toml", b"a = 1\n").is_ok());
        assert!(matches!(
            deep.extract("p.toml", b"[a]\nb = 1\n"),
            Err(LexError::TooDeep { .. })
        ));
        assert!(deep.extract("p.yaml", b"a: 1\n").is_ok());
        assert!(matches!(
            deep.extract("p.yaml", b"a:\n  b: 1\n"),
            Err(LexError::TooDeep { .. })
        ));
        let nested = CfgLexer::new(Limits {
            max_depth: 2,
            ..Limits::default()
        });
        assert!(nested.extract("p.json", b"{\"a\":{\"b\":1}}").is_ok());
        assert!(nested.extract("p.toml", b"[a]\nb = 1\n").is_ok());
        assert!(nested.extract("p.yaml", b"a:\n  b: 1\n").is_ok());
    }

    #[test]
    fn references_keep_obvious_paths_and_abstain() {
        use crate::path::{dependency_table, obvious_member, obvious_path};
        assert!(dependency_table("devDependencies") && dependency_table("workspace.dependencies"));
        assert!(!dependency_table("dependencies.serde") && !dependency_table("members"));
        assert!(obvious_member("workspace.members", "crates/core"));
        assert!(!obvious_member("members", "crates/core"));
        assert!(obvious_member("workspaces", "packages/app"));
        assert!(obvious_path("path", "src/main.rs") && obvious_path("any", "../local"));
        assert!(obvious_path("any", "./src/lib.rs"));
        assert!(!obvious_path("site", "https://example.com/a"));
        assert!(!obvious_path("version", "1.0.0") && !obvious_path("include", "crates/*"));
        assert!(!obvious_path("name", "demo"));
    }

    #[test]
    fn damaged_containment_keeps_exact_confidence() {
        // Not a defect (review finding 11). SPEC-MAP-MODE §5.3 permits only
        // Confidence::Exact on EdgeKind::Contains. A span past the first
        // diagnostic is already Precision::Partial; the contains edge stays exact.
        use dctx_map_model::{Confidence, EdgeKind};
        let lexer = CfgLexer::default();
        let damaged = lexer
            .extract("a.json", br#"{"ready":1,"broken":}"#)
            .unwrap();
        assert_eq!(damaged.completion, Completion::Partial);
        assert_ne!(damaged.diagnostics, []);
        assert!(
            damaged
                .edges
                .iter()
                .any(|edge| edge.precision == Precision::Partial)
        );
        assert!(
            damaged
                .edges
                .iter()
                .any(|edge| edge.precision == Precision::Structural)
        );
        for edge in &damaged.edges {
            assert_eq!(edge.fact.kind, EdgeKind::Contains);
            assert_eq!(edge.fact.conf, Confidence::Exact);
            assert!(edge.fact.kind.permits(edge.fact.conf));
        }
        let clean = lexer
            .extract("a.json", br#"{"ready":1,"broken":2}"#)
            .unwrap();
        assert_eq!(
            clean.completion,
            Completion::Complete,
            "{:?}",
            clean.diagnostics
        );
        for edge in &clean.edges {
            assert_eq!(edge.precision, Precision::Structural);
            assert_eq!(edge.fact.conf, Confidence::Exact);
            assert!(edge.fact.kind.permits(edge.fact.conf));
        }
    }
}
