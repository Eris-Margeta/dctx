//! IO-free extraction vocabulary: facts, precision labels, limits and refusals.
//! Implements PARSERS-v1 (`ParseResult`, completion, limits) for Python 3.
//! Used by the lexer, the structural walker and callers of the public API.
mod facts;
mod limits;
mod text;

pub use crate::error::LexError;
pub use facts::{
    Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexedFile, Precision, RawReference,
};
pub use limits::Limits;
pub use text::body_fingerprint;
pub(crate) use text::{collapse, comment_summary, decode_doc, sentence};
