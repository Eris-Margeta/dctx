//! Pure domain of the Rust extractor: facts, labels, limits and the line scan.
//! Implements PARSERS-v1 "Common interface" and "Confidence is separate from coverage".
//! No IO and no syn types; the syn adapter and the line scan both produce these facts.
mod facts;
mod limits;
mod line_head;
pub(crate) mod line_scan;
mod mask;
pub(crate) mod sink;
pub(crate) mod text;

pub use facts::{
    Completion, Confidence, Coverage, Declaration, DeclarationKind, Extraction, Implementation,
    ImportKind, Precision, RawImport, RawReference, ReferenceKind, SourceSpan, UnresolvedReason,
};
pub use limits::{Limits, Refusal};
