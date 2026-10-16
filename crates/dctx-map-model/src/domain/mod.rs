//! IO-free graph vocabulary and answer contracts.
//! Implements SPEC-MAP-MODE sections 3 through 5.
//! Shared by parsers, resolvers, stores and renderers.
mod answer;
mod graph;
mod ids;
mod sha256;
pub use answer::*;
pub use graph::*;
pub use ids::*;
