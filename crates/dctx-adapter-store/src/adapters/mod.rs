// SPDX-License-Identifier: Apache-2.0
//! Store adapters: in-memory crash disk and std filesystem.
//! Implements DX-C3 publication over the [`crate::ports::StoreFs`] port.
//! Depends on [`crate::domain`] rules. Callers use [`SegmentStore`].
//! The memory adapter is the crash, disk-full, and Windows-sharing test double.

mod gc;
mod memory;
mod publish;
mod recover;
mod stdfs;
mod store;

pub use memory::MemoryFs;
pub use stdfs::StdFs;
pub use store::SegmentStore;
