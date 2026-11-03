// SPDX-License-Identifier: Apache-2.0
//! Text answers and the optional JSON v1 conversion.
//! Implements SPEC-MAP-MODE sections 4.1, 4.2 and the id aliases in section 5.2.
//! JSON exists only behind the `wire-json` feature. Callers own transport and persistence.
mod render;
#[cfg(feature = "wire-json")]
mod wire_json;
pub use render::*;
