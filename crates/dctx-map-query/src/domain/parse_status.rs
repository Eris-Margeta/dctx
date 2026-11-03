// SPDX-License-Identifier: Apache-2.0
//! Durable parse outcomes for the query index's content cache.
//! Implements SPEC-MAP-MODE section 7: syntax failures are content facts,
//! while interrupted work remains retryable. Used by refresh and publication.
use serde::{Deserialize, Serialize};

/// Revision of extraction and recovery rules; bump when either changes.
pub(crate) const PARSER_REVISION: u32 = 1;

/// A bounded diagnostic from the syntax adapter, in original source coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseDiagnostic {
    /// The parser's error message.
    pub error: String,
    /// One-based inclusive line span (the strict parser reports one line).
    pub lines: [u32; 2],
    /// Bounded excerpt of the syntax the adapter could not read.
    pub syntax: String,
}

/// Content-specific extraction outcome, persisted alongside its digest and revision.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ParseStatus {
    /// Complete facts from the current parser.
    #[default]
    Parsed,
    /// Only a file node is available; no previous symbols are retained.
    Unparseable {
        /// Syntax or content failure that prevented extraction.
        diagnostic: ParseDiagnostic,
    },
    /// Partial facts whose summaries explicitly carry a low-precision label.
    Partial {
        /// Syntax omitted by the compatibility adapter.
        diagnostics: Vec<ParseDiagnostic>,
    },
}
