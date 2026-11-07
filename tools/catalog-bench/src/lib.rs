// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
//! Metadata-only catalog prototype. Unix path bytes are preserved verbatim.
//! The format is experimental, little endian, and limited to a 4 GiB record section.

mod format;
mod walk;

pub use format::{Catalog, Entry, Metadata, Tier, write_catalog};
pub use walk::{Snapshot, walk};
