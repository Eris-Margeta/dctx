//! Filesystem adapters for the Source port.
//! Implements DX-B3 confinement, rule loading, and stale races.
//! Depends on [`crate::domain`] and [`crate::ports`]. [`FsSource`] is the public entry.

mod error;
mod link;
mod locate;
mod path;
mod read;
mod rules;
mod source;
mod spy;
mod std_fs;
mod walk;

pub use error::SourceError;
pub use source::{FsSource, SourceOptions};
pub use spy::SpyFs;
pub use std_fs::StdFs;
