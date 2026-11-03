// SPDX-License-Identifier: Apache-2.0
//! Std-only codec and typed records for the frozen dctx plugin API v1 wire contract (PLUGINS.md §7.1 PL-P).
//! Implements docs/design/plugin-api/{wire.md, semantics.md, *.schema.json}: u32 LE frames, strict bounded
//! JSON, canonical encoding, closed records, version negotiation and a pure session tracker.
//! Depends on std only. Used by the plugin host, platform adapters, the Git plugin and distribution.
//! Domain records, negotiation and session tracking do no IO; `stream` is the only `std::io` adapter.
//! **Unstable**: this crate has no API or ABI stability promise in dctx v1 and may change in any release.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod frame;
pub mod json;
pub mod negotiate;
pub mod records;
pub mod refusal;
pub mod session;
pub mod stream;

pub use frame::{BOOTSTRAP_FRAME_CAP, FrameDecoder, MAX_FRAME_CAP, encode_frame};
pub use json::decode::{DecodeLimits, decode_object};
pub use json::encode::to_canonical;
pub use json::value::Json;
pub use negotiate::{Negotiated, SUPPORTED_VERSIONS, WorkerProfile, accept_answer, answer_hello, select_version};
pub use records::envelope::{Outcome, PendingResult, SessionBody, SessionMessage, WireMessage};
pub use records::failure::{EffectState, ErrorCode, Phase, WireError};
pub use refusal::{Predicate, Refusal, Stage};
pub use session::{Accepted, Session, SessionState};
pub use stream::{FrameReader, StreamError, write_frame};
