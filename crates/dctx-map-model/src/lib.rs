//! IO-free map domain: identities, byte spans, precision, provenance and answers.
//! Implements ARCHITECTURE-PROPOSAL §7 DX-B1 and SPEC-MAP-MODE sections 3 through 5.
//! Production dependency is first-party `dctx-hash` only. Optional `wire-json` adds JSON v1.
//! Parsers, resolvers, stores and query engines share these facts and bring their own IO.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

pub mod adapters;
pub mod domain;
pub mod ports;
pub use domain::*;
