// SPDX-License-Identifier: Apache-2.0
//! First-party SHA-256 content digests for dctx.
//! Implements the strict-profile content identity in ARCHITECTURE-PROPOSAL (`ObjectKey`) and decision 078.
//! Depends on no production crates. Callers use [`Sha256`] and [`ContentDigest`].
//! Streaming ([`Sha256::new`], [`Sha256::update`], [`Sha256::finalize`]) and [`Sha256::digest`] share one constant-size state.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod algorithm;
mod compress;
mod error;
mod sha256;

#[cfg(test)]
mod differential;

pub use algorithm::{ContentDigest, IDENTITY_VERSION, IdentityAlgorithm};
pub use error::HashError;
pub use sha256::Sha256;
