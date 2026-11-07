// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
//! Metadata-only catalog prototype. Unix path bytes are preserved verbatim.
//! The format is experimental, little endian, and limited to a 4 GiB record section.

mod format;
mod name_index;
mod walk;

pub use format::{Catalog, Entry, Metadata, Tier, write_catalog, write_snapshot};
pub use walk::{Snapshot, walk};

/// Basename-token matching mode; ASCII case insensitive, punctuation separates tokens.
#[derive(Clone, Copy, Debug)]
pub enum QueryMode {
    /// Whole token equality.
    Exact,
    /// Start of a token.
    Prefix,
    /// Contiguous bytes within a token.
    Substring,
}
