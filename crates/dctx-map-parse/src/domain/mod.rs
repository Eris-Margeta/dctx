// SPDX-License-Identifier: Apache-2.0
//! Unresolved per-file facts and parser failures.
//! Implements SPEC-MAP-MODE sections 5 and 6.2 without IO.
//! Uses model nodes and edges; consumed by parser adapters and resolution.

mod facts;
mod identity;

pub use facts::{Language, ParseError, ParsedFile, RawReference};
pub use identity::body_fingerprint;
