// SPDX-License-Identifier: Apache-2.0
//! Pure distribution domain: names, digests, manifests, signatures, trust, bundles, origins, consent,
//! the discovery index and the registry state machine. No IO; see PLUGINS.md §5 and Install.tla.
//! Depends on dctx-hash, dctx-ed25519 and the protocol's strict JSON. Used by `service`.

pub mod admission;
pub mod bundle;
pub mod consent;
pub mod digest;
pub(crate) mod fields;
pub mod index;
pub mod manifest;
pub mod names;
pub mod origin;
pub mod registry;
pub mod registry_codec;
pub mod signature;
pub mod sums;
pub mod trust;
