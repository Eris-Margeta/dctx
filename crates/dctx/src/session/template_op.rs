//! Template compile and load. An installed plan names the plugins a later hook boundary runs.
//! Implements DTS v2 compile against the session ceiling (decision 079, decision 080, decision 081).
//! Depends on `dctx-template`. Callers install a plan explicitly; compile does not write it.
//! Unknown capabilities stay [`crate::domain::DctxError::UnsupportedCapability`].

use dctx_template::ContextPlan;

use super::Session;
use crate::domain::DctxError;

impl Session {
    /// Compiles DTS source against this session's ceiling. No IO.
    pub fn compile_template(&self, source: &str) -> Result<ContextPlan, DctxError> {
        if !cfg!(feature = "templates") {
            return Err(DctxError::unsupported("templates"));
        }
        dctx_template::compile(source, &self.ceiling).map_err(DctxError::from_template)
    }

    /// Loads a compiled plan and re-checks it against the ceiling.
    pub fn load_template(&self, bytes: &[u8]) -> Result<ContextPlan, DctxError> {
        if !cfg!(feature = "templates") {
            return Err(DctxError::unsupported("templates"));
        }
        dctx_template::load(bytes, &self.ceiling).map_err(DctxError::from_template)
    }

    /// Installs `plan` and bumps the policy epoch. A pin does not keep the old epoch's grants.
    pub fn install_plan(&mut self, plan: ContextPlan) {
        self.plan = Some(plan);
        self.epoch = self.epoch.saturating_add(1);
    }

    /// Replaces the selection policy and bumps the policy epoch.
    pub fn revise_selection(&mut self, selection: dctx_core::selection::SelectionPolicy) {
        self.selection = selection;
        self.epoch = self.epoch.saturating_add(1);
    }
}
