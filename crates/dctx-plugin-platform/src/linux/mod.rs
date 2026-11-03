// SPDX-License-Identifier: Apache-2.0
//! Linux security building blocks for PL-A, not a complete native launch provider.
//! Landlock/seccomp entry is only for already-loaded trusted code in a disposable process.
//! Handle-relative opens and sealed snapshots serve brokers after host authorization.

mod binding;
mod confinement;
mod filter;
mod snapshot;

pub use binding::{ObjectIdentity, RootHandle};
pub use confinement::{RestrictedEntry, restrict_filesystem, run_restricted};
pub use snapshot::{MAX_SNAPSHOT_BYTES, SealedSnapshot};
