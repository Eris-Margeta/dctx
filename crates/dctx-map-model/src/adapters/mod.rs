//! Pure serialization adapters for the map answer contract.
//! Implements SPEC-MAP-MODE sections 4.1, 4.2 and id aliases in 5.2.
//! No adapter performs IO; callers own transport and persistence.
mod render;
pub use render::*;
