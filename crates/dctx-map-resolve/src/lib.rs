//! Pure symbol resolution and move detection for dctx map mode.
//! Implements SPEC-MAP-MODE sections 5.2, 5.3, 6.2 and 6.3.
//! Exchanges model facts with the indexer; performs no IO.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod adapters;
pub mod domain;
pub mod ports;

pub use domain::*;
