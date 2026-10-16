//! Structural Rust extractor for dctx map mode, built on `syn` (decision 077).
//! Implements docs/design/contracts/PARSERS-v1.md (Rust row) and PRODUCT-CONTRACTS C05.
//! Uses syn/proc-macro2 for valid files and a conservative line scan for the rest;
//! map indexers consume the file-local facts. Never depends on tree-sitter.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod adapters;
pub mod domain;

pub use adapters::extract;
pub use domain::{
    Completion, Confidence, Coverage, Declaration, DeclarationKind, Extraction, Implementation,
    ImportKind, Limits, Precision, RawImport, RawReference, ReferenceKind, Refusal, SourceSpan,
    UnresolvedReason,
};

/// Parser identity recorded in fact provenance.
pub const PARSER_ID: &str = "dctx-lex-rust/syn-3.0.9";

/// Bumped whenever extraction output for the same bytes can change.
pub const PARSER_REVISION: u32 = 1;
