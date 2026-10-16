//! Real platform ports and fake host ports behind one broker.
//! Implements PLUGINS.md §7.3: the same hook corpus passes through both.
//! Depends on `dctx-plugin-host` ports and `dctx-plugin-platform`. The session runner owns the adapters.
//! The platform provider never becomes an unrestricted process.

use std::sync::{Arc, Mutex, MutexGuard};

use dctx_plugin_host::fakes::{FakeAudit, FakeFiles, FakeProcess};
use dctx_plugin_host::ports::{
    AuditPort, AuditRecord, AuditUnavailable, FileFault, FileIdentity, FilePort, ProcessId, ProcessPort, Recipe, RecipeOutcome,
    RecipePort, SpawnFault,
};
use dctx_plugin_host::{HostRefusal, HostSession};
use dctx_plugin_platform::{ConfinementProvider, LaunchKind, NativeProvider, PlatformError};
use dctx_plugin_protocol::records::scalars::ProjectPath;
use dctx_plugin_protocol::{ErrorCode, Phase};

use crate::domain::DctxError;

/// What the host could see of `.gitignore` without handing bytes to a plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IgnoreSnap {
    /// The file is not there.
    Absent,
    /// Identity and full bytes.
    Present { identity: FileIdentity, bytes: Vec<u8> },
    /// The port cannot say whether the file changed.
    Unknown,
}

impl IgnoreSnap {
    pub(crate) fn unknown() -> Self {
        Self::Unknown
    }

    pub(crate) fn same_file(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Absent, Self::Absent) => true,
            (Self::Present { identity: left, .. }, Self::Present { identity: right, .. }) => left == right,
            _ => false,
        }
    }

    pub(crate) fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Present { bytes, .. } => Some(bytes),
            _ => None,
        }
    }

    pub(crate) fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

enum Backend {
    Fake { files: FakeFiles, process: FakeProcess },
    Platform { provider: NativeProvider },
}

pub(crate) struct PortBody {
    audit: FakeAudit,
    backend: Backend,
    launch: LaunchKind,
    attempts: u64,
}

impl PortBody {
    pub(crate) fn fake() -> Self {
        Self {
            audit: FakeAudit::default(),
            backend: Backend::Fake { files: FakeFiles::default(), process: FakeProcess::default() },
            launch: LaunchKind::BundledGit,
            attempts: 0,
        }
    }

    pub(crate) fn platform() -> Self {
        Self {
            audit: FakeAudit::default(),
            backend: Backend::Platform { provider: NativeProvider },
            launch: LaunchKind::BundledGit,
            attempts: 0,
        }
    }

    pub(crate) fn set_launch(&mut self, kind: LaunchKind) {
        self.launch = kind;
    }

    pub(crate) fn spawns(&self) -> u64 {
        match &self.backend {
            Backend::Fake { process, .. } => u64::try_from(process.spawns).unwrap_or(u64::MAX),
            Backend::Platform { .. } => 0,
        }
    }

    pub(crate) fn admissions(&self) -> u64 {
        self.attempts
    }

    pub(crate) fn git_children(&self) -> u64 {
        match &self.backend {
            Backend::Fake { process, .. } => u64::try_from(process.recipes.len()).unwrap_or(u64::MAX),
            Backend::Platform { .. } => 0,
        }
    }

    pub(crate) fn seed(&mut self, path: &str, bytes: &[u8]) -> Result<(), DctxError> {
        let path = ProjectPath::new(path).map_err(|_| DctxError::invalid("plugin path"))?;
        match &mut self.backend {
            Backend::Fake { files, .. } => {
                files.put(&path, bytes);
                Ok(())
            }
            Backend::Platform { .. } => Err(DctxError::unsupported("plugin-file")),
        }
    }

    pub(crate) fn observe_gitignore(&mut self) -> IgnoreSnap {
        let path = match ProjectPath::new(".gitignore") {
            Ok(path) => path,
            Err(_) => return IgnoreSnap::Unknown,
        };
        match self.identity(&path) {
            Ok(identity) => match self.read_all(&path, identity) {
                Ok(bytes) => IgnoreSnap::Present { identity, bytes },
                Err(FileFault::Missing) => IgnoreSnap::Absent,
                Err(_) => IgnoreSnap::Unknown,
            },
            Err(FileFault::Missing) => IgnoreSnap::Absent,
            Err(_) => IgnoreSnap::Unknown,
        }
    }

    fn identity(&mut self, path: &ProjectPath) -> Result<FileIdentity, FileFault> {
        match &mut self.backend {
            Backend::Fake { files, .. } => files.identity(path),
            Backend::Platform { .. } => Err(FileFault::Uncertain),
        }
    }

    fn read_all(&mut self, path: &ProjectPath, identity: FileIdentity) -> Result<Vec<u8>, FileFault> {
        match &mut self.backend {
            Backend::Fake { files, .. } => files.read(path, identity, 0, identity.size),
            Backend::Platform { .. } => Err(FileFault::Uncertain),
        }
    }

    fn spawn(&mut self) -> Result<ProcessId, SpawnFault> {
        self.attempts = self.attempts.saturating_add(1);
        let kind = self.launch;
        match &mut self.backend {
            Backend::Fake { process, .. } => process.spawn(),
            Backend::Platform { provider } => match provider.launch(kind) {
                Err(PlatformError::Unsupported(_)) => Err(SpawnFault::UnsupportedPlatform),
                Err(_) => Err(SpawnFault::Failed),
                Ok(mut child) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    Err(SpawnFault::Failed)
                }
            },
        }
    }
}

pub(crate) fn bad_token() -> HostRefusal {
    HostRefusal::new(ErrorCode::InvalidMessage, Phase::Initialize, "token")
}

#[derive(Clone)]
struct Shared(Arc<Mutex<PortBody>>);

fn lock(shared: &Shared) -> Result<MutexGuard<'_, PortBody>, ()> {
    shared.0.lock().map_err(|_| ())
}

/// Audit, file, recipe and process adapters over one port body.
pub(crate) struct Adapters {
    /// Durable audit log.
    pub audit: AuditAdapter,
    /// Project files.
    pub files: FileAdapter,
    /// Program recipes.
    pub recipes: RecipeAdapter,
    /// Worker process.
    pub process: ProcessAdapter,
}

impl Adapters {
    pub(crate) fn new(inner: Arc<Mutex<PortBody>>) -> Self {
        Self {
            audit: AuditAdapter(Shared(Arc::clone(&inner))),
            files: FileAdapter(Shared(Arc::clone(&inner))),
            recipes: RecipeAdapter(Shared(Arc::clone(&inner))),
            process: ProcessAdapter(Shared(inner)),
        }
    }
}

/// Shared audit port.
#[derive(Clone)]
pub(crate) struct AuditAdapter(Shared);

/// Shared file port.
#[derive(Clone)]
pub(crate) struct FileAdapter(Shared);

/// Shared recipe port.
#[derive(Clone)]
pub(crate) struct RecipeAdapter(Shared);

/// Shared process port.
#[derive(Clone)]
pub(crate) struct ProcessAdapter(Shared);

/// One supervised worker over the shared ports.
pub(crate) type LiveSession = HostSession<AuditAdapter, FileAdapter, RecipeAdapter, ProcessAdapter>;

impl AuditPort for AuditAdapter {
    fn append(&mut self, record: &AuditRecord) -> Result<(), AuditUnavailable> {
        let mut body = lock(&self.0).map_err(|()| AuditUnavailable)?;
        body.audit.append(record)
    }
}

impl FilePort for FileAdapter {
    fn identity(&mut self, path: &ProjectPath) -> Result<FileIdentity, FileFault> {
        let mut body = lock(&self.0).map_err(|()| FileFault::Uncertain)?;
        body.identity(path)
    }

    fn read(&mut self, path: &ProjectPath, expected: FileIdentity, offset: u64, length: u64) -> Result<Vec<u8>, FileFault> {
        let mut body = lock(&self.0).map_err(|()| FileFault::Uncertain)?;
        match &mut body.backend {
            Backend::Fake { files, .. } => files.read(path, expected, offset, length),
            Backend::Platform { .. } => Err(FileFault::Uncertain),
        }
    }

    fn replace(&mut self, path: &ProjectPath, expected: FileIdentity, bytes: &[u8]) -> Result<FileIdentity, FileFault> {
        let mut body = lock(&self.0).map_err(|()| FileFault::Uncertain)?;
        match &mut body.backend {
            Backend::Fake { files, .. } => files.replace(path, expected, bytes),
            Backend::Platform { .. } => Err(FileFault::Uncertain),
        }
    }
}

impl RecipePort for RecipeAdapter {
    fn run(&mut self, recipe: &Recipe) -> RecipeOutcome {
        let Ok(mut body) = lock(&self.0) else {
            return RecipeOutcome::Uncertain;
        };
        match &mut body.backend {
            Backend::Fake { process, .. } => process.run(recipe),
            Backend::Platform { provider } => {
                let _ = provider.support(LaunchKind::GitRecipe);
                RecipeOutcome::Missing
            }
        }
    }
}

impl ProcessPort for ProcessAdapter {
    fn spawn(&mut self) -> Result<ProcessId, SpawnFault> {
        let mut body = lock(&self.0).map_err(|()| SpawnFault::Failed)?;
        body.spawn()
    }

    fn evidence(&mut self, process: ProcessId) -> dctx_plugin_host::ProcessEvidence {
        let Ok(mut body) = lock(&self.0) else {
            return dctx_plugin_host::ProcessEvidence::Unknown;
        };
        match &mut body.backend {
            Backend::Fake { process: fake, .. } => fake.evidence(process),
            Backend::Platform { .. } => dctx_plugin_host::ProcessEvidence::Unknown,
        }
    }

    fn force(&mut self, process: ProcessId) {
        let Ok(mut body) = lock(&self.0) else {
            return;
        };
        if let Backend::Fake { process: fake, .. } = &mut body.backend {
            fake.force(process);
        }
    }

    fn reap(&mut self, process: ProcessId) -> bool {
        let Ok(mut body) = lock(&self.0) else {
            return false;
        };
        match &mut body.backend {
            Backend::Fake { process: fake, .. } => fake.reap(process),
            Backend::Platform { .. } => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_plugin_host::ports::{ProcessPort, RecipePort};
    use dctx_plugin_protocol::records::scalars::ProjectPath;

    fn ignore() -> ProjectPath {
        ProjectPath::new(".gitignore").expect("path")
    }

    #[test]
    fn platform_launch_and_git_recipe_spawn_nothing() {
        let host = crate::plugins::PluginHost::platform();
        let mut adapters = host.adapters();
        assert!(adapters.process.spawn().is_err());
        let recipe = Recipe::GitAddIgnore {
            path: ignore(),
            expected: FileIdentity { object: 1, version: 1, size: 0 },
        };
        assert_eq!(adapters.recipes.run(&recipe), RecipeOutcome::Missing);
        assert_eq!(host.spawns(), 0);
        assert_eq!(host.git_children(), 0);
        assert_eq!(host.admissions(), 1);
        assert!(host.observe_gitignore().is_unknown());
    }

    #[test]
    fn fake_spawn_and_recipe_are_counted() {
        let host = crate::plugins::PluginHost::fake();
        let mut adapters = host.adapters();
        assert!(adapters.process.spawn().is_ok());
        let recipe = Recipe::GitAddIgnore {
            path: ignore(),
            expected: FileIdentity { object: 1, version: 1, size: 0 },
        };
        assert_eq!(adapters.recipes.run(&recipe), RecipeOutcome::Succeeded);
        assert_eq!(host.spawns(), 1);
        assert_eq!(host.git_children(), 1);
        assert_eq!(host.admissions(), 1);
    }
}
