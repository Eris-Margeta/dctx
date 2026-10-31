//! Pure symbol resolution and move detection for dctx map mode.
//! Implements SPEC-MAP-MODE sections 5.2, 5.3, 6.2 and 6.3.
//! DX-B5 adds contextual views: content-keyed facts, per-layout edges,
//! reverse invalidation, and explicit alias and body collisions.
//! Exchanges model facts with the indexer; performs no IO.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

pub mod adapters;
pub mod domain;
pub mod ports;

pub use domain::*;
