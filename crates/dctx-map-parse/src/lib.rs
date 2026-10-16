//! Per-file syntax facts for dctx map mode.
//! Implements SPEC-MAP-MODE sections 5.1–5.3 and 6.2.
//! Uses map-model and tree-sitter; consumed by the resolver and indexer.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod adapters;
pub mod domain;
pub mod ports;

pub use adapters::TreeSitterParser;
pub use domain::{Language, ParseError, ParsedFile, RawReference, body_fingerprint};
pub use ports::Parser;
