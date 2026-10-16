//! Pure facts, symbol tables, reference resolution and moves.
//! Implements SPEC-MAP-MODE sections 5.2, 5.3 and 6.3.
//! Uses public model types and serves the indexing pipeline.
mod facts;
mod lookup;
mod moves;
mod resolve;
mod symbols;

pub use facts::*;
pub use moves::*;
pub use resolve::*;
pub use symbols::SymbolTable;

#[cfg(test)]
pub(crate) mod fixtures;

#[cfg(test)]
mod binding_tests;
