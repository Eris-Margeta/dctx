// SPDX-License-Identifier: Apache-2.0
//! Markdown structure facts and the bounds the extractor enforces.
//! Implements the facts shape consumed by dctx-map-parse (SPEC-MAP-MODE 5.1–5.4, 6.2).
//! Pure data, no IO; produced by the lexer, read by parser adapters and tests.
mod document;
mod limits;

pub use document::{
    CodeBlock, CodeBlockKind, Document, Heading, HeadingStyle, Reference, ReferenceKind, Span,
};
pub use limits::{LexError, Limits};
