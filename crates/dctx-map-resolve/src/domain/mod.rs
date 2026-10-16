//! Pure facts, symbol tables, reference resolution and moves.
//! Implements SPEC-MAP-MODE sections 5.2, 5.3 and 6.3.
//! DX-B5 view resolution shares content-keyed facts across layouts.
//! Uses public model types and serves the indexing pipeline.
mod collision;
mod fact_key;
mod facts;
mod lookup;
mod moves;
mod place;
mod resolve;
mod reverse;
mod shared;
mod symbols;
mod view;

pub use collision::*;
pub use fact_key::*;
pub use facts::*;
pub use moves::*;
pub use place::*;
pub use resolve::*;
pub use reverse::*;
pub use shared::*;
pub use symbols::SymbolTable;
pub use view::*;

#[cfg(test)]
pub(crate) mod fixtures;

#[cfg(test)]
mod binding_tests;

#[cfg(test)]
mod properties;
