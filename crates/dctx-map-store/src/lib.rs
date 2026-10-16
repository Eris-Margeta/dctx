//! Persistent graph facts, pack snapshots and a content-addressed comparison cache.
//! Implements SPEC-MAP-MODE sections 6.4, 7 and 8, and ARCHITECTURE-PROPOSAL §3 option B (DX-C2).
//! Depends on map-model, dctx-hash and the FactStore port. Used by indexing, query and pack rollback.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod adapters;
pub mod domain;
pub mod ports;

pub use adapters::SqliteStore;
pub use domain::*;
/// Frozen pack snapshot port. [`SqliteStore`] implements it.
pub use dctx_ports::FactStore;
pub use ports::MapStore;
