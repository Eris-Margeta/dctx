//! dctx plugin host domain and brokers (PLUGINS.md §7.2 row PL-H).
//! Pure state machines refine the PL-M models (docs/formal/dctx-plugins): `Authority` (grants, epochs,
//! selection, binding), `Receipts` (intent, replay, uncertainty), `Resources` (quotas, backpressure),
//! `Lifecycle` (decision 080 disposition, decision 077 liveness, cancel and reap) and `Hooks` (order and truth).
//! Services drive them over audit, file, recipe and process ports; `fakes` supplies deterministic spies.
//! Depends on dctx-plugin-protocol and dctx-hash only; contains no Git behaviour (decision 078/decision 079).
//! **Unstable**: this crate has no API or ABI stability promise in dctx v1 and may change in any release.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod domain;
pub mod error;
pub mod fakes;
pub mod ports;
pub mod service;

pub use domain::authority::{Admission, Authority, Layer};
pub use domain::binding::{ActionKey, ArgDigest, Binding};
pub use domain::budget::{BoundedQueue, Budget, Dimension, Quota};
pub use domain::grant::{DenyRule, GrantLayers, Need};
pub use domain::hooks::{HookEntry, HookReport, HookRun, HookStatus};
pub use domain::ledger::{EntryStatus, Ledger, Lookup};
pub use domain::lifecycle::{LaunchFacts, Origin, StopReason, Worker, WorkerPhase};
pub use domain::liveness::{LivenessFeed, Observation, ProcessEvidence, Verdict};
pub use domain::selection::{Decision, Selection};
pub use domain::trace::{ModelAction, Trace};
pub use error::HostRefusal;
pub use service::broker::{Broker, Reply};
pub use service::handles::HandleKind;
pub use service::session::HostSession;
pub use service::supervisor::Supervisor;
