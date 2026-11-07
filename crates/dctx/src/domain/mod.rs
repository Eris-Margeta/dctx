// SPDX-License-Identifier: Apache-2.0
//! Facade domain: owned request, result, error and policy types.
//! Implements ARCHITECTURE-PROPOSAL §2 (library contract) and contracts/API-PORTS-v1.md.
//! Depends on dctx-core, dctx-hash and dctx-template for the pure rules those crates already own.
//! The session and the adapters consume these types; they are not a second engine.

mod answer;
mod capabilities;
mod error;
mod grep_lines;
mod identity;
mod limits;
mod policy;
mod report;
mod request;
mod rules;
mod sink;
mod source_ceiling;
mod wire;

pub use answer::{Answer, AnswerRecord, Freshness, IdentityOutcome, Precision};
pub use capabilities::Capabilities;
pub use error::{DctxError, ErrorCode, Resource};
pub use identity::{STRUCTURAL_PARSER, object_key};
pub use limits::OperationLimits;
pub use policy::{HostGrants, PolicyView};
pub use report::{
    AutoCleanConfig, CacheLedger, CacheReport, GcRequest, PackReport, PurgeRequest, PurgeScope,
    SegmentInfo, validate_segment,
};
pub use request::{
    Direction, FreshnessMode, MapRequest, PackRequest, REQUEST_VERSION, SnapshotRequest, Verb,
};
pub use rules::RuleSources;
pub use sink::{BoundedSink, VecSink};
pub use source_ceiling::{
    ENTRIES_ENV, RECORDS_ENV, SOURCE_ENV, parse_source_ceiling, records_ceiling, source_ceiling,
};
pub use wire::{encode_answer, encode_answer_with};

pub(crate) use grep_lines::{append_rows, append_summary, hit_row, shrink};
pub(crate) use identity::digest_text;
pub(crate) use limits::{
    ResolvedLimits, ScanLimits, check_deadline, resolve_query, resolve_scan, within_budget,
};
pub(crate) use policy::{ParserChoice, ancestors_include};
pub(crate) use report::seal_segment;
pub(crate) use wire::{cursor, encode_text, parse_cursor};

pub(crate) use wire::push_string as quote_json;
