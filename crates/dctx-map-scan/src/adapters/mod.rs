//! Filesystem adapters for scan rules and content.
//! Implements SPEC-MAP-MODE section 6.1 and ADR-014/015b.
//! Connects IO ports to the pure ignore engine.

/// Content access gated by domain decisions.
pub mod content;
/// Layered rule loading and typed failures.
pub mod rules;
