//! Pure scan domain.
//! Implements SPEC-MAP-MODE section 6.1 and ADR-014/015b.
//! Uses standard-library types; consumed by scan clients.

/// File handling permissions.
pub mod decision;
/// Ignore engine and pattern rules.
pub mod ignore;
pub use decision::{FileHandlingDecision, ItemContext};
