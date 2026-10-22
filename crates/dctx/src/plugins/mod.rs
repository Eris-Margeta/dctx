//! Facade plugin host: DTS plans sequenced over host and platform ports.
//! Implements PLUGINS.md §3.1 and §7.3 row PL-F. Engines stay unaware of the host.
//! The `plugin-runtime` feature enables host/platform ports; otherwise only refusal stubs compile.
//! A session with no [`PluginHost`] discovers no plugin and starts no process.

#[cfg(all(feature = "plugin-runtime", any(all(feature = "fs", unix), test)))]
mod audit;
#[cfg(feature = "plugin-runtime")]
mod boundary;
#[cfg(feature = "plugin-runtime")]
mod plan;
#[cfg(feature = "plugin-runtime")]
mod ports;
#[cfg(not(feature = "plugin-runtime"))]
mod disabled;

#[cfg(feature = "plugin-runtime")]
use std::collections::BTreeMap;
#[cfg(feature = "plugin-runtime")]
use std::sync::{Arc, Mutex};

#[cfg(feature = "plugin-runtime")]
use crate::domain::DctxError;

#[cfg(feature = "plugin-runtime")]
pub(crate) use boundary::{run_plan, BoundaryInput};
#[cfg(feature = "plugin-runtime")]
pub(crate) use plan::participants;
#[cfg(feature = "plugin-runtime")]
pub(crate) use ports::IgnoreSnap;
#[cfg(not(feature = "plugin-runtime"))]
pub(crate) use disabled::participants;
#[cfg(not(feature = "plugin-runtime"))]
pub use disabled::PluginHost;

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
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
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
#[cfg(feature = "plugin-runtime")]
#[derive(Clone)]
pub struct PluginHost {
    inner: Arc<Mutex<ports::PortBody>>,
    effects: BTreeMap<(String, Boundary), Vec<HookEffect>>,
    next: Arc<Mutex<u64>>,
    confined: bool,
    authority: Arc<plan::AuthoritySource>,
}

#[cfg(feature = "plugin-runtime")]
impl PluginHost {
    /// In-memory ports with explicit consent for the bundled Git fixture only.
    /// Recipe runs count as git children and do not exec; templates cannot extend this consent.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn fake() -> Self {
        Self::from_body(ports::PortBody::fake(), true)
    }

    /// Platform confinement adapter without an audit location; all audit admission fails closed.
    /// Use `platform_with_audit` with the `fs` feature to supply durable storage.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn platform() -> Self {
        Self::from_body(ports::PortBody::platform(None), false)
    }

    /// Platform ports with a bounded, durable audit under an explicit private directory.
    /// The directory must already exist with owner-only permissions and trusted ancestors.
    /// Capacity is in bytes (at most 64 MiB); full logs refuse effects without deleting evidence.
    /// Unsupported owner-only filesystem adapters return `AuditUnavailable`.
    #[cfg(feature = "fs")]
    /// # Errors
    /// Propagates audit-directory validation, opening, or durable recovery failures.
    pub fn platform_with_audit(
        directory: &std::path::Path,
        capacity: usize,
    ) -> Result<Self, dctx_plugin_host::ports::AuditUnavailable> {
        #[cfg(unix)]
        {
            let audit = audit::open(directory, capacity)?;
            Ok(Self::from_body(ports::PortBody::platform(Some(audit)), false))
        }
        #[cfg(not(unix))]
        {
            let _ = (directory, capacity);
            Err(dctx_plugin_host::ports::AuditUnavailable)
        }
    }

    /// Inject current trusted identity, manifest ask, consent, organization, host and adapter grants,
    /// and selection. Called at initialization and at admission and commit of every broker request.
    /// The source must return a coherent current snapshot, bind the supplied invocation, and advance
    /// `Binding::generation` on policy changes (including the supplied session policy epoch).
    /// Root, session, namespace and artifact are trusted caller inputs, never template values.
    /// Defaults authorize only the in-memory bundled fixture; platform grants default to empty.
    #[allow(clippy::return_self_not_must_use, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn with_authority_source<F>(mut self, source: F) -> Self
    where
        F: Fn(&str, &dctx_plugin_protocol::records::scalars::Token, u64)
                -> Result<(dctx_plugin_host::Binding, dctx_plugin_host::GrantLayers, dctx_plugin_host::Selection), dctx_plugin_host::HostRefusal>
            + Send + Sync + 'static,
    {
        self.authority = Arc::new(source);
        self
    }

    /// Script `effects` for `plugin` at `boundary`. Later calls replace that pair.
    #[allow(clippy::return_self_not_must_use, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn on(mut self, plugin: impl Into<String>, boundary: Boundary, effects: Vec<HookEffect>) -> Self {
        self.effects.insert((plugin.into(), boundary), effects);
        self
    }

    /// Seeds a fake project file. The platform file port cannot store bytes.
    /// # Errors
    /// Refuses a poisoned plugin-port lock or a seed rejected by the underlying port.
    pub fn seed(self, path: &str, bytes: &[u8]) -> Result<Self, DctxError> {
        self.inner.lock().map_err(|_| DctxError::invalid("plugin port"))?.seed(path, bytes)?;
        Ok(self)
    }

    /// Workers the process port actually started.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn spawns(&self) -> u64 {
        self.inner.lock().map_or(0, |body| body.spawns())
    }

    /// Launch attempts, including fail-closed platform refusals.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn admissions(&self) -> u64 {
        self.inner.lock().map_or(0, |body| body.admissions())
    }

    /// Git recipe children. A refused platform recipe counts as zero.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn git_children(&self) -> u64 {
        self.inner.lock().map_or(0, |body| body.git_children())
    }

    pub(crate) fn confined(&self) -> bool {
        self.confined
    }

    pub(crate) fn observe_gitignore(&self) -> IgnoreSnap {
        self.inner.lock().map_or_else(|_| IgnoreSnap::unknown(), |mut body| body.observe_gitignore())
    }

    pub(crate) fn effects_for(&self, plugin: &str, boundary: Boundary) -> Vec<HookEffect> {
        self.effects.get(&(plugin.to_string(), boundary)).cloned().unwrap_or_default()
    }

    pub(crate) fn next_request(&mut self) -> Result<dctx_plugin_protocol::records::scalars::RequestId, dctx_plugin_host::HostRefusal> {
        let next = self.next_id()?;
        dctx_plugin_protocol::records::scalars::RequestId::new(&format!("p:{next}")).map_err(|_| ports::bad_token())
    }

    pub(crate) fn next_token(&mut self, prefix: &str) -> Result<dctx_plugin_protocol::records::scalars::Token, dctx_plugin_host::HostRefusal> {
        let next = self.next_id()?;
        dctx_plugin_protocol::records::scalars::Token::new(&format!("{prefix}-{next}")).map_err(|_| ports::bad_token())
    }

    fn next_id(&self) -> Result<u64, dctx_plugin_host::HostRefusal> {
        let mut next = self.next.lock().map_err(|_| ports::bad_token())?;
        *next = next.checked_add(1).ok_or(dctx_plugin_host::HostRefusal::limit("identity-exhausted"))?;
        Ok(*next)
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
        Self {
            inner: Arc::new(Mutex::new(body)), effects: BTreeMap::new(), next: Arc::new(Mutex::new(0)), confined,
            authority: Arc::new(move |plugin, invocation, epoch| plan::fixture_authority(plugin, invocation, epoch, confined)),
        }
    }
}

#[cfg(feature = "plugin-runtime")]
#[allow(clippy::missing_fields_in_debug, reason = "Debug intentionally reports safe host counters and confinement only; port internals, effects and authority closures stay opaque.")]
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

/// The invocation binding accompanies every scripted request and its registered handles.
#[cfg(feature = "plugin-runtime")]
pub(crate) struct RequestGate<'a> {
    plugin: &'a dctx_template::PluginBinding,
    binding: dctx_plugin_host::Binding,
    grant: Vec<dctx_plugin_protocol::records::capability::Capability>,
    included: bool,
    epoch: u64,
}

#[cfg(feature = "plugin-runtime")]
impl<'a> RequestGate<'a> {
    pub(crate) fn new(plugin: &'a dctx_template::PluginBinding, authority: &dctx_plugin_host::Authority, included: bool, epoch: u64) -> Self {
        Self { plugin, binding: authority.binding().clone(), grant: authority.effective().to_vec(), included, epoch }
    }

    fn current(&self, host: &PluginHost) -> Result<dctx_plugin_host::Authority, dctx_plugin_host::HostRefusal> {
        let mut authority = plan::authority(host, self.plugin, &self.binding.invocation, self.included, self.epoch)?;
        if authority.binding() != &self.binding {
            return Err(dctx_plugin_host::HostRefusal::denied("binding-mismatch"));
        }
        authority.configure(&self.grant);
        Ok(authority)
    }

    pub(crate) fn call(
        &self, session: &mut ports::LiveSession, host: &mut PluginHost,
        request: dctx_plugin_protocol::records::requests::Request, need: dctx_plugin_host::Need, now: u64,
    ) -> Result<dctx_plugin_host::Reply, dctx_plugin_host::HostRefusal> {
        let mut current = self.current(host)?;
        Self::check_selection(&current, &need)?;
        let admission = current.admit(&self.binding, need)?;
        session.receive(host.next_request()?, request, now)?;
        let mut current = self.current(host)?;
        Self::check_selection(&current, &admission.need)?;
        current.commit(&admission)?;
        *session.broker_mut().authority_mut() = current;
        match session.serve_next(now) {
            Some((_, result)) => result,
            None => Err(ports::bad_token()),
        }
    }

    fn check_selection(authority: &dctx_plugin_host::Authority, need: &dctx_plugin_host::Need) -> Result<(), dctx_plugin_host::HostRefusal> {
        use dctx_plugin_host::Need;
        match need {
            Need::FileRead { path, .. } | Need::FileWrite { path, .. } | Need::GitAddIgnore { path } => authority.check_content(path),
            Need::OutputRead { .. } => Ok(()),
        }
    }
}
