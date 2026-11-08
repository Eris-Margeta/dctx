// SPDX-License-Identifier: Apache-2.0
//! dctx-pack: the pack snapshot and diff engine over injected ports, with no concrete IO.
//! Implements ARCHITECTURE-PROPOSAL §2 (`dctx-pack`) and §7 DX-B0: it reproduces the legacy
//! walker/content-reader tree through `Source`, diffs and versions it, and publishes via `FactStore`.
//! Depends on dctx-core, dctx-ports and dctx-hash (decision 077 content identity); never on git (decision 078).
//! Used by the future `dctx` facade; dctx-cli keeps the legacy path during the bridge period.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod diff;
pub mod engine;
pub mod legacy_codec;
pub mod legacy_decode;
pub mod legacy_encode;
pub mod snapshot;

pub use diff::{diff_against, VersionedDiff};
pub use engine::{pack, PackPorts, PackReport, PackRequest};
pub use legacy_codec::{
    LegacyBodies, LegacyDocument, LegacyError, LegacyHeader, LegacyLimits, LegacyNode,
};
pub use legacy_decode::decode_legacy;
pub use legacy_encode::{encode_legacy, encode_legacy_to};
pub use snapshot::{scan, scan_spooled, Scan, ScanOptions};
