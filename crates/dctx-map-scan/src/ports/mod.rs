//! IO boundaries for scan adapters.
//! Implements ADR-014/015b explicit failures and content privacy.
//! Uses standard IO types; pure matching never performs IO.

use std::{io, path::Path};

/// Supplies optional UTF-8 rules; only absence returns `None`.
pub trait RuleSource {
    /// Reads rules, preserving permission and encoding errors.
    fn read_rules(&self, path: &Path) -> io::Result<Option<String>>;
}

/// Supplies content after the privacy decision authorizes access.
pub trait ContentSource {
    /// Opens and reads the authorized path.
    fn read_content(&self, path: &Path) -> io::Result<Vec<u8>>;
}

/// Traversal filesystem boundary.
pub mod fs;
