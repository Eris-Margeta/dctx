//! Library surface for cache administration.
//! Implements ARCHITECTURE-PROPOSAL §4. The `dctx` binary wires the same modules.
//! Depends on the cache, help and host modules. Integration tests call [`run_in`].
//! The legacy `dctx .` path stays in the binary and does not auto-clean.
//! Binary-only entry points are unused in this crate, which exists so tests can call [`run_in`].

#![forbid(unsafe_code)]
#![allow(dead_code)]

mod cache;
mod help;
mod host;

pub use cache::run_in;
