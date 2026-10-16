//! Index and query orchestration over the landed map crates.
//! Implements SPEC-MAP-MODE sections 4, 6 and 7.
//! Connects filesystem sources and SQLite to pure request rules.
mod cache;
mod index;
mod output;
mod parsing;
mod query;
mod reading;
mod revision;
mod resolving;
mod source;
mod traversal;
pub use index::{Index, IndexReport};
pub use output::{render_answer, render_answer_at};
pub use source::FileSource;
