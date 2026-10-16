//! Sequences plugin hooks around pack, index and cache. Engines call these methods only.
//! Implements PLUGINS.md §3.1 and §6.2. A `.gitignore` change is revalidated before content reads.
//! Depends on [`crate::plugins`] and the session policy. The engines do not import the host.
//! No installed host means no plugin process. A required plan with no host is refused.

use dctx_core::FileHandlingDecision;
use dctx_template::PluginPlan;

use super::Session;
use crate::domain::DctxError;
use crate::plugins::{participants, run_plan, Boundary, BoundaryInput, HookWarning, IgnoreSnap};

impl Session {
    /// Advisory warnings from the latest pack, snapshot or cache collection.
    pub fn hook_warnings(&self) -> &[HookWarning] {
        &self.warnings
    }

    /// Workers the process port started. Zero when the session has no plugin host.
    pub fn plugin_spawns(&self) -> u64 {
        self.plugins.as_ref().map(crate::plugins::PluginHost::spawns).unwrap_or(0)
    }

    /// Launch attempts, including a fail-closed platform refusal. Zero without a host.
    pub fn plugin_admissions(&self) -> u64 {
        self.plugins.as_ref().map(crate::plugins::PluginHost::admissions).unwrap_or(0)
    }

    /// Git recipe children. A refused platform recipe counts as zero.
    pub fn git_children(&self) -> u64 {
        self.plugins.as_ref().map(crate::plugins::PluginHost::git_children).unwrap_or(0)
    }

    pub(super) fn reset_hook_warnings(&mut self) {
        self.warnings.clear();
    }

    pub(super) fn hooks_before_pack(&mut self) -> Result<(), DctxError> {
        self.warnings.clear();
        self.run_before(Boundary::BeforePack)
    }

    pub(super) fn hooks_after_pack(&mut self) -> Result<(), DctxError> {
        self.run_after(Boundary::AfterPack)
    }

    pub(super) fn hooks_before_index(&mut self) -> Result<(), DctxError> {
        self.warnings.clear();
        self.run_before(Boundary::BeforeIndex)
    }

    pub(super) fn hooks_after_index(&mut self) -> Result<(), DctxError> {
        self.run_after(Boundary::AfterIndex)
    }

    pub(super) fn hooks_after_cache(&mut self) -> Result<(), DctxError> {
        self.run_after(Boundary::CacheGc)
    }

    fn run_before(&mut self, boundary: Boundary) -> Result<(), DctxError> {
        let Some(plan) = self.plugin_plan() else {
            return Ok(());
        };
        if let Some(error) = self.missing_host(&plan, boundary, false) {
            return error;
        }
        let included = self.gitignore_included()?;
        let epoch = self.epoch;
        let now = self.clock.now().millis();
        let before = self.observe_ignore();
        let spawned_before = self.plugin_spawns();
        let hook = self.invoke(&plan, boundary, included, epoch, now, false);
        let spawned = self.plugin_spawns() > spawned_before;
        let reload = self.reload_ignore(before, spawned);
        hook?;
        reload
    }

    fn run_after(&mut self, boundary: Boundary) -> Result<(), DctxError> {
        let Some(plan) = self.plugin_plan() else {
            return Ok(());
        };
        if let Some(error) = self.missing_host(&plan, boundary, true) {
            return error;
        }
        let included = self.gitignore_included()?;
        let epoch = self.epoch;
        let now = self.clock.now().millis();
        self.invoke(&plan, boundary, included, epoch, now, true)
    }

    fn plugin_plan(&self) -> Option<PluginPlan> {
        self.plan.as_ref().map(|plan| plan.plugins.clone())
    }

    /// `Some(Err)` when a required participant has no host. `Some(Ok)` records an advisory gap.
    fn missing_host(&mut self, plan: &PluginPlan, boundary: Boundary, committed: bool) -> Option<Result<(), DctxError>> {
        if self.plugins.is_some() {
            return None;
        }
        let people = participants(plan, boundary);
        if people.iter().any(|plugin| plugin.required) {
            return Some(Err(DctxError::Hook {
                hook: boundary.as_str(),
                reason: "plugin-host-missing",
                output_committed: committed,
            }));
        }
        if !people.is_empty() {
            self.warnings.push(HookWarning {
                hook: boundary.as_str(),
                reason: "plugin-host-missing",
                output_committed: committed,
            });
        }
        Some(Ok(()))
    }

    fn invoke(
        &mut self,
        plan: &PluginPlan,
        boundary: Boundary,
        included: bool,
        epoch: u64,
        now: u64,
        committed: bool,
    ) -> Result<(), DctxError> {
        let Some(mut host) = self.plugins.take() else {
            return Ok(());
        };
        let result = run_plan(
            &mut host,
            BoundaryInput { plan, boundary, included, epoch, now, committed },
            &mut self.warnings,
        );
        self.plugins = Some(host);
        result
    }

    fn observe_ignore(&self) -> IgnoreSnap {
        self.plugins.as_ref().map(crate::plugins::PluginHost::observe_gitignore).unwrap_or_else(IgnoreSnap::unknown)
    }

    fn gitignore_included(&self) -> Result<bool, DctxError> {
        Ok(self.policy_view().decide(".gitignore", false)? == FileHandlingDecision::Include)
    }

    fn reload_ignore(&mut self, before: IgnoreSnap, spawned: bool) -> Result<(), DctxError> {
        let after = self.observe_ignore();
        if before.same_file(&after) {
            return Ok(());
        }
        if before.is_unknown() && after.is_unknown() {
            return if spawned { Err(DctxError::PolicyUnavailable) } else { Ok(()) };
        }
        let Some(bytes) = after.bytes() else {
            return Err(DctxError::PolicyUnavailable);
        };
        let text = std::str::from_utf8(bytes).map_err(|_| DctxError::PolicyUnavailable)?;
        let Some(rules) = self.rules.as_mut() else {
            return Err(DctxError::PolicyUnavailable);
        };
        rules.local_ignore = Some(text.to_string());
        self.selection = rules.policy();
        self.epoch = self.epoch.saturating_add(1);
        Ok(())
    }
}
