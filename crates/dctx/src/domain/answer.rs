//! Owned query answers. No SQL handle, parser node or storage path is reachable from these types.
//! Implements contracts/API-PORTS-v1.md `Answer` and the golden semantic projection.
//! Depends on [`super::request::Verb`]. The session fills one answer per successful query.
//! Gone, moved and ambiguous identities are outcomes, not errors.

use super::request::Verb;

/// How fresh the generation was at the moment of the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Freshness {
    /// [`super::request::FreshnessMode`] wire name.
    pub mode: String,
    /// `verified`, `metadata` or `stale`.
    pub state: String,
    /// Whole seconds on the injected clock since the snapshot.
    pub age_s: u64,
}

/// Parser provenance for the generation. Strict builds say `structural`, not `syntax`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Precision {
    /// `structural`, `syntax` or `opaque`.
    pub coverage: String,
    /// `complete`, `partial` or `metadata`.
    pub completion: String,
    /// Parser id, for example `dctx-structural/1`.
    pub parser: String,
}

/// A normal identity result that is not a successful record page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityOutcome {
    /// The id is not in this generation.
    Gone {
        /// The requested id.
        id: String,
    },
    /// The id was replaced by another id.
    Moved {
        /// The requested id.
        from: String,
        /// The replacement id.
        to: String,
    },
    /// More than one node matches and the facade will not guess.
    Ambiguous {
        /// The requested id.
        id: String,
    },
}

impl IdentityOutcome {
    /// Wire tag: `gone`, `moved` or `ambiguous`.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Gone { .. } => "gone",
            Self::Moved { .. } => "moved",
            Self::Ambiguous { .. } => "ambiguous",
        }
    }

    /// The id the caller asked for.
    pub fn id(&self) -> &str {
        match self {
            Self::Gone { id } | Self::Ambiguous { id } => id,
            Self::Moved { from, .. } => from,
        }
    }
}

/// One record in an [`Answer`]. `score` orders pages and is omitted from the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerRecord {
    /// Stable id.
    pub id: String,
    /// Symbol or heading kind (`fn`, `heading`, …).
    pub kind: String,
    /// Root-relative path.
    pub path: String,
    /// Inclusive 1-based lines.
    pub lines: [u32; 2],
    /// Short summary text.
    pub summary: String,
    /// `doc`, `heading`, `sig` or empty.
    pub summary_source: String,
    /// Signature without the body, when the verb is explain.
    pub signature: Option<String>,
    /// Byte span `[start, end)` in the file, when the verb is read.
    pub span: Option<[u64; 2]>,
    /// Verified source text, when the verb is read.
    pub source_utf8: Option<String>,
    /// Edge name for a neighbors record.
    pub edge: Option<String>,
    /// `out` or `in`.
    pub direction: Option<String>,
    /// Neighbor stable id.
    pub target: Option<String>,
    /// `exact`, `name` or `guess`. Calls are never `exact`.
    pub confidence: Option<String>,
    /// Sort key. Higher is a better find hit. Not part of the public JSON object.
    pub score: i64,
}

impl AnswerRecord {
    /// An empty record with `id` and `kind` set. Other fields stay blank.
    pub fn basic(id: impl Into<String>, kind: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            path: path.into(),
            lines: [0, 0],
            summary: String::new(),
            summary_source: String::new(),
            signature: None,
            span: None,
            source_utf8: None,
            edge: None,
            direction: None,
            target: None,
            confidence: None,
            score: 0,
        }
    }
}

/// One successful query page, including a gone or ambiguous identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// Pinned generation.
    pub generation: u64,
    /// Own snapshot digest (`v1:sha256:…:length`).
    pub snapshot: String,
    /// Verb that produced the page.
    pub verb: Verb,
    /// Whole records that fit the budget, best first.
    pub records: Vec<AnswerRecord>,
    /// Records that exist but were not included.
    pub more: u64,
    /// Cursor for the next page, or a next action such as `map`.
    pub next: Option<String>,
    /// Freshness of the generation.
    pub freshness: Freshness,
    /// Parser provenance.
    pub precision: Precision,
    /// Set when the id is gone, moved or ambiguous.
    pub outcome: Option<IdentityOutcome>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gone_is_an_outcome_with_a_stable_tag() {
        let outcome = IdentityOutcome::Gone { id: "s:src/lib.rs#removed".to_string() };
        assert_eq!(outcome.as_str(), "gone");
        assert_eq!(outcome.id(), "s:src/lib.rs#removed");
    }
}
