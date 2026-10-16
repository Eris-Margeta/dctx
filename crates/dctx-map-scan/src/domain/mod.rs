//! Pure scan domain.
//! Implements SPEC-MAP-MODE section 6.1 and ADR-014/015b.
//! Uses standard-library types; consumed by scan clients.

/// File handling permissions.
pub mod decision;
/// Ignore engine and pattern rules.
pub mod ignore;
pub use decision::{FileHandlingDecision, ItemContext};

/// Binary and SVG classification.
pub mod classifier;
#[cfg(test)]
mod omit_parity_0;
#[cfg(test)]
mod omit_parity_1;
#[cfg(test)]
mod omit_parity_2;
/// Scan output records and hashes.
pub mod scanned_file;
