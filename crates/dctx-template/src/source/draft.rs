// SPDX-License-Identifier: Apache-2.0
//! Unchecked DTS section types before host validation.
//! Implements ARCHITECTURE-PROPOSAL §2 and PLUGINS.md §3.5.
//! Depends on the plan model and v1 template types; used by the DTS source parser.

use crate::model::Selection;
use crate::v1::Template;

/// Unchecked v2 sections plus the v1 render pieces.
#[derive(Debug)]
pub(crate) struct Draft {
    pub selections: Vec<Selection>,
    pub indexes: Vec<IndexDraft>,
    pub budgets: Vec<BudgetDraft>,
    pub views: Vec<ViewDraft>,
    pub groups: Vec<GroupDraft>,
    pub plugins: Vec<PluginDecl>,
    pub template: Template,
    pub partials: Vec<(String, String)>,
    pub layouts: Vec<(String, String)>,
}

/// INDEX fields before capability checks.
#[derive(Debug)]
pub(crate) struct IndexDraft {
    pub name: String,
    pub facts: Vec<String>,
    pub parser: Option<String>,
    pub language: String,
    pub dialect: String,
    pub max_bytes: Option<u64>,
}

/// BUDGET fields before the host ceiling is applied. `None` means "use the ceiling".
#[derive(Debug)]
pub(crate) struct BudgetDraft {
    pub name: String,
    pub bytes: Option<u64>,
    pub estimated_tokens: Option<u64>,
    pub hard_estimated_tokens: Option<u64>,
    pub records: Option<u64>,
    pub depth: Option<u32>,
    pub parse_time_ms: Option<u64>,
    pub parse_memory: Option<u64>,
    pub work: Option<u64>,
}

/// VIEW fields before reference checks.
#[derive(Debug)]
pub(crate) struct ViewDraft {
    pub name: String,
    pub mode: Option<String>,
    pub selection: Option<String>,
    pub index: Option<String>,
    pub budget: Option<String>,
    pub render: Option<String>,
    pub layout_label: Option<String>,
}

/// PLUGINS group before network narrowing.
#[derive(Debug)]
pub(crate) struct GroupDraft {
    pub name: String,
    pub network: Option<String>,
    pub enabled: Vec<String>,
    pub order: Vec<String>,
}

/// PLUGIN declaration before catalog checks.
#[derive(Debug)]
pub(crate) struct PluginDecl {
    pub id: String,
    pub version: Option<String>,
    pub digest: Option<String>,
    pub hooks: Vec<String>,
    pub required: bool,
    pub alias: Option<String>,
    pub allow: Vec<String>,
    pub config: Vec<(String, RawValue)>,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

/// A source value that is not yet checked against a manifest field.
#[derive(Debug, Clone)]
pub(crate) enum RawValue {
    Bool(bool),
    Int(u64),
    Str(String),
    List(Vec<String>),
}

