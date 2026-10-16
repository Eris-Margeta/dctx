//! Incremental indexing and bounded traversal for dctx map mode.
//! Implements SPEC-MAP-MODE sections 4, 6.4 and 7.
//! Wires the scan, parse, resolve and store public APIs for library and CLI users.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod adapters;
pub mod domain;
pub mod ports;

pub use adapters::{FileSource, Index, IndexReport};
pub use adapters::{render_answer, render_answer_at};
pub use dctx_map_model::adapters::Format;
pub use dctx_map_model::{Answer, AnswerBudget, EdgeKind, Record, Verb};
pub use domain::{Direction, Error, Request, Result};
