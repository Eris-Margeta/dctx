// SPDX-License-Identifier: Apache-2.0
//! Strict JSON layer: value tree, bounded decoder, canonical encoder and closed-object helpers.
//! Implements plugin-api wire.md §Bytes and semantics.md S-BOUNDS before any record is constructed.
//! Depends on refusal; used by records, frame callers and fixture tests.

pub mod decode;
pub mod encode;
pub(crate) mod fields;
pub mod value;
