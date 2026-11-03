// SPDX-License-Identifier: Apache-2.0
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
    ///
    /// # Errors
    /// Returns `CorruptIndex` for an invalid stored snapshot, or an adapter
    /// error when storage cannot be read.
    fn load(&self, view: &ViewKey) -> Result<Option<PackSnapshot>, DctxError>;

    /// Atomically replaces the latest snapshot of `view`.
    ///
    /// # Errors
    /// Returns an adapter error if storage cannot publish the snapshot;
    /// the previous published snapshot remains visible.
    fn publish(&mut self, view: &ViewKey, snapshot: &PackSnapshot) -> Result<(), DctxError>;
}
