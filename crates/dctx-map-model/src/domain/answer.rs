//! Bounded query answer records and explicit moved/gone outcomes.
//! Implements SPEC-MAP-MODE sections 3, 4 and 5.2.
//! Query engines supply ranked facts. The renderer enforces the final byte limit.
//! JSON traits exist only behind the `wire-json` feature.
use super::{Confidence, Edge, EdgeKind, Node, StableId, SummarySource};

/// Default output budget, estimated tokens.
pub const DEFAULT_BUDGET: u16 = 500;
/// Absolute output budget ceiling.
pub const MAX_BUDGET: u16 = 2000;

/// A validated positive token limit, capped at 2,000.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wire-json", derive(serde::Serialize))]
#[cfg_attr(feature = "wire-json", serde(transparent))]
pub struct AnswerBudget(u16);
impl AnswerBudget {
    /// Default for the selected verb (explain uses 400 tokens).
    pub const fn for_verb(verb: Verb) -> Self {
        Self(if matches!(verb, Verb::Explain) {
            400
        } else {
            DEFAULT_BUDGET
        })
    }
    /// Rejects zero and values above the hard maximum.
    pub fn new(tokens: u16) -> Result<Self, InvalidBudget> {
        if tokens == 0 || tokens > MAX_BUDGET {
            Err(InvalidBudget)
        } else {
            Ok(Self(tokens))
        }
    }
    /// Token ceiling supplied by the caller.
    pub const fn tokens(self) -> u16 {
        self.0
    }
    /// Maximum bytes whose ceil(bytes / 3.5) fits the token ceiling.
    pub const fn bytes(self) -> usize {
        self.0 as usize * 7 / 2
    }
    /// Tests actual serialized output, including all metadata.
    pub fn fits(self, output: &str) -> bool {
        output.len() <= self.bytes()
    }
}
impl Default for AnswerBudget {
    fn default() -> Self {
        Self(DEFAULT_BUDGET)
    }
}
#[cfg(feature = "wire-json")]
impl<'de> serde::Deserialize<'de> for AnswerBudget {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::new(u16::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
/// An out-of-range answer budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidBudget;
impl std::fmt::Display for InvalidBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("budget must be 1..=2000 tokens")
    }
}
impl std::error::Error for InvalidBudget {}

/// Supported read-only map queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wire-json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "wire-json", serde(rename_all = "lowercase"))]
pub enum Verb {
    /// Hierarchical outline.
    Map,
    /// Ranked concept lookup.
    Find,
    /// Explicit graph relations.
    Neighbors,
    /// Fixed-order node details.
    Explain,
    /// Exact source span.
    Read,
}

/// Edge metadata prefixed to a neighbor record.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "wire-json", derive(serde::Serialize, serde::Deserialize))]
pub struct Relation {
    /// Named edge.
    pub edge: EdgeKind,
    /// Resolution confidence.
    pub confidence: Confidence,
    /// Whether this is an incoming edge.
    pub incoming: bool,
}

/// One indivisible query result; JSON keys match section 4.2.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "wire-json", derive(serde::Serialize, serde::Deserialize))]
pub struct Record {
    /// Copyable stable id.
    pub id: StableId,
    /// Display kind (for example fn, struct, module or doc).
    pub kind: String,
    /// Place-relative source path.
    pub path: String,
    /// Inclusive one-based source span.
    pub l: [u32; 2],
    /// Summary text.
    pub sum: String,
    /// Summary provenance.
    pub src: SummarySource,
    /// Optional explicit neighbor edge.
    #[cfg_attr(
        feature = "wire-json",
        serde(skip_serializing_if = "Option::is_none", default)
    )]
    pub relation: Option<Relation>,
    /// Whether the record could not be refreshed in time.
    #[cfg_attr(feature = "wire-json", serde(skip_serializing_if = "is_false", default))]
    pub stale: bool,
    /// Signature shown by explain.
    #[cfg_attr(
        feature = "wire-json",
        serde(skip_serializing_if = "Option::is_none", default)
    )]
    pub signature: Option<String>,
    /// Top incoming edges for explain, at most five.
    #[cfg_attr(
        feature = "wire-json",
        serde(skip_serializing_if = "Vec::is_empty", default)
    )]
    pub incoming: Vec<Edge>,
    /// Top outgoing edges for explain, at most five.
    #[cfg_attr(
        feature = "wire-json",
        serde(skip_serializing_if = "Vec::is_empty", default)
    )]
    pub outgoing: Vec<Edge>,
    /// Covering tests for explain.
    #[cfg_attr(
        feature = "wire-json",
        serde(skip_serializing_if = "Vec::is_empty", default)
    )]
    pub tests: Vec<StableId>,
    /// Documentation mentioning this node.
    #[cfg_attr(
        feature = "wire-json",
        serde(skip_serializing_if = "Vec::is_empty", default)
    )]
    pub documents: Vec<StableId>,
    /// Exact source span for read; preserved as a whole record.
    #[cfg_attr(
        feature = "wire-json",
        serde(skip_serializing_if = "Option::is_none", default)
    )]
    pub body: Option<String>,
    /// Integer query rank; excluded from the public JSON shape.
    #[cfg_attr(feature = "wire-json", serde(skip))]
    pub score: i64,
}
#[cfg(feature = "wire-json")]
fn is_false(value: &bool) -> bool {
    !value
}
impl From<&Node> for Record {
    fn from(node: &Node) -> Self {
        Self {
            id: node.id.clone(),
            kind: node
                .symbol_kind
                .map_or(node.kind.as_str(), |k| k.as_str())
                .into(),
            path: node.path.clone(),
            l: node.lines,
            sum: node.summary.text.clone(),
            src: node.summary.source,
            relation: None,
            stale: false,
            signature: None,
            incoming: vec![],
            outgoing: vec![],
            tests: vec![],
            documents: vec![],
            body: None,
            score: 0,
        }
    }
}

/// Lookup result for identities that disappeared from the index.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "wire-json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "wire-json", serde(tag = "status", rename_all = "lowercase"))]
pub enum IdentityStatus {
    /// Old id resolves to a new node; its record follows the notice.
    Moved {
        /// Disappeared identity.
        old: StableId,
        /// Replacement identity.
        new: StableId,
    },
    /// No replacement exists.
    Gone {
        /// Disappeared identity.
        old: StableId,
    },
}

/// Unrendered answer; timestamps and commands are supplied, never read from IO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// Place name.
    pub place: String,
    /// Index revision (rendered as seven characters).
    pub rev: String,
    /// Dirty file count.
    pub dirty: usize,
    /// Index age in seconds.
    pub age_seconds: u64,
    /// Query verb.
    pub verb: Verb,
    /// Results, sorted by score descending then id ascending by the renderer.
    pub records: Vec<Record>,
    /// Additional results not included in this query page.
    pub more: usize,
    /// Exact continuation command when any results remain.
    pub next: String,
    /// Optional moved or gone notice.
    pub identity: Option<IdentityStatus>,
    /// Empty/refused reason; requires a concrete next command.
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budgets_are_positive_capped_and_byte_accurate() {
        assert!(AnswerBudget::new(0).is_err());
        assert!(AnswerBudget::new(2001).is_err());
        assert_eq!(AnswerBudget::default().tokens(), 500);
        assert_eq!(AnswerBudget::for_verb(Verb::Explain).tokens(), 400);
        assert_eq!(AnswerBudget::for_verb(Verb::Map).tokens(), 500);
        assert!(AnswerBudget::new(2).unwrap().fits("1234567"));
        assert!(!AnswerBudget::new(2).unwrap().fits("12345678"));
        #[cfg(feature = "wire-json")]
        {
            assert!(serde_json::from_str::<AnswerBudget>("0").is_err());
            assert_eq!(
                serde_json::from_str::<AnswerBudget>("2000")
                    .unwrap()
                    .tokens(),
                2000
            );
        }
    }
}
