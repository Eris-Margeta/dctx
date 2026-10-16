//! Facade plugin host: DTS plans sequenced over host and platform ports.
//! Implements PLUGINS.md §3.1 and §7.3 row PL-F. Engines stay unaware of the host.
//! Depends on `dctx-plugin-host`, `dctx-plugin-platform` and `dctx-template`.
//! A session with no [`PluginHost`] discovers no plugin and starts no process.

mod boundary;
mod effects;
mod plan;
mod ports;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::domain::DctxError;

pub(crate) use boundary::{run_plan, BoundaryInput};
pub(crate) use plan::participants;
pub(crate) use ports::IgnoreSnap;

/// Hook boundary the facade runs around an engine operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Boundary {
    /// After the pack plan is authorized, before the export is published.
    BeforePack,
    /// After the pack export is published.
    AfterPack,
    /// After selection is frozen, before index content reads.
    BeforeIndex,
    /// After the index generation is published.
    AfterIndex,
    /// After cache collection has committed.
    CacheGc,
}

impl Boundary {
    /// Wire name from the plugin contract.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BeforePack => "before-pack-output-written",
            Self::AfterPack => "after-pack-output-written",
            Self::BeforeIndex => "before-index",
            Self::AfterIndex => "after-index",
            Self::CacheGc => "cache-gc",
        }
    }
}

/// One scripted broker effect for a fake or platform port run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookEffect {
    /// `file.read` of `.gitignore`.
    ReadGitignore,
    /// Read, stage and `file.replace` `.gitignore` with these bytes.
    ReplaceGitignore(Vec<u8>),
    /// `program.run` `git.add-ignore` through the recipe port.
    StageGitignore,
    /// The worker fails the hook.
    Fail,
    /// The worker returns a warning.
    Warn,
}

/// Visible advisory hook result. Required failures are [`DctxError::Hook`] instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookWarning {
    /// Hook boundary name.
    pub hook: &'static str,
    /// Safe reason token.
    pub reason: &'static str,
    /// True when this warning was raised after the engine published.
    pub output_committed: bool,
}

/// Caller-supplied plugin ports. Library sessions leave this unset.
#[derive(Clone)]
pub struct PluginHost {
    inner: Arc<Mutex<ports::PortBody>>,
    effects: BTreeMap<(String, Boundary), Vec<HookEffect>>,
    next: u64,
    confined: bool,
}

impl PluginHost {
    /// In-memory audit, files and process. Recipe runs count as git children and do not exec.
    pub fn fake() -> Self {
        Self::from_body(ports::PortBody::fake(), true)
    }

    /// Platform confinement adapter. Launch and git recipes fail closed and spawn nothing.
    pub fn platform() -> Self {
        Self::from_body(ports::PortBody::platform(), false)
    }

    /// Script `effects` for `plugin` at `boundary`. Later calls replace that pair.
    pub fn on(mut self, plugin: impl Into<String>, boundary: Boundary, effects: Vec<HookEffect>) -> Self {
        self.effects.insert((plugin.into(), boundary), effects);
        self
    }

    /// Seeds a fake project file. The platform file port cannot store bytes.
    pub fn seed(self, path: &str, bytes: &[u8]) -> Result<Self, DctxError> {
        self.inner.lock().map_err(|_| DctxError::invalid("plugin port"))?.seed(path, bytes)?;
        Ok(self)
    }

    /// Workers the process port actually started.
    pub fn spawns(&self) -> u64 {
        self.inner.lock().map(|body| body.spawns()).unwrap_or(0)
    }

    /// Launch attempts, including fail-closed platform refusals.
    pub fn admissions(&self) -> u64 {
        self.inner.lock().map(|body| body.admissions()).unwrap_or(0)
    }

    /// Git recipe children. A refused platform recipe counts as zero.
    pub fn git_children(&self) -> u64 {
        self.inner.lock().map(|body| body.git_children()).unwrap_or(0)
    }

    pub(crate) fn confined(&self) -> bool {
        self.confined
    }

    pub(crate) fn observe_gitignore(&self) -> IgnoreSnap {
        self.inner.lock().map(|mut body| body.observe_gitignore()).unwrap_or_else(|_| IgnoreSnap::unknown())
    }

    pub(crate) fn effects_for(&self, plugin: &str, boundary: Boundary) -> Vec<HookEffect> {
        self.effects.get(&(plugin.to_string(), boundary)).cloned().unwrap_or_default()
    }

    pub(crate) fn next_request(&mut self) -> Result<dctx_plugin_protocol::records::scalars::RequestId, dctx_plugin_host::HostRefusal> {
        self.next = self.next.saturating_add(1);
        dctx_plugin_protocol::records::scalars::RequestId::new(&format!("p:{}", self.next)).map_err(|_| ports::bad_token())
    }

    pub(crate) fn next_token(&mut self, prefix: &str) -> Result<dctx_plugin_protocol::records::scalars::Token, dctx_plugin_host::HostRefusal> {
        self.next = self.next.saturating_add(1);
        dctx_plugin_protocol::records::scalars::Token::new(&format!("{prefix}-{}", self.next)).map_err(|_| ports::bad_token())
    }

    pub(crate) fn set_launch(&self, kind: dctx_plugin_platform::LaunchKind) {
        if let Ok(mut body) = self.inner.lock() {
            body.set_launch(kind);
        }
    }

    pub(crate) fn adapters(&self) -> ports::Adapters {
        ports::Adapters::new(Arc::clone(&self.inner))
    }

    fn from_body(body: ports::PortBody, confined: bool) -> Self {
        Self { inner: Arc::new(Mutex::new(body)), effects: BTreeMap::new(), next: 0, confined }
    }
}

impl std::fmt::Debug for PluginHost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginHost")
            .field("confined", &self.confined)
            .field("spawns", &self.spawns())
            .field("admissions", &self.admissions())
            .field("git_children", &self.git_children())
            .finish()
    }
}
