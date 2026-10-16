//! Facade domain: owned request, result, error and policy types.
//! Implements ARCHITECTURE-PROPOSAL §2 (library contract) and contracts/API-PORTS-v1.md.
//! Depends on dctx-core, dctx-hash and dctx-template for the pure rules those crates already own.
//! The session and the adapters consume these types; they are not a second engine.

mod answer;
mod capabilities;
mod error;
mod identity;
mod limits;
mod policy;
mod report;
mod request;
mod rules;
mod sink;
mod wire;

pub use answer::{Answer, AnswerRecord, Freshness, IdentityOutcome, Precision};
pub use capabilities::Capabilities;
pub use error::{DctxError, ErrorCode, Resource};
pub use identity::{object_key, STRUCTURAL_PARSER};
pub use limits::OperationLimits;
pub use policy::{HostGrants, PolicyView};
pub use rules::RuleSources;
pub use report::{validate_segment, CacheReport, GcRequest, PackReport, PurgeRequest, PurgeScope, SegmentInfo};
pub use request::{
    Direction, FreshnessMode, MapRequest, PackRequest, SnapshotRequest, Verb, REQUEST_VERSION,
};
pub use sink::{BoundedSink, VecSink};
pub use wire::encode_answer;

pub(crate) use identity::digest_text;
pub(crate) use limits::{
    check_deadline, resolve_query, resolve_scan, within_budget, ResolvedLimits, ScanLimits,
};
pub(crate) use policy::{ancestors_include, ParserChoice};
pub(crate) use report::seal_segment;
pub(crate) use wire::{cursor, encode_text, parse_cursor};
