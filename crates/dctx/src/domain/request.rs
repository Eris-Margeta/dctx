//! Versioned requests for snapshot, pack and the five map verbs.
//! Implements contracts/API-PORTS-v1.md ("Requests, budgets and results").
//! Depends on [`super::limits::OperationLimits`]. The session validates these before any source read.
//! Callers own the request; the facade does not discover a view, a clock or a policy from the environment.

use super::error::DctxError;
use super::limits::OperationLimits;

/// Request major this facade accepts.
pub const REQUEST_VERSION: u32 = 1;

/// One of the five read verbs. Cache administration is not a verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// Children of a node, bounded by depth.
    Map,
    /// Ranked name and path search.
    Find,
    /// Edges around a node.
    Neighbors,
    /// One node's signature and summary.
    Explain,
    /// A verified source span.
    Read,
}

impl Verb {
    /// Wire name (`map`, `find`, `neighbors`, `explain`, `read`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Map => "map",
            Self::Find => "find",
            Self::Neighbors => "neighbors",
            Self::Explain => "explain",
            Self::Read => "read",
        }
    }

    /// Parses a wire name.
    pub fn parse(text: &str) -> Result<Self, DctxError> {
        match text {
            "map" => Ok(Self::Map),
            "find" => Ok(Self::Find),
            "neighbors" => Ok(Self::Neighbors),
            "explain" => Ok(Self::Explain),
            "read" => Ok(Self::Read),
            _ => Err(DctxError::invalid("unknown verb")),
        }
    }
}

/// Which side of an edge a neighbors query returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    /// Edges leaving the target.
    Out,
    /// Edges entering the target.
    In,
    /// Both sides.
    #[default]
    Both,
}

impl Direction {
    /// Wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Out => "out",
            Self::In => "in",
            Self::Both => "both",
        }
    }
}

/// How a snapshot proves freshness. The clock is the caller's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessMode {
    /// Hash authorized bytes once and serve the pinned generation.
    ImmutableSnapshot,
    /// Hash authorized bytes and label the generation strict.
    Strict,
    /// Names and metadata only. No content read and no symbols.
    Metadata,
}

impl FreshnessMode {
    /// Wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ImmutableSnapshot => "immutable_snapshot",
            Self::Strict => "strict",
            Self::Metadata => "metadata",
        }
    }
}

/// Opens one pinned generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRequest {
    /// Must be [`REQUEST_VERSION`].
    pub request_version: u32,
    /// Freshness proof this snapshot will record.
    pub freshness: FreshnessMode,
    /// Walk caps. `None` selects the safety cap, never unlimited.
    pub limits: OperationLimits,
}

impl SnapshotRequest {
    /// Immutable snapshot with safety caps.
    pub fn immutable() -> Self {
        Self { request_version: REQUEST_VERSION, freshness: FreshnessMode::ImmutableSnapshot, limits: OperationLimits::default() }
    }

    /// Metadata inventory with safety caps.
    pub fn metadata() -> Self {
        Self { request_version: REQUEST_VERSION, freshness: FreshnessMode::Metadata, limits: OperationLimits::default() }
    }

    /// Strict snapshot with safety caps.
    pub fn strict() -> Self {
        Self { request_version: REQUEST_VERSION, freshness: FreshnessMode::Strict, limits: OperationLimits::default() }
    }
}

/// One pack run. The session's selection policy is the policy; this request does not load files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRequest {
    /// Must be [`REQUEST_VERSION`].
    pub request_version: u32,
    /// Fact-store view name. Empty selects the session view.
    pub view: String,
    /// Scan and output caps.
    pub limits: OperationLimits,
    /// `2` is JSON v2, `1` is text.
    pub output_format: u32,
    /// Any value refuses with `git` before a scan. The core has no git identity.
    pub git_revision: Option<String>,
}

impl PackRequest {
    /// JSON v2 pack of the session view.
    pub fn new() -> Self {
        Self {
            request_version: REQUEST_VERSION,
            view: String::new(),
            limits: OperationLimits::default(),
            output_format: 2,
            git_revision: None,
        }
    }
}

impl Default for PackRequest {
    fn default() -> Self {
        Self::new()
    }
}

/// One map-mode query. Convenience methods on the session fill the same fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapRequest {
    /// Must be [`REQUEST_VERSION`].
    pub request_version: u32,
    /// Which verb to run.
    pub verb: Verb,
    /// Stable id for map, neighbors, explain and read.
    pub target: String,
    /// Find text. Matching is case-sensitive.
    pub query: String,
    /// Kind filter. Empty admits every kind.
    pub kinds: Vec<String>,
    /// Edge filter. Empty admits every edge.
    pub edges: Vec<String>,
    /// Graph depth for map and neighbors. `None` uses the verb default.
    pub depth: Option<u32>,
    /// Neighbors direction.
    pub direction: Direction,
    /// Output and page caps.
    pub limits: OperationLimits,
    /// `2` is JSON v2, `1` is text. Any other value is [`DctxError::UnsupportedVersion`].
    pub output_format: u32,
    /// Opaque cursor from a previous page, or `None`.
    pub continuation: Option<String>,
    /// Source lines for read. Above the hard cap is [`DctxError::InvalidRequest`].
    pub lines: Option<u32>,
    /// When set, and `lines` is `None`, read uses the hard line cap.
    pub full: bool,
    /// Any value refuses with `git` before a read.
    pub git_revision: Option<String>,
    /// Optional path prefix. Records outside it are dropped.
    pub within: Option<String>,
}

impl MapRequest {
    /// A request for `verb` at version 1, JSON v2, both directions.
    pub fn new(verb: Verb) -> Self {
        Self {
            request_version: REQUEST_VERSION,
            verb,
            target: String::new(),
            query: String::new(),
            kinds: Vec::new(),
            edges: Vec::new(),
            depth: None,
            direction: Direction::Both,
            limits: OperationLimits::default(),
            output_format: 2,
            continuation: None,
            lines: None,
            full: false,
            git_revision: None,
            within: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verb_names_round_trip() {
        for name in ["map", "find", "neighbors", "explain", "read"] {
            assert_eq!(Verb::parse(name).expect("verb").as_str(), name);
        }
        assert!(Verb::parse("pack").is_err());
        assert_eq!(MapRequest::new(Verb::Find).output_format, 2);
        assert_eq!(SnapshotRequest::metadata().freshness, FreshnessMode::Metadata);
    }
}
