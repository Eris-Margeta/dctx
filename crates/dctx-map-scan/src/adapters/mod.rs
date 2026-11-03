// SPDX-License-Identifier: Apache-2.0
//! Filesystem adapters for scan rules and content.
//! Implements SPEC-MAP-MODE section 6.1 and ADR-014/015b.
//! Connects IO ports to the pure ignore engine.

/// Content access gated by domain decisions.
pub mod content;
/// Standard filesystem adapter.
pub mod fs;
/// Layered rule loading and typed failures.
pub mod rules;
/// Observable filesystem decorator.
pub mod spy;
/// Privacy-preserving traversal.
pub mod walker;
