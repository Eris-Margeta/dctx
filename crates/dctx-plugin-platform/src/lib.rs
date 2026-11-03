// SPDX-License-Identifier: Apache-2.0
//! Native plugin admission and platform security primitives (PLUGINS §4.2, PL-A).
//! The Linux primitives enforce restrictions but do not constitute a qualified launcher.
//! All general native launches fail closed until one-shot exec and lifecycle gates are met.
//! Depends on approved safe adapters; used by facade brokers and platform qualification.
//! **Unstable**: this crate has no API or ABI stability promise in dctx v1 and may change in any release.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod admission;
mod environment;
mod error;
mod path;

#[cfg(target_os = "linux")]
pub mod linux;
pub mod macos;

pub use admission::{ConfinementProvider, LaunchKind, NativeProvider, Support, Target};
pub use environment::NoSecretEnvironment;
pub use error::{PlatformError, Unsupported};
pub use path::RelativePath;
