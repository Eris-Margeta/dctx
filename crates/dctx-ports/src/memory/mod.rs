// SPDX-License-Identifier: Apache-2.0
//! In-memory port implementations: ephemeral adapters and spies for engine tests.
//! Implements every port of this crate without IO; the source and stores log what they were asked.
//! Depends only on the port traits and dctx-core. Used by dctx-pack's tests and by hosts that
//! explicitly choose ephemeral storage (never a hidden fallback).

pub mod source;
pub mod store;
pub mod time;

pub use source::{MemorySource, SourceCall};
pub use store::{MemoryCache, MemoryFactStore};
pub use time::{CancelAfter, CancelFlag, FixedClock};
