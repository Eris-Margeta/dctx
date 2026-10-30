//! Versioned requests for snapshot, pack and the five map verbs.
//! Implements contracts/API-PORTS-v1.md ("Requests, budgets and results").
//! Depends on [`super::limits::OperationLimits`]. The session validates these before any source read.
//! Callers own the request; the facade does not discover a view, a clock or a policy from the environment.
//! Pack view filters are fields on [`PackRequest`]. Unset, they admit every path.

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
    /// Literal text search over the indexed files, hits grouped by enclosing symbol.
    Grep,
}

impl Verb {
    /// Wire name (`map`, `find`, `neighbors`, `explain`, `read`, `grep`).
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Map => "map",
            Self::Find => "find",
            Self::Neighbors => "neighbors",
            Self::Explain => "explain",
            Self::Read => "read",
            Self::Grep => "grep",
        }
    }

    /// Parses a wire name.
    /// # Errors
    /// Refuses an unknown verb name.
    pub fn parse(text: &str) -> Result<Self, DctxError> {
        match text {
            "map" => Ok(Self::Map),
            "find" => Ok(Self::Find),
            "neighbors" => Ok(Self::Neighbors),
            "explain" => Ok(Self::Explain),
            "read" => Ok(Self::Read),
            "grep" => Ok(Self::Grep),
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
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
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
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
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
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn immutable() -> Self {
        Self { request_version: REQUEST_VERSION, freshness: FreshnessMode::ImmutableSnapshot, limits: OperationLimits::default() }
    }

    /// Metadata inventory with safety caps.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn metadata() -> Self {
        Self { request_version: REQUEST_VERSION, freshness: FreshnessMode::Metadata, limits: OperationLimits::default() }
    }

    /// Strict snapshot with safety caps.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn strict() -> Self {
        Self { request_version: REQUEST_VERSION, freshness: FreshnessMode::Strict, limits: OperationLimits::default() }
    }
}

/// One pack run. The session's selection policy is the policy; this request does not load files.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools, reason = "These independent public pack options retain their existing combinations and serialized request schema.")]
pub struct PackRequest {
    /// Must be [`REQUEST_VERSION`].
    pub request_version: u32,
    /// Fact-store view name. Empty selects the session view.
    pub view: String,
    /// Scan and output caps.
    pub limits: OperationLimits,
    /// `2` is JSON v2, `1` is text.
    pub output_format: u32,
    /// Optional legacy document instead of the summary selected by `output_format`.
    /// Template, project variables and SVG handling are supplied by the caller.
    pub legacy_output: Option<dctx_template::LegacyOutput>,
    /// Any value refuses with `git` before a scan. The core has no git identity.
    pub git_revision: Option<String>,
    /// When false, every path is admitted and the scan is unchanged.
    /// The CLI sets this so extension, size and binary filters hide files before they are opened.
    pub apply_filters: bool,
    /// Extensions admitted when [`Self::apply_filters`] is set. Empty admits every extension.
    pub extensions: Vec<String>,
    /// Include globs. Empty admits every file the other filters keep. Directories are not dropped.
    pub include_patterns: Vec<String>,
    /// Exclude globs. A hit drops the file or the directory.
    pub exclude_patterns: Vec<String>,
    /// When false, an extension binary is omitted. A sniff-only binary is not classified here.
    pub include_binary: bool,
    /// Admit only empty files.
    pub only_empty: bool,
    /// Admit only non-empty files.
    pub only_non_empty: bool,
    /// Minimum file size in bytes, when set.
    pub min_size_bytes: Option<u64>,
    /// Maximum file size in bytes, when set.
    pub max_size_bytes: Option<u64>,
    /// Admit only hidden names. A non-hidden directory is dropped with its children.
    pub only_hidden: bool,
}

impl PackRequest {
    /// JSON v2 pack of the session view.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new() -> Self {
        Self {
            request_version: REQUEST_VERSION,
            view: String::new(),
            limits: OperationLimits::default(),
            output_format: 2,
            legacy_output: None,
            git_revision: None,
            apply_filters: false,
            extensions: Vec::new(),
            include_patterns: Vec::new(),
            exclude_patterns: Vec::new(),
            include_binary: false,
            only_empty: false,
            only_non_empty: false,
            min_size_bytes: None,
            max_size_bytes: None,
            only_hidden: false,
        }
    }

    /// True when a directory may be listed. Include patterns do not drop directories.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn admits_directory(&self, path: &str, name: &str) -> bool {
        if !self.apply_filters {
            return true;
        }
        self.view_filter().should_include_directory(name) && !pattern_hit(&self.exclude_patterns, path, name, true)
    }

    /// True when a file may be opened. `size` is used only for the empty and byte-size filters.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn admits_file(&self, path: &str, name: &str, size: u64) -> bool {
        if !self.apply_filters {
            return true;
        }
        let binary = extension_binary(name);
        if !self.view_filter().should_include_file(name, size, binary, std::path::Path::new(path)) {
            return false;
        }
        if !self.include_patterns.is_empty() && !pattern_hit(&self.include_patterns, path, name, false) {
            return false;
        }
        !pattern_hit(&self.exclude_patterns, path, name, false)
    }

    /// True when admission needs a size, so the caller stats the file before deciding.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn filters_need_size(&self) -> bool {
        self.apply_filters
            && (self.only_empty || self.only_non_empty || self.min_size_bytes.is_some() || self.max_size_bytes.is_some())
    }

    fn view_filter(&self) -> dctx_core::filter::FilterConfig {
        dctx_core::filter::FilterConfig {
            extensions: self.extensions.clone(),
            include_binary: self.include_binary,
            only_empty: self.only_empty,
            only_non_empty: self.only_non_empty,
            min_size_bytes: self.min_size_bytes,
            max_size_bytes: self.max_size_bytes,
            only_hidden: self.only_hidden,
            collect_ignored_folders: false,
        }
    }
}

fn extension_binary(name: &str) -> bool {
    matches!(
        dctx_core::file_classifier::classify_file(std::path::Path::new(name), b""),
        dctx_core::file_classifier::FileContentHandling::OmitBinary { .. }
    )
}

fn pattern_hit(patterns: &[String], path: &str, name: &str, is_dir: bool) -> bool {
    patterns.iter().any(|pattern| {
        dctx_core::selection::parse_ignore_pattern_line(pattern)
            .is_some_and(|rule| dctx_core::selection::rule_matches(&rule, path, name, is_dir))
    })
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
    /// Find text, or the literal for grep. Matching is case-sensitive unless `ignore_case` is set.
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
    /// Absolute 1-based first line for read. `None` starts at the node.
    /// `depth` is the hop count and is never a line number. Zero is invalid.
    pub line_start: Option<u32>,
    /// When set, and `lines` is `None`, read uses the hard line cap.
    pub full: bool,
    /// Any value refuses with `git` before a read.
    pub git_revision: Option<String>,
    /// Optional path prefix. Records outside it are dropped.
    pub within: Option<String>,
    /// When set, the page also carries the snapshot digest, the generation and the freshness age. Off by default so equal answers are equal bytes.
    pub meta: bool,
    /// Grep only: fold ASCII letters when matching the literal.
    pub ignore_case: bool,
}

impl MapRequest {
    /// A request for `verb` at version 1, JSON v2, both directions.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
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
            line_start: None,
            full: false,
            git_revision: None,
            within: None,
            meta: false,
            ignore_case: false,
        }
    }

    /// Checks the read line window. A positive read `depth` is the hop cap, not a line.
    /// # Errors
    /// Refuses incompatible window selectors, invalid line starts, or limits above the selected verb's hard caps.
    pub fn validate(&self) -> Result<(), DctxError> {
        if self.verb == Verb::Grep && self.query.is_empty() {
            return Err(DctxError::invalid("grep requires a literal"));
        }
        if self.ignore_case && self.verb != Verb::Grep {
            return Err(DctxError::invalid("ignore case applies only to grep"));
        }
        if let Some(start) = self.line_start {
            if self.verb != Verb::Read {
                return Err(DctxError::invalid("line start applies only to read"));
            }
            if start == 0 {
                return Err(DctxError::invalid("line start must be positive"));
            }
        }
        if self.verb == Verb::Read {
            if self.line_start.is_some() && self.continuation.is_some() {
                return Err(DctxError::invalid("line start and a continuation both select the window"));
            }
            // Read's hard depth is 0. A positive depth used to carry `--lines` start.
            if self.depth.is_some_and(|depth| depth > 0) {
                return Err(DctxError::invalid_detail("limit exceeds the verb hard cap", "depth"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verb_names_round_trip() {
        for name in ["map", "find", "neighbors", "explain", "read", "grep"] {
            assert_eq!(Verb::parse(name).expect("verb").as_str(), name);
        }
        assert!(Verb::parse("pack").is_err());
        assert_eq!(MapRequest::new(Verb::Find).output_format, 2);
        assert!(MapRequest::new(Verb::Read).line_start.is_none());
        assert_eq!(SnapshotRequest::metadata().freshness, FreshnessMode::Metadata);
    }

    #[test]
    fn a_read_depth_is_not_a_line_start() {
        let mut request = MapRequest::new(Verb::Read);
        request.target = "m:lines.rs".to_string();
        request.depth = Some(35);
        request.lines = Some(2);
        let error = request.validate().expect_err("smuggled start");
        assert!(error.to_string().contains("depth"), "{error}");

        request.depth = Some(2);
        request.line_start = Some(35);
        let error = request.validate().expect_err("depth and lines");
        assert!(error.to_string().contains("depth"), "{error}");

        request.depth = Some(0);
        assert!(request.validate().is_ok(), "depth 0 is the read hop cap");
        request.depth = None;
        assert!(request.validate().is_ok());

        request.line_start = Some(0);
        assert!(request.validate().is_err());
        request.line_start = Some(35);
        request.continuation = Some("v1|1|read|m:lines.rs|36|40|0".to_string());
        assert!(request.validate().is_err());

        request.continuation = None;
        request.verb = Verb::Map;
        request.depth = Some(1);
        let error = request.validate().expect_err("map line start");
        assert!(error.to_string().contains("line start"), "{error}");
    }

    #[test]
    fn unset_filters_admit_everything_and_an_extension_filter_drops_the_rest() {
        let open = PackRequest::new();
        assert!(!open.filters_need_size());
        assert!(open.admits_file("b.md", "b.md", 4));
        assert!(open.admits_file("logo.png", "logo.png", 8));
        assert!(open.admits_directory("src", "src"));
        let mut rust_only = PackRequest::new();
        rust_only.apply_filters = true;
        rust_only.extensions = vec!["rs".to_string()];
        assert!(rust_only.admits_file("src/a.rs", "a.rs", 4));
        assert!(!rust_only.admits_file("b.md", "b.md", 4));
        assert!(!rust_only.admits_file("logo.png", "logo.png", 8));
        assert!(rust_only.admits_directory("src", "src"));
        rust_only.include_binary = true;
        rust_only.extensions.clear();
        rust_only.min_size_bytes = Some(100);
        assert!(rust_only.filters_need_size());
        assert!(rust_only.admits_file("logo.png", "logo.png", 100));
        assert!(!rust_only.admits_file("logo.png", "logo.png", 99));
        rust_only.exclude_patterns = vec!["secret/".to_string()];
        assert!(!rust_only.admits_directory("secret", "secret"));
    }
}
