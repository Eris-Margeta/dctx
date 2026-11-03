// SPDX-License-Identifier: Apache-2.0
//! Pack facts bounded context: source entries, read grants, content records and snapshots.
//! Implements the pack data of ARCHITECTURE-PROPOSAL §2 ("domain functions take facts and return facts").
//! Depends on [`crate::ids`], [`crate::datatypes`], [`crate::domain`] and `dctx-hash`; pure.
//! Used by the legacy content reader, dctx-ports and dctx-pack.

pub mod content;
pub mod entry;
pub mod snapshot;

pub use content::{apply_content, mark_subtree_ignored};
pub use entry::{EntryMeta, EntryStamp, ReadGrant, SourceEntry};
pub use snapshot::{snapshot_digest, ContentDigests, Diagnostic, PackSnapshot};
