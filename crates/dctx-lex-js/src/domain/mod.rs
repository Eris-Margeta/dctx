// SPDX-License-Identifier: Apache-2.0
//! IO-free extraction vocabulary: facts, precision labels, limits and refusals.
//! Implements PARSERS-v1 (`ParseResult`, completion, limits) for JavaScript and TypeScript.
//! Used by the lexer, the structural walker and callers of the public API.
mod facts;
mod limits;
mod text;

pub use facts::{
    Completion, Diagnostic, DiagnosticReason, Dialect, Labeled, LexedFile, Precision,
    RawReference,
};
pub use limits::{LexError, Limits};
pub use text::body_fingerprint;
pub(crate) use text::{compact, first_comment, jsdoc};
