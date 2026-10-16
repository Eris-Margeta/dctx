//! Port adapters over the in-memory lifecycle ledger.
//! Implements `dctx_ports::Cache` for [`crate::Lifecycle`] (API-PORTS-v1 reserve/commit/abort).
//! Depends on the domain ledger and `dctx-ports`. Engines settle reservations through this impl.
//! There is no filesystem adapter here: callers pass discovered records in.

mod cache;

pub use dctx_ports::Cache;
