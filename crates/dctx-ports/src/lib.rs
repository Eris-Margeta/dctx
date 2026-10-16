//! dctx-ports: the boundary traits engines use instead of IO — Source, FactStore, Cache, Clock,
//! Cancellation — whose data are dctx-core types, plus in-memory implementations and spies.
//! Implements ARCHITECTURE-PROPOSAL §2 (`dctx-ports`) and contracts/API-PORTS-v1.md "Boundary ports".
//! Depends only on dctx-core; imports no engine or adapter. Used by dctx-pack and future adapters.
//! The core has no VCS port (decision 078): freshness comes from `Source` metadata and the own digest.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod cache;
pub mod cancellation;
pub mod clock;
pub mod fact_store;
pub mod memory;
pub mod source;

pub use cache::Cache;
pub use cancellation::Cancellation;
pub use clock::Clock;
pub use fact_store::FactStore;
pub use source::Source;
