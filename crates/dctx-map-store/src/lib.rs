//! Persistent graph facts and lexical search for map mode.
//! Implements spec sections 6.4, 7 and 8 using SQLite.
//! Depends on map-model; used by indexing and query adapters.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod adapters;
pub mod domain;
pub mod ports;

pub use adapters::SqliteStore;
pub use domain::*;
pub use ports::MapStore;
