//! Hook sequencing: canonical acyclic order, required-predecessor suppression and ledger-truth reports.
//! Implements PLUGINS.md §3.5 and Hooks.tla `Plan`, `RunHook`, `Publish`, `Report` (PP10): topological
//! sort with canonical-ID tie-break, cycles refused, sequential execution, earlier committed effects kept,
//! reports derived from host receipts and never from plugin claims. Depends on protocol scalars and trace.
//! Used by the facade (PL-F) through the host crate; PL-D compiles the edges.

use std::collections::{BTreeMap, BTreeSet};

use dctx_plugin_protocol::records::scalars::{PluginId, Token};
use dctx_plugin_protocol::{ErrorCode, Phase};

use crate::domain::trace::{ModelAction, Trace};
use crate::error::HostRefusal;

/// One plugin entry in an immutable effective hook plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookEntry {
    /// Plugin ID (the canonical tie-breaker).
    pub plugin: PluginId,
    /// Whether failure stops dependents and a reversible parent operation.
    pub required: bool,
}

/// Outcome of one hook run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookStatus {
    /// Not yet run.
    Pending,
    /// Ran successfully.
    Ok,
    /// Skipped because a required predecessor failed or was skipped.
    Skipped,
    /// A required hook failed.
    RequiredFailure,
    /// An advisory hook failed (visible warning).
    AdvisoryFailure,
}

/// The report of one hook boundary: statuses and the host-ledger receipts only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookReport {
    /// Per-plugin status in execution order.
    pub statuses: Vec<(PluginId, HookStatus)>,
    /// Committed receipts from the host ledger.
    pub receipts: Vec<Token>,
    /// Plugin-claimed receipts that the ledger does not contain.
    pub rejected_claims: Vec<Token>,
    /// Whether the engine output was published (never rolled back by a later failure).
    pub published: bool,
}

/// A planned, running hook boundary.
#[derive(Clone, Debug)]
pub struct HookRun {
    order: Vec<PluginId>,
    required: BTreeSet<PluginId>,
    predecessors: BTreeMap<PluginId, BTreeSet<PluginId>>,
    status: BTreeMap<PluginId, HookStatus>,
    receipts: Vec<Token>,
    claims: BTreeSet<Token>,
    next: usize,
    published: bool,
    trace: Trace,
}

impl HookRun {
    /// `Plan`: refuse unknown references and cycles; order topologically with the canonical tie-break.
    /// # Errors
    /// Refuses duplicate entries, unknown dependency references, or a hook dependency cycle.
    pub fn plan(entries: &[HookEntry], edges: &[(PluginId, PluginId)]) -> Result<Self, HostRefusal> {
        let invalid = |reason| HostRefusal::new(ErrorCode::InvalidConfig, Phase::Resolve, reason);
        let ids: BTreeSet<PluginId> = entries.iter().map(|e| e.plugin.clone()).collect();
        if ids.len() != entries.len() {
            return Err(invalid("duplicate-hook-entry"));
        }
        let mut predecessors: BTreeMap<PluginId, BTreeSet<PluginId>> =
            ids.iter().map(|id| (id.clone(), BTreeSet::new())).collect();
        for (before, after) in edges {
            if !ids.contains(before) || !ids.contains(after) {
                return Err(invalid("unknown-hook-reference"));
            }
            predecessors.entry(after.clone()).or_default().insert(before.clone());
        }
        let mut order = Vec::with_capacity(ids.len());
        let mut placed = BTreeSet::new();
        while order.len() < ids.len() {
            let ready = ids
                .iter()
                .find(|id| !placed.contains(*id) && predecessors.get(*id).is_none_or(|p| p.is_subset(&placed)));
            let Some(next) = ready.cloned() else { return Err(invalid("hook-cycle")) };
            placed.insert(next.clone());
            order.push(next);
        }
        let mut trace = Trace::default();
        trace.push(ModelAction::HooksPlan);
        Ok(Self {
            status: ids.iter().map(|id| (id.clone(), HookStatus::Pending)).collect(),
            required: entries.iter().filter(|e| e.required).map(|e| e.plugin.clone()).collect(),
            order,
            predecessors,
            receipts: Vec::new(),
            claims: BTreeSet::new(),
            next: 0,
            published: false,
            trace,
        })
    }

    /// The canonical execution order.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn order(&self) -> &[PluginId] {
        &self.order
    }

    /// Next plugin to run and whether it may run (false: a required predecessor failed or was skipped).
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn next(&self) -> Option<(&PluginId, bool)> {
        let plugin = self.order.get(self.next)?;
        Some((plugin, !self.blocked(plugin)))
    }

    /// `RunHook`: record the outcome and the host-ledger receipts of the plugin just run.
    /// # Errors
    /// Refuses when there is no pending hook left to record.
    pub fn record(&mut self, succeeded: bool, ledger_receipts: Vec<Token>, claims: Vec<Token>) -> Result<HookStatus, HostRefusal> {
        let Some(plugin) = self.order.get(self.next).cloned() else {
            return Err(HostRefusal::new(ErrorCode::InvalidMessage, Phase::Invoke, "no-hook-pending"));
        };
        let status = if self.blocked(&plugin) {
            HookStatus::Skipped
        } else if succeeded {
            HookStatus::Ok
        } else if self.required.contains(&plugin) {
            HookStatus::RequiredFailure
        } else {
            HookStatus::AdvisoryFailure
        };
        if status != HookStatus::Skipped {
            self.receipts.extend(ledger_receipts);
        }
        self.claims.extend(claims);
        self.status.insert(plugin, status);
        self.next += 1;
        self.trace.push(ModelAction::HooksRunHook);
        Ok(status)
    }

    /// Whether a pre-commit boundary may publish: every hook ran and no required hook failed.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn may_publish_before(&self) -> bool {
        self.next == self.order.len() && !self.status.values().any(|s| *s == HookStatus::RequiredFailure)
    }

    /// `Publish`: the engine output became visible; irreversible.
    pub fn published(&mut self) {
        self.published = true;
        self.trace.push(ModelAction::HooksPublish);
    }

    /// `Report`: statuses plus ledger receipts; unbacked plugin claims are rejected, not reported.
    pub fn report(&mut self) -> HookReport {
        self.trace.push(ModelAction::HooksReport);
        HookReport {
            statuses: self.order.iter().map(|id| (id.clone(), self.status.get(id).copied().unwrap_or(HookStatus::Pending))).collect(),
            receipts: self.receipts.clone(),
            rejected_claims: self.claims.iter().filter(|c| !self.receipts.contains(c)).cloned().collect(),
            published: self.published,
        }
    }

    /// Model actions taken.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn trace(&self) -> &Trace {
        &self.trace
    }

    fn blocked(&self, plugin: &PluginId) -> bool {
        self.predecessors.get(plugin).is_some_and(|preds| {
            preds.iter().any(|p| matches!(self.status.get(p), Some(HookStatus::RequiredFailure | HookStatus::Skipped)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> PluginId {
        PluginId::new(text).expect("id")
    }

    fn entry(text: &str, required: bool) -> HookEntry {
        HookEntry { plugin: id(text), required }
    }

    #[test]
    fn canonical_tie_break_and_edges() {
        let run = HookRun::plan(&[entry("x/b", false), entry("x/a", false)], &[]).expect("plan");
        assert_eq!(run.order(), &[id("x/a"), id("x/b")]);
        let run = HookRun::plan(&[entry("x/b", false), entry("x/a", false)], &[(id("x/b"), id("x/a"))]).expect("plan");
        assert_eq!(run.order(), &[id("x/b"), id("x/a")]);
    }

    #[test]
    fn cycles_and_unknown_references_refuse() {
        let cyc = HookRun::plan(&[entry("x/a", false), entry("x/b", false)], &[(id("x/a"), id("x/b")), (id("x/b"), id("x/a"))]);
        assert_eq!(cyc.map(|_| ()).map_err(|e| e.reason), Err("hook-cycle"));
        let selfloop = HookRun::plan(&[entry("x/a", false)], &[(id("x/a"), id("x/a"))]);
        assert!(selfloop.is_err());
        let unknown = HookRun::plan(&[entry("x/a", false)], &[(id("x/z"), id("x/a"))]);
        assert_eq!(unknown.map(|_| ()).map_err(|e| e.reason), Err("unknown-hook-reference"));
    }

    #[test]
    fn required_failure_skips_dependents_but_keeps_earlier_effects() {
        let edges = [(id("x/a"), id("x/b"))];
        let mut run = HookRun::plan(&[entry("x/a", true), entry("x/b", false)], &edges).expect("plan");
        let receipt = Token::new("receipt:1").expect("receipt");
        assert_eq!(run.record(false, vec![receipt.clone()], vec![]), Ok(HookStatus::RequiredFailure));
        assert_eq!(run.next().map(|(_, runs)| runs), Some(false));
        assert_eq!(run.record(true, vec![], vec![]), Ok(HookStatus::Skipped));
        assert!(!run.may_publish_before());
        let report = run.report();
        assert_eq!(report.receipts, vec![receipt]);
        assert!(!report.published);
    }

    #[test]
    fn advisory_failure_does_not_skip_and_forged_claims_are_rejected() {
        let edges = [(id("x/a"), id("x/b"))];
        let mut run = HookRun::plan(&[entry("x/a", false), entry("x/b", false)], &edges).expect("plan");
        run.record(false, vec![], vec![]).expect("a");
        assert_eq!(run.next().map(|(_, runs)| runs), Some(true));
        let forged = Token::new("receipt:3").expect("forged");
        run.record(true, vec![], vec![forged.clone()]).expect("b");
        assert!(run.may_publish_before());
        run.published();
        let report = run.report();
        assert_eq!(report.receipts, [] as [dctx_plugin_protocol::records::scalars::Token; 0]);
        assert_eq!(report.rejected_claims, vec![forged]);
        assert!(report.published);
    }
}
