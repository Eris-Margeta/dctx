//! Ports this adapter implements and consumes.
//! Implements API-PORTS-v1 `Cache`, `Clock` and `Cancellation`, plus the ledger and object-store ports.
//! Depends on `dctx-ports` and the domain ledger. Filesystem adapters in `crate::adapters` implement the new ports.
//! This crate does not discover a home directory and does not open a source tree.

use crate::domain::{Lifecycle, ObjectId, Quota};
use crate::error::CacheError;
use std::collections::BTreeSet;

pub use dctx_ports::{Cache, Cancellation, Clock};

/// A file under the cache root that the ledger does not own. Load never deletes these.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OrphanFile {
    /// Path relative to the cache root, with `/` separators.
    pub relative: String,
}

/// Result of opening a ledger file. `fault` set means `lifecycle` is empty and nothing was deleted.
#[derive(Debug, Clone)]
pub struct LoadedLedger {
    /// Restored ledger, or an empty one when the file is missing, corrupt, or an unknown version.
    pub lifecycle: Lifecycle,
    /// Corruption or an unknown version. Missing files leave this empty.
    pub fault: Option<CacheError>,
    /// Unowned names found under the cache root.
    pub orphans: Vec<OrphanFile>,
}

/// Durable image of a [`Lifecycle`]. The filesystem adapter writes it atomically.
pub trait LedgerStore {
    /// Reads the committed ledger. Corrupt and unknown versions come back as `fault` plus an empty ledger.
    fn load(&self, empty_quota: Quota) -> Result<LoadedLedger, CacheError>;

    /// Replaces the committed ledger. A crash before the final rename keeps the previous image.
    fn store(&self, ledger: &Lifecycle) -> Result<(), CacheError>;
}

/// Reclaimed object bytes. Implementations must not follow a symlink out of the cache root.
pub trait ObjectStore {
    /// Unlinks one reclaimed object. A missing file is success. A symlink is a refusal.
    fn unlink_object(&self, id: ObjectId) -> Result<(), CacheError>;

    /// Names on disk that `known` does not include. This does not delete them.
    fn orphans(&self, known: &BTreeSet<ObjectId>) -> Result<Vec<OrphanFile>, CacheError>;
}
