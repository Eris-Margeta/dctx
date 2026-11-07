// SPDX-License-Identifier: Apache-2.0
//! Persistent graph facts, pack snapshots and a content-addressed comparison cache.
//! Implements SPEC-MAP-MODE sections 6.4, 7 and 8, and ARCHITECTURE-PROPOSAL §3 option B (DX-C2).
//! Depends on map-model, dctx-hash and the `FactStore` port. Used by indexing, query and pack rollback.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

pub mod adapters;
pub mod domain;
pub mod ports;

pub use adapters::SqliteStore;
/// Frozen pack snapshot port. [`SqliteStore`] implements it.
pub use dctx_ports::FactStore;
pub use domain::*;
pub use ports::MapStore;
