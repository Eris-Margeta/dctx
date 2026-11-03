// SPDX-License-Identifier: Apache-2.0
//! Stable numeric tags for segment sections, facts, edges and ranking fields.
//! Implements SEGMENTS-v1 schema 1 and the SPEC-MAP-MODE confidence matrix.
//! Depends on no other module. Encoders and decoders share these codes.
//! Codes are part of the v1 layout. Adding a variant needs a new schema.

macro_rules! code_enum {
    ($name:ident, $doc:literal, $($variant:ident => $code:literal, $text:literal),+ $(,)?) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $(
                #[doc = $text]
                $variant,
            )+
        }
        impl $name {
            /// Numeric tag stored in the segment.
            pub const fn code(self) -> u16 {
                match self {
                    $(Self::$variant => $code,)+
                }
            }
            /// Restores a tag. Unknown tags are `None`.
            pub const fn from_code(code: u16) -> Option<Self> {
                match code {
                    $($code => Some(Self::$variant),)+
                    _ => None,
                }
            }
            /// Short stable name.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)+
                }
            }
        }
    };
}

code_enum!(
    SectionKind,
    "Directory kind. Unknown kinds are skippable only when they are not required.",
    Dictionary => 1, "dictionary",
    Prefix => 2, "prefix",
    Facts => 3, "facts",
    Postings => 4, "postings",
    Nodes => 5, "nodes",
    Forward => 6, "forward",
    Reverse => 7, "reverse",
);
code_enum!(
    Language,
    "Fact-key language tag. Identity v1 assigns these numbers.",
    Rust => 1, "rust",
    JavaScript => 2, "javascript",
    TypeScript => 3, "typescript",
    Markdown => 4, "markdown",
    Opaque => 5, "opaque",
);
code_enum!(
    Field,
    "Posting field. Path postings are refused in a fact segment.",
    Identifier => 1, "identifier",
    Path => 2, "path",
    Doc => 3, "doc",
);
code_enum!(
    EdgeKind,
    "Adjacency relation. The numeric order is the on-disk tag.",
    Contains => 1, "contains",
    Imports => 2, "imports",
    Calls => 3, "calls",
    Implements => 4, "implements",
    UsesType => 5, "uses-type",
    Tests => 6, "tests",
    Documents => 7, "documents",
    Reexports => 8, "reexports",
);
code_enum!(
    Confidence,
    "Resolution confidence stored beside an edge.",
    Exact => 1, "exact",
    Name => 2, "name",
    Guess => 3, "guess",
);
code_enum!(
    DeclarationKind,
    "File-local declaration tag. Place and package nodes are not fact records.",
    Module => 1, "module",
    Function => 2, "function",
    Method => 3, "method",
    Struct => 4, "struct",
    Enum => 5, "enum",
    Trait => 6, "trait",
    Impl => 7, "impl",
    Const => 8, "const",
    Type => 9, "type",
    Class => 10, "class",
    Interface => 11, "interface",
    Macro => 12, "macro",
    Test => 13, "test",
    Doc => 14, "doc",
);
code_enum!(
    SummarySource,
    "Provenance of a declaration summary.",
    Doc => 1, "doc",
    Signature => 2, "sig",
    Heading => 3, "heading",
    Llm => 4, "llm",
);
code_enum!(
    Coverage,
    "What the extractor claims to understand. This is not edge confidence.",
    Syntax => 1, "syntax",
    Structural => 2, "structural",
    Opaque => 3, "opaque",
);
code_enum!(
    Completion,
    "Whether extraction finished. Partial and refused carry a reason string.",
    Complete => 1, "complete",
    Partial => 2, "partial",
    Refused => 3, "refused",
);

impl SectionKind {
    /// Every kind this decoder knows, in directory order.
    pub const ALL: [Self; 7] = [
        Self::Dictionary,
        Self::Prefix,
        Self::Facts,
        Self::Postings,
        Self::Nodes,
        Self::Forward,
        Self::Reverse,
    ];
}

impl Field {
    /// Ranking revision 1 weight for this field.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn weight(self) -> u32 {
        match self {
            Self::Identifier => 8,
            Self::Path => 3,
            Self::Doc => 1,
        }
    }
}

impl EdgeKind {
    /// Whether SPEC-MAP-MODE section 5.3 permits this confidence on the relation.
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

/// Which section set a segment is allowed to carry. Mixed roles are refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SegmentRole {
    /// Requires the dictionary and fact sections. Optional prefix and postings.
    Fact,
    /// Requires the dictionary, view nodes, and both adjacency sections.
    View,
}

impl SegmentRole {
    /// Sections that must be present and flagged required.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn requires(self, kind: SectionKind) -> bool {
        matches!(
            (self, kind),
            (Self::Fact, SectionKind::Dictionary | SectionKind::Facts)
                | (
                    Self::View,
                    SectionKind::Dictionary
                        | SectionKind::Nodes
                        | SectionKind::Forward
                        | SectionKind::Reverse,
                )
        )
    }

    /// Sections that must be absent.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn forbids(self, kind: SectionKind) -> bool {
        matches!(
            (self, kind),
            (Self::Fact, SectionKind::Nodes | SectionKind::Forward | SectionKind::Reverse)
                | (Self::View, SectionKind::Facts)
        )
    }

    /// Prefix and postings may appear, without the required flag.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn optional(self, kind: SectionKind) -> bool {
        matches!(kind, SectionKind::Prefix | SectionKind::Postings) && !self.forbids(kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_and_unknown_tags_are_refused() {
        assert_eq!(SectionKind::from_code(1), Some(SectionKind::Dictionary));
        assert_eq!(SectionKind::Reverse.code(), 7);
        assert_eq!(Language::from_code(5), Some(Language::Opaque));
        assert_eq!(Language::from_code(0), None);
        assert_eq!(EdgeKind::from_code(9), None);
        assert_eq!(Field::Identifier.weight(), 8);
        assert_eq!(Field::Path.weight(), 3);
        assert_eq!(Field::Doc.weight(), 1);
        assert_eq!(DeclarationKind::Function.as_str(), "function");
    }

    #[test]
    fn confidence_matrix_matches_the_spec() {
        for kind in [
            EdgeKind::Contains,
            EdgeKind::Imports,
            EdgeKind::Calls,
            EdgeKind::Implements,
            EdgeKind::UsesType,
            EdgeKind::Tests,
            EdgeKind::Documents,
            EdgeKind::Reexports,
        ] {
            for confidence in [Confidence::Exact, Confidence::Name, Confidence::Guess] {
                let allowed = match kind {
                    EdgeKind::Contains => confidence == Confidence::Exact,
                    EdgeKind::Calls => {
                        matches!(confidence, Confidence::Name | Confidence::Guess)
                    }
                    EdgeKind::UsesType | EdgeKind::Tests | EdgeKind::Documents => {
                        confidence == Confidence::Name
                    }
                    EdgeKind::Imports | EdgeKind::Implements | EdgeKind::Reexports => {
                        matches!(confidence, Confidence::Exact | Confidence::Name)
                    }
                };
                assert_eq!(kind.permits(confidence), allowed);
            }
        }
        assert!(SegmentRole::Fact.requires(SectionKind::Facts));
        assert!(SegmentRole::Fact.forbids(SectionKind::Nodes));
        assert!(SegmentRole::View.optional(SectionKind::Postings));
        assert!(!SegmentRole::View.optional(SectionKind::Facts));
    }
}
