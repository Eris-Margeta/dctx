// SPDX-License-Identifier: Apache-2.0
//! Filesystem port for the confined Source adapter.
//! Implements ADR-014 by splitting metadata, policy text, and content reads.
//! Depends on `std`. [`crate::adapters`] implements [`SourceFs`].

mod fs;

pub use fs::{DirEntry, EntryMeta, FsKind, FsOp, SourceFs};
