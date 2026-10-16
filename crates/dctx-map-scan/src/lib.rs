//! Model-independent scan privacy rules.
//! Implements SPEC-MAP-MODE sections 6.1 and 13a.
//! Uses the standard library and serves map indexers.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Filesystem rule loaders.
pub mod adapters;
/// Pure scan rules and privacy permissions.
pub mod domain;
/// Rule loading boundaries.
pub mod ports;

pub use domain::ignore::IgnoreEngine;
pub use domain::{FileHandlingDecision, ItemContext};
