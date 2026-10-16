//! SQLite comparison bridge: FactStore snapshots and content-addressed blob facts.
//! Implements ARCHITECTURE-PROPOSAL §3 option B and §7 row DX-C2.
//! Depends on the v1 map connection. [`super::SqliteStore`] publishes through it.
//! Cleaning uses the injected clock. This module does not read git.

mod facts;
mod ledger;
mod pack;
mod schema;

#[cfg(test)]
mod fact_tests;
#[cfg(test)]
mod pack_tests;

pub(super) use facts::{release_root, retain, retained};
pub(super) use ledger::{cache_cap, cached_bytes, maintain, parse_count, set_cap};
pub(super) use pack::{load, publish};
pub(super) use schema::ensure;

use crate::domain::StoreError;
use dctx_core::error::DctxError;

pub(super) fn to_dctx(error: StoreError) -> DctxError {
    match error {
        StoreError::CacheLimit { requested, available } => {
            DctxError::CacheLimit { requested, available }
        }
        StoreError::Corrupt(reason) => DctxError::CorruptIndex { reason },
        StoreError::InvalidKey(reason) => DctxError::InvalidRequest { reason },
        StoreError::FactConflict => DctxError::InvalidRequest { reason: "fact payload conflict" },
        StoreError::RebuildRequired(version) => DctxError::UnsupportedVersion {
            found: u32::try_from(version).unwrap_or(u32::MAX),
        },
        other => DctxError::io(other),
    }
}
