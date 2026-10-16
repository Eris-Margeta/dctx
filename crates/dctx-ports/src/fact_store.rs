//! `FactStore` port: load and publish the latest pack snapshot of a view.
//! Implements the pack subset of contracts/API-PORTS-v1.md `FactStore` (pin/begin/commit): one
//! publish is the commit point; a failed publish leaves the previous snapshot visible.
//! Depends on dctx-core `PackSnapshot` and `ViewKey`. Used by dctx-pack; adapters own storage.

use dctx_core::error::DctxError;
use dctx_core::facts::PackSnapshot;
use dctx_core::ids::ViewKey;

/// Durable (or ephemeral, if the caller chose so) home of pack snapshots.
pub trait FactStore {
    /// The latest published snapshot of `view`, if any. Corrupt data is `CorruptIndex`.
    fn load(&self, view: &ViewKey) -> Result<Option<PackSnapshot>, DctxError>;

    /// Atomically replaces the latest snapshot of `view`.
    fn publish(&mut self, view: &ViewKey, snapshot: &PackSnapshot) -> Result<(), DctxError>;
}
