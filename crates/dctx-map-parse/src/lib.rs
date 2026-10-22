//! Per-file syntax facts for dctx map mode.
//! Implements SPEC-MAP-MODE sections 5.1–5.3 and 6.2 and PARSERS-v1.
//! Default parser is `PureParser` over the dctx-lex crates. The syntax-tree-sitter
//! feature compiles `TreeSitterParser` as the differential oracle. No C in the default build.
//! Consumed by the resolver and the query indexer.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod adapters;
pub mod domain;
pub mod ports;

pub use adapters::PureParser;
#[cfg(feature = "syntax-tree-sitter")]
pub use adapters::TreeSitterParser;
#[cfg(not(feature = "syntax-tree-sitter"))]
pub use adapters::PureParser as TreeSitterParser;
pub use domain::{Language, ParseError, ParsedFile, RawReference, body_fingerprint};
pub use ports::Parser;
