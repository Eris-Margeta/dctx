//! Parser coverage and completion, kept separate from edge confidence.
//! Implements PARSERS-v1 and API-PORTS-v1 precision labels.
//! Depends only on the standard library. Answers and provenance carry a [`Precision`].
//! A label is bounded so a wire adapter cannot echo an unbounded string.

/// Longest accepted evidence label, in UTF-8 bytes.
pub const MAX_LABEL_BYTES: usize = 256;

/// Why a label was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidLabel;

impl std::fmt::Display for InvalidLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("evidence label is empty, too long, or contains a control character")
    }
}

impl std::error::Error for InvalidLabel {}

/// Bounded parser, extractor or model name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceLabel(String);

impl EvidenceLabel {
    /// Accepts one line of at most [`MAX_LABEL_BYTES`] bytes.
    pub fn new(text: impl Into<String>) -> Result<Self, InvalidLabel> {
        let text = text.into();
        if text.is_empty()
            || text.trim().is_empty()
            || text.len() > MAX_LABEL_BYTES
            || text.chars().any(char::is_control)
        {
            Err(InvalidLabel)
        } else {
            Ok(Self(text))
        }
    }

    /// Borrowed label text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What the parser claims to understand. This is not edge confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Coverage {
    /// Supported syntax for the declared language.
    Syntax,
    /// Structural extraction without a full grammar claim.
    Structural,
    /// Path and metadata only.
    Opaque,
}

impl Coverage {
    /// Wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Structural => "structural",
            Self::Opaque => "opaque",
        }
    }
}

/// Whether extraction finished for the queried scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion {
    /// Every requested construct in scope was classified.
    Complete,
    /// Some spans were skipped. The reason is bounded.
    Partial(EvidenceLabel),
    /// Extraction refused. The reason is bounded.
    Refused(EvidenceLabel),
}

impl Completion {
    /// Wire spelling of the state, without the reason.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial(_) => "partial",
            Self::Refused(_) => "refused",
        }
    }

    /// Bounded reason for a partial or refused completion.
    pub fn reason(&self) -> Option<&EvidenceLabel> {
        match self {
            Self::Complete => None,
            Self::Partial(reason) | Self::Refused(reason) => Some(reason),
        }
    }
}

/// Precision of one answer or fact: coverage, completion and parser label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Precision {
    coverage: Coverage,
    completion: Completion,
    parser: EvidenceLabel,
}

impl Precision {
    /// Records an explicit parser label. Edge confidence is a different type.
    pub fn new(coverage: Coverage, completion: Completion, parser: EvidenceLabel) -> Self {
        Self {
            coverage,
            completion,
            parser,
        }
    }

    /// Extraction coverage.
    pub const fn coverage(&self) -> Coverage {
        self.coverage
    }

    /// Completion of this scope.
    pub const fn completion(&self) -> &Completion {
        &self.completion
    }

    /// Parser name and revision, for example `syn:reviewed`.
    pub const fn parser(&self) -> &EvidenceLabel {
        &self.parser
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_precision_keep_coverage_separate_from_completion() {
        let parser = EvidenceLabel::new("syn:reviewed").unwrap();
        assert_eq!(parser.as_str(), "syn:reviewed");
        let precision = Precision::new(Coverage::Syntax, Completion::Complete, parser);
        assert_eq!(precision.coverage().as_str(), "syntax");
        assert_eq!(precision.completion().as_str(), "complete");
        assert!(precision.completion().reason().is_none());
        let partial = Completion::Partial(EvidenceLabel::new("macro").unwrap());
        assert_eq!(partial.as_str(), "partial");
        assert_eq!(partial.reason().unwrap().as_str(), "macro");
        assert!(EvidenceLabel::new("structural-js").is_ok());
        assert_eq!(EvidenceLabel::new(""), Err(InvalidLabel));
        assert_eq!(EvidenceLabel::new("   "), Err(InvalidLabel));
        assert_eq!(EvidenceLabel::new("bad\nline"), Err(InvalidLabel));
        let full = "x".repeat(MAX_LABEL_BYTES);
        assert_eq!(EvidenceLabel::new(full.clone()).unwrap().as_str(), full);
        assert_eq!(
            EvidenceLabel::new("x".repeat(MAX_LABEL_BYTES + 1)),
            Err(InvalidLabel)
        );
    }
}
