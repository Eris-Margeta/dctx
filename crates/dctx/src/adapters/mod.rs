//! Adapters for the facade ports. Memory doubles stay in `dctx-ports`.
//! Implements the filesystem source used by hosts that already chose a root.
//! Depends on [`crate::ports`]. [`crate::Session::builder`] takes the adapter as a `Source`.
//! This adapter never writes, never reads a home directory and never follows a symlink.

#[cfg(feature = "fs")]
mod fs;

#[cfg(feature = "fs")]
pub use fs::FsSource;
