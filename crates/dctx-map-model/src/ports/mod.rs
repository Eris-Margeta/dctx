//! Boundary policy for the pure model crate.
//! SPEC-MAP-MODE section 10 and ARCHITECTURE-PROPOSAL §2 assign IO to adapters.
//! This crate has no filesystem, clock, process or network port.
//! Byte spans, precision and provenance are domain facts, not storage handles.
