//! Local public-index validator for dctx plugins.
//! Implements PLUGINS.md §5.3 and §7.2 row PL-X, and the PL-C index schema.
//! Depends on `dctx-ed25519` for verification and `dctx-hash` for key fingerprints.
//! The `plugin-index` binary and the crate tests call it. It does not publish or sign.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod command;
mod domain;
mod error;

pub use command::execute;
pub use domain::{
    Index, Plugin, PublisherPins, Release, INDEX_DOMAIN, check_signed_index, decode_index_key,
    fingerprint_hex, index_message, validate_index, verify_index_signature,
};
pub use error::IndexError;
