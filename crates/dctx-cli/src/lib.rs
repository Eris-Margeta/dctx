// SPDX-License-Identifier: Apache-2.0
//! CLI application operations for explicit pack, map, plugin, template and cache commands.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

pub mod cache;
mod clipboard;
pub mod exit;
pub mod help;
pub mod host;
pub mod languages;
pub mod pack;
pub mod template;
pub mod tui;

pub use cache::run_in;
