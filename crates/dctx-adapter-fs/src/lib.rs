// SPDX-License-Identifier: Apache-2.0
//! Confined filesystem Source adapter.
//! Implements ARCHITECTURE-PROPOSAL §7 DX-B3 as adapted by decision 078:
//! selection before content, plain ignore rules, and [`dctx_hash`] digests.
//! Depends on `dctx-hash` and `std`. Engines call [`FsSource`]; this crate never runs git.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod adapters;
pub mod domain;
pub mod ports;

pub use adapters::{FsSource, SourceError, SourceOptions, SpyFs, StdFs};
pub use dctx_hash::ContentDigest;
pub use domain::{
    AuthorizedFile, FileBody, FileHandlingDecision, FileStamp, FreshnessMode, RecordedIdentity,
    SelectionPolicy, SourceFile, StaleReason, Validation,
};
pub use ports::{DirEntry, EntryMeta, FsKind, FsOp, SourceFs};
