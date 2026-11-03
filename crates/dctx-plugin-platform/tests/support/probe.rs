// SPDX-License-Identifier: Apache-2.0
//! Trusted native subprocess fixture for the PL-A adversary suite.
//! Never dispatches a plugin: hard-coded probes run only after the restriction boundary.
//! Built as a test utility so integration tests use an actual single-threaded entry.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

#[cfg(target_os = "linux")]
mod probe_linux;

fn main() {
    #[cfg(target_os = "linux")]
    if let Err(error) = probe_linux::run() {
        eprintln!("{error}");
        std::process::exit(125);
    }
    #[cfg(not(target_os = "linux"))]
    std::process::exit(125);
}
