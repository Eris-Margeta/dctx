//! Index and query orchestration over the landed map crates.
//! Implements SPEC-MAP-MODE sections 4, 6 and 7.
//! Connects filesystem sources and SQLite to pure request rules.
mod cache;
mod database;
mod index;
mod inventory;
mod output;
mod parsing;
mod publication;
mod query;
mod reading;
mod rebuild;
mod refresh;
mod resolving;
mod revision;
mod source;
mod traversal;
mod view;
pub use index::{Index, IndexReport};
pub use output::{render_answer, render_answer_at};
pub use source::FileSource;
