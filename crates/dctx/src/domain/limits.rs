//! Operation limits, verb caps and the safety ceiling.
//! Implements contracts/API-PORTS-v1.md budgets: `ceil(2 * utf8_bytes / 7)` and verb hard caps.
//! Depends on dctx-core's estimate and on [`super::request::Verb`]. The session intersects these
//! with the host ceiling and the DTS budget. A caller cannot raise the verb hard cap or the safety cap.
//! Read defaults to its hard cap so a typical module page fits; a smaller request is still exact.

use dctx_core::budget::{byte_ceiling, estimate_tokens};
use dctx_template::{BudgetPlan, HostCeiling};

use super::error::{DctxError, Resource};
use super::request::Verb;

/// Most directory levels one snapshot will walk when the caller sets no depth.
pub(crate) const SAFETY_DEPTH: u64 = 256;
/// Most entries one snapshot will visit when the caller sets no work cap.
pub(crate) const SAFETY_ENTRIES: u64 = 100_000;
/// Most bytes read from one file.
pub(crate) const SAFETY_FILE: u64 = 8 * 1024 * 1024;
/// Most content bytes read from one snapshot.
pub(crate) const SAFETY_SOURCE: u64 = 64 * 1024 * 1024;
/// Read token budget when the caller sets none. Equal to the verb hard cap.
/// `floor(7 * 2000 / 2)` is 7,000 bytes, enough for a typical module page (~5 KB encoded).
/// The old 500-token default admitted 1,750 bytes and dropped that single record.
const READ_DEFAULT_TOKENS: u64 = 2_000;

/// Caller limits. `None` means "use the verb default, then the safety cap", not "unlimited".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OperationLimits {
    /// Final encoded bytes.
    pub encoded_bytes: Option<u64>,
    /// Estimated tokens of the final encoding.
    pub estimated_tokens: Option<u64>,
    /// Whole records.
    pub records: Option<u64>,
    /// Map or neighbor depth, or snapshot directory depth.
    pub depth: Option<u32>,
    /// Content bytes read from the source.
    pub source_bytes: Option<u64>,
    /// Bytes of one file.
    pub parse_bytes: Option<u64>,
    /// Encoded segment bytes.
    pub memory_bytes: Option<u64>,
    /// Entries visited.
    pub work_units: Option<u64>,
    /// Milliseconds on the injected clock. A frozen clock does not expire.
    pub deadline_ms: Option<u64>,
}

impl OperationLimits {
    /// The tighter of two limits in every dimension.
    #[allow(clippy::must_use_candidate, clippy::return_self_not_must_use, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn intersect(self, other: Self) -> Self {
        Self {
            encoded_bytes: min_opt(self.encoded_bytes, other.encoded_bytes),
            estimated_tokens: min_opt(self.estimated_tokens, other.estimated_tokens),
            records: min_opt(self.records, other.records),
            depth: min_opt_u32(self.depth, other.depth),
            source_bytes: min_opt(self.source_bytes, other.source_bytes),
            parse_bytes: min_opt(self.parse_bytes, other.parse_bytes),
            memory_bytes: min_opt(self.memory_bytes, other.memory_bytes),
            work_units: min_opt(self.work_units, other.work_units),
            deadline_ms: min_opt(self.deadline_ms, other.deadline_ms),
        }
    }

    pub(crate) fn from_ceiling(ceiling: &HostCeiling) -> Self {
        Self {
            encoded_bytes: finite(ceiling.max_bytes),
            estimated_tokens: finite(ceiling.max_estimated_tokens.min(ceiling.max_hard_estimated_tokens)),
            records: finite(ceiling.max_records),
            depth: finite_u32(ceiling.max_depth),
            source_bytes: None,
            parse_bytes: finite(ceiling.max_parse_memory),
            memory_bytes: finite(ceiling.max_parse_memory),
            work_units: finite(ceiling.max_work),
            deadline_ms: finite(ceiling.max_parse_time_ms),
        }
    }

    pub(crate) fn from_budget(budget: &BudgetPlan) -> Self {
        Self {
            encoded_bytes: finite(budget.bytes),
            estimated_tokens: finite(budget.estimated_tokens.min(budget.hard_estimated_tokens)),
            records: finite(budget.records),
            depth: finite_u32(budget.depth),
            source_bytes: None,
            parse_bytes: finite(budget.parse_memory),
            memory_bytes: finite(budget.parse_memory),
            work_units: finite(budget.work),
            deadline_ms: finite(budget.parse_time_ms),
        }
    }
}

fn finite(value: u64) -> Option<u64> {
    if value == u64::MAX { None } else { Some(value) }
}

fn finite_u32(value: u32) -> Option<u32> {
    if value == u32::MAX { None } else { Some(value) }
}

fn min_opt(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn min_opt_u32(left: Option<u32>, right: Option<u32>) -> Option<u32> {
    match (left, right) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// Caps actually applied to one query page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedLimits {
    /// Estimated-token cap.
    pub tokens: u64,
    /// Encoded-byte cap, already intersected with the token ceiling.
    pub bytes: u64,
    /// Record cap.
    pub records: u64,
    /// Relative depth. Zero means the verb does not use depth.
    pub depth: u32,
    /// Source lines for `read`.
    pub lines: u32,
}

struct VerbCap {
    default_tokens: u64,
    hard_tokens: u64,
    default_records: u64,
    hard_records: u64,
    default_depth: u32,
    hard_depth: u32,
    default_lines: u32,
    hard_lines: u32,
}

fn verb_cap(verb: Verb) -> VerbCap {
    match verb {
        Verb::Map => VerbCap {
            default_tokens: 500,
            hard_tokens: 2_000,
            default_records: 60,
            hard_records: 60,
            default_depth: 1,
            hard_depth: 3,
            default_lines: 0,
            hard_lines: 0,
        },
        Verb::Find => VerbCap {
            default_tokens: 500,
            hard_tokens: 2_000,
            default_records: 10,
            hard_records: 40,
            default_depth: 0,
            hard_depth: 0,
            default_lines: 0,
            hard_lines: 0,
        },
        Verb::Neighbors => VerbCap {
            default_tokens: 500,
            hard_tokens: 2_000,
            default_records: 20,
            hard_records: 60,
            default_depth: 1,
            hard_depth: 4,
            default_lines: 0,
            hard_lines: 0,
        },
        Verb::Explain => VerbCap {
            default_tokens: 400,
            hard_tokens: 1_000,
            default_records: 1,
            hard_records: 1,
            default_depth: 0,
            hard_depth: 0,
            default_lines: 0,
            hard_lines: 0,
        },
        Verb::Skeleton => VerbCap {
            default_tokens: 2_000,
            hard_tokens: 2_000,
            default_records: 60,
            hard_records: 60,
            default_depth: 0,
            hard_depth: 0,
            default_lines: 0,
            hard_lines: 0,
        },
        Verb::Read => VerbCap {
            default_tokens: READ_DEFAULT_TOKENS,
            hard_tokens: READ_DEFAULT_TOKENS,
            default_records: 1,
            hard_records: 1,
            default_depth: 0,
            hard_depth: 0,
            default_lines: 120,
            hard_lines: 400,
        },
        Verb::Grep => VerbCap {
            default_tokens: 500,
            hard_tokens: 2_000,
            default_records: 60,
            hard_records: 60,
            default_depth: 0,
            hard_depth: 0,
            default_lines: 0,
            hard_lines: 0,
        },
    }
}

pub(crate) fn resolve_query(
    verb: Verb,
    requested: &super::request::MapRequest,
    policy: OperationLimits,
) -> Result<ResolvedLimits, DctxError> {
    let cap = verb_cap(verb);
    let tokens = choose(requested.limits.estimated_tokens, cap.default_tokens, cap.hard_tokens, "tokens")?;
    let records = choose(requested.limits.records, cap.default_records, cap.hard_records, "records")?;
    let depth = if cap.hard_depth == 0 {
        0
    } else {
        u32::try_from(choose(
            requested.depth.map(u64::from).or(requested.limits.depth.map(u64::from)),
            u64::from(cap.default_depth),
            u64::from(cap.hard_depth),
            "depth",
        )?)
        .unwrap_or(cap.hard_depth)
    };
    let lines = if cap.hard_lines == 0 {
        0
    } else {
        let asked = if requested.full && requested.lines.is_none() {
            Some(u64::from(cap.hard_lines))
        } else {
            requested.lines.map(u64::from)
        };
        u32::try_from(choose(asked, u64::from(cap.default_lines), u64::from(cap.hard_lines), "lines")?)
            .unwrap_or(cap.hard_lines)
    };
    let tokens = policy.estimated_tokens.map_or(tokens, |limit| tokens.min(limit));
    let records = policy.records.map_or(records, |limit| records.min(limit));
    let depth = policy.depth.map_or(depth, |limit| depth.min(limit));
    if tokens == 0 || records == 0 {
        return Err(DctxError::BudgetTooSmall { minimum_bytes: 1 });
    }
    let token_bytes = byte_ceiling(tokens);
    let explicit = min_opt(requested.limits.encoded_bytes, policy.encoded_bytes);
    let bytes = match explicit {
        Some(limit) => token_bytes.min(limit),
        None => token_bytes,
    };
    Ok(ResolvedLimits { tokens, bytes, records, depth, lines })
}

fn choose(requested: Option<u64>, default: u64, hard: u64, what: &'static str) -> Result<u64, DctxError> {
    match requested {
        None => Ok(default.min(hard)),
        Some(0) => Err(DctxError::invalid_detail("limit must be positive", what)),
        Some(value) if value > hard => Err(DctxError::invalid_detail("limit exceeds the verb hard cap", what)),
        Some(value) => Ok(value),
    }
}

/// Snapshot walk caps. `None` becomes the safety cap. A value above the safety cap is refused.
pub(crate) struct ScanLimits {
    /// Entry cap, including directories.
    pub entries: u64,
    /// Directory depth cap.
    pub depth: u64,
    /// One-file cap.
    pub file_bytes: u64,
    /// Total content cap.
    pub source_bytes: u64,
}

pub(crate) fn resolve_scan(limits: &OperationLimits) -> Result<ScanLimits, DctxError> {
    Ok(ScanLimits {
        entries: under(limits.work_units, SAFETY_ENTRIES, "work")?,
        depth: under(limits.depth.map(u64::from), SAFETY_DEPTH, "depth")?,
        file_bytes: under(limits.parse_bytes, SAFETY_FILE, "file")?,
        source_bytes: under(limits.source_bytes, SAFETY_SOURCE, "source")?,
    })
}

fn under(requested: Option<u64>, safety: u64, what: &'static str) -> Result<u64, DctxError> {
    match requested {
        None => Ok(safety),
        Some(value) if value == 0 || value > safety => {
            Err(DctxError::invalid_detail("limit exceeds the facade safety cap", what))
        }
        Some(value) => Ok(value),
    }
}

pub(crate) fn within_budget(bytes: &[u8], limits: &ResolvedLimits) -> bool {
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    len <= limits.bytes && estimate_tokens(len) <= limits.tokens
}

pub(crate) fn check_deadline(start_ms: u64, now_ms: u64, deadline_ms: Option<u64>) -> Result<(), DctxError> {
    if let Some(bound) = deadline_ms
        && now_ms.saturating_sub(start_ms) > bound
    {
        return Err(DctxError::ResourceLimit { resource: Resource::Deadline, limit: bound });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::request::{MapRequest, Verb};

    #[test]
    fn oversized_find_is_refused_and_the_default_fits() {
        let mut request = MapRequest::new(Verb::Find);
        request.limits.estimated_tokens = Some(5_000);
        let error = resolve_query(Verb::Find, &request, OperationLimits::default()).expect_err("hard");
        assert_eq!(error.code().as_str(), "InvalidRequest");
        request.limits.estimated_tokens = Some(2_000);
        let ok = resolve_query(Verb::Find, &request, OperationLimits::default()).expect("ok");
        assert_eq!(ok.tokens, 2_000);
        let defaulted = resolve_query(Verb::Find, &MapRequest::new(Verb::Find), OperationLimits::default()).expect("default");
        assert_eq!(defaulted.records, 10);
        assert_eq!(defaulted.tokens, 500);
    }

    #[test]
    fn read_default_fits_a_typical_module_and_cannot_pass_the_hard_cap() {
        let read = resolve_query(Verb::Read, &MapRequest::new(Verb::Read), OperationLimits::default()).expect("read");
        assert_eq!(read.tokens, super::READ_DEFAULT_TOKENS);
        assert!(read.bytes >= 5_250, "a ~5 KB module page must fit, got {} bytes", read.bytes);
        assert!(super::estimate_tokens(5_250) <= read.tokens);

        let mut narrow = MapRequest::new(Verb::Read);
        narrow.limits.estimated_tokens = Some(500);
        let narrow = resolve_query(Verb::Read, &narrow, OperationLimits::default()).expect("narrow");
        assert_eq!(narrow.tokens, 500);
        assert!(narrow.bytes < 5_250, "an explicit 500-token budget stays narrow");

        let mut over = MapRequest::new(Verb::Read);
        over.limits.estimated_tokens = Some(super::READ_DEFAULT_TOKENS + 1);
        let error = resolve_query(Verb::Read, &over, OperationLimits::default()).expect_err("cap");
        assert!(error.to_string().contains("hard cap"), "{error}");
    }

    #[test]
    fn explain_defaults_are_tighter_than_map() {
        let explain = resolve_query(Verb::Explain, &MapRequest::new(Verb::Explain), OperationLimits::default()).expect("e");
        let map = resolve_query(Verb::Map, &MapRequest::new(Verb::Map), OperationLimits::default()).expect("m");
        assert_eq!(explain.tokens, 400);
        assert_eq!(map.depth, 1);
        assert_eq!(map.records, 60);
    }

    #[test]
    fn safety_cap_refuses_a_raised_walk_limit() {
        let mut limits = OperationLimits { work_units: Some(SAFETY_ENTRIES + 1), ..OperationLimits::default() };
        assert!(resolve_scan(&limits).is_err());
        limits.work_units = Some(4);
        assert_eq!(resolve_scan(&limits).expect("narrow").entries, 4);
        limits.work_units = Some(0);
        assert!(resolve_scan(&limits).is_err());
    }
}

#[cfg(test)]
mod properties {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn intersection_keeps_the_smaller_token_cap(a in 1u64..20_000, b in 1u64..20_000) {
            let left = OperationLimits { estimated_tokens: Some(a), ..OperationLimits::default() };
            let right = OperationLimits { estimated_tokens: Some(b), ..OperationLimits::default() };
            let got = left.intersect(right);
            prop_assert_eq!(got.estimated_tokens, Some(a.min(b)));
        }
    }
}
