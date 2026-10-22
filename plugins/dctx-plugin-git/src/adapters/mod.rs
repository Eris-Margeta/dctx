//! Adapters: the framed stdio worker. The only IO is the inherited protocol reader/writer pair;
//! there is no filesystem, process, network or environment access here (decision 079/decision 080, PLUGINS.md §4.2).
//! Depends on dctx-plugin-protocol's stream codec and session tracker; used by main.rs and the tests.

pub mod link;
pub mod wire_broker;
pub mod worker;

use std::fmt;
use std::io::ErrorKind;

use dctx_plugin_protocol::{ErrorCode, Predicate, Refusal, Stage};

/// Why a worker session ended abnormally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerError {
    /// The protocol stream failed.
    Io(ErrorKind),
    /// The host violated the protocol (frame, decode, schema or session rule).
    Protocol(Refusal),
    /// The stream closed while an answer was required.
    Closed,
    /// The host's hello offered no usable version or a required API was missing.
    Unsupported(ErrorCode),
    /// Initialize was refused; the session is over.
    InitializeRefused(ErrorCode),
    /// The worker produced a message its own tracker refused (a defect, reported not hidden).
    Internal(Refusal),
}

impl WorkerError {
    /// Stable safe reason name.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::Io(_) => "stream-failed",
            Self::Protocol(refusal) | Self::Internal(refusal) => refusal.reason,
            Self::Closed => "stream-closed",
            Self::Unsupported(_) => "unsupported",
            Self::InitializeRefused(_) => "initialize-refused",
        }
    }
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(kind) => write!(f, "protocol stream failed: {kind}"),
            Self::Protocol(refusal) => write!(f, "host protocol violation: {refusal}"),
            Self::Closed => write!(f, "protocol stream closed"),
            Self::Unsupported(code) => write!(f, "negotiation refused: {}", code.as_str()),
            Self::InitializeRefused(code) => write!(f, "initialize refused: {}", code.as_str()),
            Self::Internal(refusal) => write!(f, "worker message refused by its own tracker: {refusal}"),
        }
    }
}

impl std::error::Error for WorkerError {}

/// A session-layer protocol violation detected by the worker.
pub(crate) const fn violation(reason: &'static str) -> Refusal {
    Refusal::new(ErrorCode::InvalidMessage, Stage::Session, Predicate::WState, reason)
}
