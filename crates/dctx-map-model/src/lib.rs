//! Pure graph facts and bounded answers for dctx map mode.
//! Implements SPEC-MAP-MODE sections 4 and 5 without filesystem access.
//! Depends on serde and serde_json; used by indexers and query adapters.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod adapters;
pub mod domain;
pub mod ports;
pub use domain::*;
