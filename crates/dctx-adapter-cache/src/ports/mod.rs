//! Ports this adapter implements and consumes.
//! Implements API-PORTS-v1 `Cache`, `Clock` and `Cancellation` for CACHE-v1.
//! Depends on `dctx-ports`. The facade injects a clock and calls `Lifecycle`.
//! This crate does not discover a home directory and does not open a source tree.

pub use dctx_ports::{Cache, Cancellation, Clock};
