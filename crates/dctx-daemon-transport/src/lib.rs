// SPDX-License-Identifier: Apache-2.0
//! Local-only, user-owned IPC. No TCP listener is used on any platform.
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(unix)]
pub use unix::*;
#[cfg(windows)]
pub use windows::*;
