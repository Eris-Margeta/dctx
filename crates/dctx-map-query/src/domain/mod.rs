//! Pure requests, ranking and answer selection for the five map verbs.
//! Implements SPEC-MAP-MODE sections 4 and 6.4.
//! Uses graph model values and is called by the query adapter.
pub(crate) mod bridge;
pub(crate) mod cached;
mod error;
mod ranking;
mod request;
pub use error::{Error, Result};
pub(crate) use ranking::{expression, rank};
pub use request::{Direction, Request};
pub(crate) use request::{inside, quote, validate_path};
