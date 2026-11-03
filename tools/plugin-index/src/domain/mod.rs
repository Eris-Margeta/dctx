// SPDX-License-Identifier: Apache-2.0
//! Pure index rules: strict JSON, schema, display text, and signature bytes.
//! Implements PLUGINS.md §5.3 / §7.2 PL-X and plugin-api distribution.md.
//! Depends on `dctx-ed25519` and `dctx-hash`. The command layer calls this module.
//! No file, network, or signing IO lives here.

mod index;
mod json;
mod signature;
mod text;

pub use index::{Index, Plugin, Release, validate_index};
pub use signature::{
    PublisherPins, check_signed_index, decode_index_key, fingerprint_hex, index_message,
    verify_index_signature, INDEX_DOMAIN,
};
