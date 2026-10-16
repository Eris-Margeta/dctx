//! IO-free graph vocabulary, content identity and answer contracts.
//! Implements SPEC-MAP-MODE sections 3 through 5 and ARCHITECTURE-PROPOSAL DX-B1.
//! Depends on `dctx-hash` for algorithm-tagged digests. Shared by every map crate.
mod answer;
mod byte;
mod content_id;
mod graph;
mod ids;
mod precision;
mod provenance;
mod sha256;
pub use answer::*;
pub use byte::*;
pub use content_id::*;
pub use graph::*;
pub use ids::*;
pub use precision::*;
pub use provenance::*;
