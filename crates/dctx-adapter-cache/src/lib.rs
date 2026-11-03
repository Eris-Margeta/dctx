// SPDX-License-Identifier: Apache-2.0
//! Cache lifecycle adapter: quotas, leases, expiry, a persisted ledger and bounded auto-clean.
//! Implements ARCHITECTURE-PROPOSAL §4 and §7 row DX-C4, plus CACHE-v1 and the DX-A2 model.
//! Depends on dctx-core, dctx-hash and dctx-ports. The facade calls startup, `auto_clean`, status, gc and purge.
//! No daemon and no source-tree walk. Object unlink stays inside the cache root and does not follow symlinks.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod error;

pub mod adapters;
pub mod domain;
pub mod ports;

pub use domain::*;
pub use error::CacheError;
