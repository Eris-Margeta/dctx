//! Graph kinds, source spans and fingerprint-bound summaries.
//! Implements SPEC-MAP-MODE sections 5.1, 5.3 and 5.4.
//! JSON derives exist only behind the `wire-json` feature. Indexers exchange these facts.
use super::StableId;

macro_rules! vocabulary {
    ($name:ident, $doc:literal, {$($variant:ident => $text:literal),+ $(,)?}) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        #[cfg_attr(feature = "wire-json", derive(serde::Serialize, serde::Deserialize))]
        pub enum $name { $(
            #[doc = $text]
            #[cfg_attr(feature = "wire-json", serde(rename = $text))]
            $variant
        ),+ }
        impl $name {
            /// Canonical agent-facing spelling.
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }
    };
}
vocabulary!(NodeKind, "Graph node category and its stable-id prefix.", {
    Place => "place", Package => "package", Module => "module",
    Symbol => "symbol", Test => "test", Doc => "doc"
});
impl NodeKind {
    /// Prefix used by stable ids.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn letter(self) -> char {
        match self {
            Self::Place => 'p',
            Self::Package => 'k',
            Self::Module => 'm',
            Self::Symbol => 's',
            Self::Test => 't',
            Self::Doc => 'd',
        }
    }
}
vocabulary!(SymbolKind, "Parsed symbol category, distinct from graph node category.", {
    Function => "fn", Method => "method", Struct => "struct", Enum => "enum",
    Trait => "trait", Impl => "impl", Const => "const", Type => "type",
    Class => "class", Interface => "interface", Macro => "macro"
});
vocabulary!(EdgeKind, "Explicit graph relation.", {
    Contains => "contains", Imports => "imports", Calls => "calls",
    Implements => "implements", UsesType => "uses-type", Tests => "tests",
    Documents => "documents", Reexports => "reexports"
});
vocabulary!(Confidence, "Resolution confidence carried by every edge.", {
    Exact => "exact", Name => "name", Guess => "guess"
});
impl EdgeKind {
    /// Whether section 5.3 permits this confidence for the relation.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn permits(self, confidence: Confidence) -> bool {
        match self {
            Self::Contains => matches!(confidence, Confidence::Exact),
            Self::Calls => matches!(confidence, Confidence::Name | Confidence::Guess),
            Self::UsesType | Self::Tests | Self::Documents => {
                matches!(confidence, Confidence::Name)
            }
            Self::Imports | Self::Implements | Self::Reexports => {
                matches!(confidence, Confidence::Exact | Confidence::Name)
            }
        }
    }
}
vocabulary!(SummarySource, "Provenance shown with every summary.", {
    Doc => "doc", Signature => "sig", Heading => "heading", Llm => "llm"
});

/// Directed relation owned by the file that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "wire-json", derive(serde::Serialize, serde::Deserialize))]
pub struct Edge {
    /// Source node.
    pub src: StableId,
    /// Destination node.
    pub dst: StableId,
    /// Named relation.
    pub kind: EdgeKind,
    /// Resolution quality.
    pub conf: Confidence,
    /// File replaced when reindexing these facts.
    pub owner_path: String,
}

/// First-party summary with explicit provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "wire-json", derive(serde::Serialize, serde::Deserialize))]
pub struct Summary {
    /// Single-line text.
    pub text: String,
    /// Origin of the summary.
    pub source: SummarySource,
}

/// Imported tier-one summary tied to an exact body fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "wire-json", derive(serde::Serialize, serde::Deserialize))]
pub struct GeneratedSummary {
    /// Whitespace-normalized body SHA-256 supplied by the indexer.
    pub fingerprint: String,
    /// Generated single-line summary, at most 100 characters.
    pub text: String,
}
impl GeneratedSummary {
    /// Serves valid current summaries, otherwise returns the tier-zero fallback.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn select(&self, fingerprint: &str, fallback: &Summary) -> Summary {
        if self.fingerprint == fingerprint
            && self.text.chars().count() <= 100
            && !self.text.contains(['\n', '\r'])
            && !self.text.is_empty()
        {
            Summary {
                text: self.text.clone(),
                source: SummarySource::Llm,
            }
        } else {
            fallback.clone()
        }
    }
}

/// Parser/storage node; locations are one-based inclusive line numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "wire-json", derive(serde::Serialize, serde::Deserialize))]
pub struct Node {
    /// Stable path/name identity.
    pub id: StableId,
    /// Graph category.
    pub kind: NodeKind,
    /// Optional parsed symbol category.
    pub symbol_kind: Option<SymbolKind>,
    /// Place-relative source file.
    pub path: String,
    /// Inclusive start and end lines.
    pub lines: [u32; 2],
    /// Qualified item name.
    pub name: String,
    /// Source signature.
    pub signature: String,
    /// Body fingerprint supplied by the parser/indexer.
    pub fingerprint: String,
    /// Public visibility used by ranking.
    pub public: bool,
    /// Tier-zero summary.
    pub summary: Summary,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confidence_matrix_matches_the_spec() {
        for (kind, allowed) in [
            (EdgeKind::Contains, vec![Confidence::Exact]),
            (EdgeKind::Calls, vec![Confidence::Name, Confidence::Guess]),
            (EdgeKind::Imports, vec![Confidence::Exact, Confidence::Name]),
            (
                EdgeKind::Implements,
                vec![Confidence::Exact, Confidence::Name],
            ),
            (
                EdgeKind::Reexports,
                vec![Confidence::Exact, Confidence::Name],
            ),
            (EdgeKind::UsesType, vec![Confidence::Name]),
            (EdgeKind::Tests, vec![Confidence::Name]),
            (EdgeKind::Documents, vec![Confidence::Name]),
        ] {
            for confidence in [Confidence::Exact, Confidence::Name, Confidence::Guess] {
                assert_eq!(kind.permits(confidence), allowed.contains(&confidence));
            }
        }
    }
    #[test]
    fn generated_summaries_require_current_fingerprint_and_valid_text() {
        let base = Summary {
            text: "signature".into(),
            source: SummarySource::Signature,
        };
        let mut generated = GeneratedSummary {
            fingerprint: "abc".into(),
            text: "Generated.".into(),
        };
        assert_eq!(generated.select("abc", &base).source, SummarySource::Llm);
        assert_eq!(generated.select("changed", &base), base);
        generated.text = "x".repeat(101);
        assert_eq!(generated.select("abc", &base), base);
        generated.text = "bad\nline".into();
        assert_eq!(generated.select("abc", &base), base);
        generated.text = "é".repeat(100);
        assert_eq!(generated.select("abc", &base).source, SummarySource::Llm);
        generated.text.clear();
        assert_eq!(generated.select("abc", &base), base);
    }
}
