//! Typed ContextPlan, host ceiling and plugin catalog.
//! Implements contracts/DTS-v2.md (ContextPlanV1) and PLUGINS.md §3.5 declarations.
//! Depends on [`crate::error`] and [`dctx_core::FileHandlingDecision`]. Used by compile, encode and render.
//! Templates narrow these ceilings; they never raise them.

use std::collections::{BTreeMap, BTreeSet};

use dctx_core::FileHandlingDecision;

use crate::error::{MAX_INSTRUCTIONS, MAX_PLAN_BYTES, MAX_SECTIONS};
use crate::plugin::{ConfigEntry, KnownPlugin, Liveness, NetworkPolicy, PluginPlan, ResourceLimits};

/// Render instruction: copy a string from the program table.
pub const OP_TEXT: u16 = 1;
/// Render instruction: insert one runtime field.
pub const OP_DATA: u16 = 2;
/// Render instruction: inline another partial.
pub const OP_PARTIAL: u16 = 3;
/// Highest opcode this IR executes.
pub const OP_MAX: u16 = OP_PARTIAL;

/// Required-capability bits stored in the IR header.
pub const CAP_PACK: u64 = 1 << 0;
/// View mode `map`.
pub const CAP_MAP: u64 = 1 << 1;
/// Parser `syn`.
pub const CAP_SYN: u64 = 1 << 2;
/// Parser `structural`.
pub const CAP_STRUCTURAL: u64 = 1 << 3;
/// Parser `syntax`.
pub const CAP_SYNTAX: u64 = 1 << 4;
/// Parser `opaque`.
pub const CAP_OPAQUE: u64 = 1 << 5;
/// Optional native parser. Absent unless the host grants it.
pub const CAP_TREE_SITTER: u64 = 1 << 6;
/// The plan enables at least one plugin.
pub const CAP_PLUGIN_HOST: u64 = 1 << 7;
/// Bits this reader understands. Any other bit refuses the load.
pub const KNOWN_CAPABILITY_BITS: u64 = (1 << 8) - 1;

/// One SELECT rule. Later rules win inside the host ceiling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectRule {
    /// Decision requested when [`SelectRule::pattern`] matches.
    pub decision: FileHandlingDecision,
    /// Root-relative glob.
    pub pattern: String,
}

/// One named SELECT definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Definition name.
    pub name: String,
    /// Root-relative directory prefixes.
    pub roots: Vec<String>,
    /// Language tags the selection admits.
    pub languages: Vec<String>,
    /// Record kinds the selection admits.
    pub kinds: Vec<String>,
    /// Rules in source order.
    pub rules: Vec<SelectRule>,
}

/// One named INDEX definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexPlan {
    /// Definition name.
    pub name: String,
    /// Fact names the extractor may emit.
    pub facts: Vec<String>,
    /// `syn`, `structural`, `syntax`, `opaque` or `tree-sitter`.
    pub parser: String,
    /// Language tag, or empty when the index does not pin one.
    pub language: String,
    /// Dialect token, or empty.
    pub dialect: String,
    /// Most bytes the extractor may read from one file.
    pub max_bytes: u64,
}

/// Concrete budget after the host ceiling was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetPlan {
    /// Definition name.
    pub name: String,
    /// Final encoded bytes.
    pub bytes: u64,
    /// Soft token estimate cap.
    pub estimated_tokens: u64,
    /// Hard token estimate cap.
    pub hard_estimated_tokens: u64,
    /// Most records in one answer.
    pub records: u64,
    /// Deepest record depth.
    pub depth: u32,
    /// Parse time ceiling, in milliseconds.
    pub parse_time_ms: u64,
    /// Parse memory ceiling, in bytes.
    pub parse_memory: u64,
    /// Work counter ceiling.
    pub work: u64,
}

/// One named VIEW. Names of other sections are resolved at compile time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewPlan {
    /// Definition name.
    pub name: String,
    /// `pack`, `map` or a future mode token.
    pub mode: String,
    /// SELECT name.
    pub selection: String,
    /// INDEX name, or empty when the view does not extract.
    pub index: String,
    /// BUDGET name.
    pub budget: String,
    /// Render program name, or empty when the view has no layout body.
    pub render: String,
    /// Output label (`SNAPSHOT`, `AGENT_LINES`, `JSON`, …).
    pub layout_label: String,
}

/// One render instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inst {
    /// [`OP_TEXT`], [`OP_DATA`] or [`OP_PARTIAL`].
    pub opcode: u16,
    /// String index, field id or partial index.
    pub arg: u16,
}

/// Named bytecode sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedCode {
    /// Partial or layout name.
    pub name: String,
    /// Instructions in order.
    pub instructions: Vec<Inst>,
}

/// RENDER program: resolved variables, partials and layouts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderProgram {
    /// String table addressed by [`OP_TEXT`].
    pub strings: Vec<String>,
    /// Resolved variables in key order, used after data insertion.
    pub variables: Vec<(String, String)>,
    /// Partials in source order.
    pub partials: Vec<NamedCode>,
    /// Layouts in source order. The v1 layout is `main`.
    pub layouts: Vec<NamedCode>,
}

impl RenderProgram {
    /// Number of instructions across partials and layouts.
    pub fn instruction_count(&self) -> usize {
        let partials: usize = self.partials.iter().map(|code| code.instructions.len()).sum();
        let layouts: usize = self.layouts.iter().map(|code| code.instructions.len()).sum();
        partials + layouts
    }
}

/// Versioned organization plan. Digests are domain-separated SHA-256 values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPlan {
    /// `1` for a legacy template, `2` when `[DTS] version = 2` is present.
    pub source_version: u16,
    /// Required capability bits.
    pub capabilities: u64,
    /// SELECT definitions in name order.
    pub selections: Vec<Selection>,
    /// INDEX definitions in name order.
    pub indexes: Vec<IndexPlan>,
    /// BUDGET definitions in name order.
    pub budgets: Vec<BudgetPlan>,
    /// VIEW definitions in name order.
    pub views: Vec<ViewPlan>,
    /// RENDER program.
    pub render: RenderProgram,
    /// PLUGIN plan.
    pub plugins: PluginPlan,
    /// Identity kind 4 over the SELECT payload.
    pub selection_digest: [u8; 32],
    /// Identity kind 5 over the INDEX payload.
    pub extraction_digest: [u8; 32],
    /// Identity kind 8 schema 2 over the RENDER payload.
    pub render_digest: [u8; 32],
    /// Identity kind 8 schema 3 over the PLUGIN payload.
    pub plugin_digest: [u8; 32],
    /// Identity kind 8 schema 1 over the plan preimage.
    pub plan_digest: [u8; 32],
}

impl ContextPlan {
    /// Selection and extraction digests.
    /// These identify map facts. Plugin configuration is not part of either value.
    pub fn map_facts(&self) -> ([u8; 32], [u8; 32]) {
        (self.selection_digest, self.extraction_digest)
    }
}

/// Host grant for one glob. Unmatched paths are [`FileHandlingDecision::Exclude`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRule {
    /// Root-relative glob.
    pub pattern: String,
    /// Widest decision the template may keep for a match.
    pub decision: FileHandlingDecision,
}

/// Ceilings the template is allowed to narrow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCeiling {
    /// Most final answer bytes.
    pub max_bytes: u64,
    /// Most estimated tokens.
    pub max_estimated_tokens: u64,
    /// Most hard estimated tokens.
    pub max_hard_estimated_tokens: u64,
    /// Most records.
    pub max_records: u64,
    /// Deepest depth.
    pub max_depth: u32,
    /// Parse time ceiling, in milliseconds.
    pub max_parse_time_ms: u64,
    /// Parse memory ceiling, in bytes.
    pub max_parse_memory: u64,
    /// Work ceiling.
    pub max_work: u64,
    /// Granted capability tokens (`mode:pack`, `parser:syn`, …).
    pub capabilities: BTreeSet<String>,
    /// Path ceilings. Empty means every path is excluded.
    pub grants: Vec<GrantRule>,
    /// Plugins the host has already resolved.
    pub plugins: Vec<KnownPlugin>,
    /// User disables. Sticky: a template cannot clear these.
    pub disabled_plugins: BTreeSet<String>,
    /// Selectors removed from a plugin's manifest ceiling. Key `*` applies to every plugin.
    pub denied_capabilities: BTreeMap<String, BTreeSet<String>>,
    /// User config applied after manifest defaults and before the template.
    pub user_config: BTreeMap<String, Vec<ConfigEntry>>,
    /// Plugin resource ceiling.
    pub limits: ResourceLimits,
    /// Plugin liveness policy. The template cannot set it.
    pub liveness: Liveness,
    /// Plan size cap. Values above [`MAX_PLAN_BYTES`] are treated as that spec cap.
    pub max_plan_bytes: u32,
    /// Section cap. Values above [`MAX_SECTIONS`] are treated as that spec cap.
    pub max_sections: u32,
    /// Instruction cap. Values above [`MAX_INSTRUCTIONS`] are treated as that spec cap.
    pub max_instructions: u32,
    /// Widest network policy.
    pub network: NetworkPolicy,
}

impl HostCeiling {
    /// Spec maximums, pure parsers, pack and map, no grants and no plugins.
    pub fn base() -> Self {
        let mut capabilities = BTreeSet::new();
        for name in [
            "mode:pack",
            "mode:map",
            "parser:syn",
            "parser:structural",
            "parser:syntax",
            "parser:opaque",
        ] {
            capabilities.insert(name.to_string());
        }
        Self {
            max_bytes: u64::MAX,
            max_estimated_tokens: u64::MAX,
            max_hard_estimated_tokens: u64::MAX,
            max_records: u64::MAX,
            max_depth: u32::MAX,
            max_parse_time_ms: u64::MAX,
            max_parse_memory: u64::MAX,
            max_work: u64::MAX,
            capabilities,
            grants: Vec::new(),
            plugins: Vec::new(),
            disabled_plugins: BTreeSet::new(),
            denied_capabilities: BTreeMap::new(),
            user_config: BTreeMap::new(),
            limits: ResourceLimits::standard(),
            liveness: Liveness::fail_closed(),
            max_plan_bytes: MAX_PLAN_BYTES,
            max_sections: MAX_SECTIONS,
            max_instructions: MAX_INSTRUCTIONS,
            network: NetworkPolicy::None,
        }
    }
}

/// Facts an INDEX may name.
pub const KNOWN_FACTS: &[&str] = &["symbols", "imports", "calls", "headings", "docs", "references"];

/// Languages an INDEX or SELECT may name.
pub const KNOWN_LANGUAGES: &[&str] = &["rust", "javascript", "typescript", "markdown", "opaque"];

/// Kinds a SELECT may name.
pub const KNOWN_KINDS: &[&str] = &["file", "directory", "symbol", "heading", "module"];
