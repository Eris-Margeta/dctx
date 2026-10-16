//! Cache lifecycle adapter: quotas, leases, expiry and bounded startup maintenance.
//! Implements ARCHITECTURE-PROPOSAL §4 and §7 row DX-C4, plus CACHE-v1 and the DX-A2 model.
//! Depends on dctx-core and dctx-ports. The facade calls startup, status, gc and purge on [`Lifecycle`].
//! No daemon and no directory walk. Expiry uses an injected clock, manifests, leases and file metadata.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod error;

pub mod adapters;
pub mod domain;
pub mod ports;

pub use domain::*;
pub use error::CacheError;
