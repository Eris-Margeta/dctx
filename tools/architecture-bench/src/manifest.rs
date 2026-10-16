//! Checked-in corpus recipe for the DX-A1 baseline.
//! Implements ARCHITECTURE-PROPOSAL §7 Stage 0 manifest/hash contract.
//! Depends on serde; the runner loads `corpus/manifest.json` through this module.

use crate::error::BenchError;
use crate::queries::WARM_QUERIES;
use serde::Deserialize;

/// Corpus recipe. Hashes are recorded in the report, not stored here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    /// `dx-a1-corpus-v1`.
    pub schema: String,
    /// Number of warm queries. Must match the runner's fixed list.
    pub warm_query_count: u32,
    /// Representative `find` target for the five-verb sample.
    pub verb_find_target: String,
    /// Pin of this repository.
    pub dctx: DctxSpec,
    /// Generated second corpus.
    pub fixture: FixtureSpec,
}

/// How the dctx worktree is pinned.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DctxSpec {
    /// Corpus id stored as the map place label.
    pub id: String,
    /// Directory names skipped anywhere in the content hash.
    pub skip_dir_names: Vec<String>,
    /// Relative prefixes skipped by the content hash.
    pub skip_prefixes: Vec<String>,
}

/// Size budget for the generated public-domain fixture.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FixtureSpec {
    /// Corpus id.
    pub id: String,
    /// Generator name. Only `dx-a1-fixture-v1` is accepted.
    pub generator: String,
    /// Dedication asserted in every generated file.
    pub dedication: String,
    /// Seed mixed into every generated file.
    pub seed: String,
    /// Size class. `public-domain-text` means original CC0 text, no binaries.
    pub size_class: String,
    /// Rust file count.
    pub rust: u32,
    /// JavaScript file count.
    pub javascript: u32,
    /// TypeScript file count.
    pub typescript: u32,
    /// Markdown file count.
    pub markdown: u32,
}

impl Manifest {
    /// Parses and refuses a recipe the runner cannot reproduce.
    pub fn parse(text: &str) -> Result<Self, BenchError> {
        let manifest: Self = serde_json::from_str(text)?;
        if manifest.schema != "dx-a1-corpus-v1" {
            return Err(BenchError::Message(format!(
                "unsupported corpus schema {}",
                manifest.schema
            )));
        }
        if manifest.warm_query_count != WARM_QUERIES.len() as u32 {
            return Err(BenchError::Message(format!(
                "warm_query_count must be {}",
                WARM_QUERIES.len()
            )));
        }
        if manifest.verb_find_target.is_empty()
            || manifest.verb_find_target.chars().any(char::is_control)
        {
            return Err(BenchError::Message(
                "verb_find_target must be a non-empty plain string".into(),
            ));
        }
        manifest.dctx.validate()?;
        manifest.fixture.validate()?;
        Ok(manifest)
    }
}

impl DctxSpec {
    fn validate(&self) -> Result<(), BenchError> {
        if self.id.is_empty() {
            return Err(BenchError::Message("dctx corpus id is empty".into()));
        }
        for name in &self.skip_dir_names {
            if name.is_empty() || name.contains('/') || name.contains("..") {
                return Err(BenchError::Message(format!(
                    "skip_dir_names entry is not a single directory name: {name}"
                )));
            }
        }
        for prefix in &self.skip_prefixes {
            if prefix.is_empty()
                || prefix.starts_with('/')
                || prefix.split('/').any(|part| part == "..")
            {
                return Err(BenchError::Message(format!(
                    "skip prefix must be a relative path: {prefix}"
                )));
            }
        }
        Ok(())
    }
}

impl FixtureSpec {
    /// Refuses an empty seed, an unknown generator, or a zero-file corpus.
    pub fn validate(&self) -> Result<(), BenchError> {
        if self.generator != "dx-a1-fixture-v1" {
            return Err(BenchError::Message(format!(
                "unknown fixture generator {}",
                self.generator
            )));
        }
        if self.dedication != "CC0-1.0" || self.size_class != "public-domain-text" {
            return Err(BenchError::Message(
                "fixture must be CC0-1.0 public-domain text".into(),
            ));
        }
        if self.seed.is_empty() || self.id.is_empty() {
            return Err(BenchError::Message(
                "fixture seed and id must be non-empty".into(),
            ));
        }
        let total = self
            .rust
            .saturating_add(self.javascript)
            .saturating_add(self.typescript)
            .saturating_add(self.markdown);
        if total == 0 {
            return Err(BenchError::Message(
                "fixture must contain at least one file".into(),
            ));
        }
        for (label, count) in [
            ("rust", self.rust),
            ("javascript", self.javascript),
            ("typescript", self.typescript),
            ("markdown", self.markdown),
        ] {
            if count > 10_000 {
                return Err(BenchError::Message(format!(
                    "{label} file count {count} exceeds the fixture bound of 10000"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe() -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/manifest.json"),
        )
        .unwrap()
    }

    #[test]
    fn checked_in_manifest_is_accepted_and_a_bad_schema_is_refused() {
        let manifest = Manifest::parse(&recipe()).unwrap();
        assert_eq!(manifest.dctx.id, "dctx");
        assert_eq!(manifest.fixture.generator, "dx-a1-fixture-v1");
        assert_eq!(manifest.warm_query_count, 30);
        let mut broken: serde_json::Value = serde_json::from_str(&recipe()).unwrap();
        broken["schema"] = serde_json::Value::String("other".into());
        assert!(Manifest::parse(&broken.to_string()).is_err());
        broken["schema"] = serde_json::Value::String("dx-a1-corpus-v1".into());
        broken["fixture"]["seed"] = serde_json::Value::String(String::new());
        assert!(Manifest::parse(&broken.to_string()).is_err());
    }
}
