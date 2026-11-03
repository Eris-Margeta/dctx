// SPDX-License-Identifier: Apache-2.0
//! IO ports the host domain needs: durable audit, race-safe files, worker processes and a monotonic clock.
//! Implements the port boundary of PLUGINS.md §3.1 (process/confinement/audit/effect ports) and §4.4.
//! Real adapters live in PL-A (`dctx-plugin-platform`) and the facade; `fakes` provides deterministic
//! spies. Depends on protocol scalars and domain types; used by the broker and supervisor services.

use dctx_plugin_protocol::records::scalars::{PluginId, ProjectPath, Token};

use crate::domain::binding::{ActionKey, ArgDigest};

/// Kind of a host audit record (PLUGINS.md §4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditKind {
    /// Durable intent before an effect.
    Intent,
    /// Completion after an effect.
    Completion,
    /// A content delivery, recorded before bytes leave the host.
    Delivery,
    /// A refused broker action.
    Refusal,
    /// Lifecycle or liveness transition.
    Lifecycle,
}

/// Safe audit metadata; never source bytes, replacement bodies, arguments or stderr.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditRecord {
    /// Record kind.
    pub kind: AuditKind,
    /// Plugin ID.
    pub plugin: PluginId,
    /// Invocation.
    pub invocation: Token,
    /// Policy epoch.
    pub epoch: u64,
    /// Method or transition name.
    pub method: &'static str,
    /// Resource handle, if any.
    pub handle: Option<Token>,
    /// Action argument digest, if any.
    pub args: Option<ArgDigest>,
    /// Byte count delivered or written.
    pub bytes: u64,
    /// Linked receipt.
    pub receipt: Option<Token>,
    /// Safe status or reason name.
    pub reason: &'static str,
}

/// Audit could not durably append.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditUnavailable;

/// Maximum number of retained recovery transitions, including archived evidence.
pub const MAX_RECOVERY_RECORDS: usize = 4096;

/// Durable action transition; an intent alone never proves whether execution happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryState {
    /// Intent synced before execution.
    Pending,
    /// Successful completion synced.
    Committed,
    /// Some of the effect happened; never automatically retry.
    Partial,
    /// Outcome cannot be established; never automatically retry.
    Unknown,
    /// Trusted evidence proves that the effect did not run.
    NotRun,
    /// Trusted reconciliation proves the effect committed.
    Reconciled,
    /// Committed evidence archived without discarding its replay key.
    Archived,
}

impl RecoveryState {
    /// Stable, source-free storage tag.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Committed => "committed",
            Self::Partial => "partial",
            Self::Unknown => "unknown",
            Self::NotRun => "not-run",
            Self::Reconciled => "reconciled",
            Self::Archived => "archived",
        }
    }

    /// Validates a durable transition tag; unknown tags fail closed.
    /// # Errors
    /// Returns `AuditUnavailable` for an unknown recovery-state tag.
    pub fn parse(value: &str) -> Result<Self, AuditUnavailable> {
        match value {
            "pending" => Ok(Self::Pending),
            "committed" => Ok(Self::Committed),
            "partial" => Ok(Self::Partial),
            "unknown" => Ok(Self::Unknown),
            "not-run" => Ok(Self::NotRun),
            "reconciled" => Ok(Self::Reconciled),
            "archived" => Ok(Self::Archived),
            _ => Err(AuditUnavailable),
        }
    }
}

/// Typed recovery metadata; contains digests and opaque identities, never source bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryRecord {
    /// Full host-bound action key.
    pub key: ActionKey,
    /// Original argument digest.
    pub args: ArgDigest,
    /// Original receipt, also retained after archival.
    pub receipt: Token,
    /// Durable transition.
    pub state: RecoveryState,
}

/// Durable, owner-only local audit log.
pub trait AuditPort {
    /// Appends `record` durably, or reports that logging is unavailable or full.
    /// # Errors
    /// Returns `AuditUnavailable` when the record cannot be durably appended, including a full log.
    fn append(&mut self, record: &AuditRecord) -> Result<(), AuditUnavailable>;

    /// Atomically syncs an audit event and its recovery metadata.
    /// The default is for volatile adapters only; persistent adapters must override it.
    /// # Errors
    /// Returns `AuditUnavailable` when the event and recovery transition cannot be durably recorded.
    fn append_effect(&mut self, record: &AuditRecord, _recovery: &RecoveryRecord) -> Result<(), AuditUnavailable> {
        self.append(record)
    }

    /// Reads all retained transitions in order, including archives; never silently truncates.
    /// Refuses damaged, legacy unbound or over-limit evidence. Volatile adapters return empty.
    /// # Errors
    /// Returns `AuditUnavailable` for unreadable, damaged, legacy unbound, or over-limit recovery evidence.
    fn recover(&mut self, _limit: usize) -> Result<Vec<RecoveryRecord>, AuditUnavailable> {
        Ok(Vec::new())
    }
}

/// A host-checked file object identity and version; equal identities mean an unchanged file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    /// Stable object identity (inode/file ID).
    pub object: u64,
    /// Content version or change counter.
    pub version: u64,
    /// Size in bytes.
    pub size: u64,
}

/// File port failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileFault {
    /// No such file (creation is a separate grant).
    Missing,
    /// The object identity changed, or a symlink/hardlink/reparse race was detected.
    Changed,
    /// The replacement may or may not have happened.
    Uncertain,
}

/// Race-safe access to named project files through host-opened root handles.
pub trait FilePort {
    /// Current identity of `path`.
    /// # Errors
    /// Returns `FileFault` if the file is missing or its identity cannot be safely established.
    fn identity(&mut self, path: &ProjectPath) -> Result<FileIdentity, FileFault>;
    /// Reads `length` bytes at `offset` if the file still has `expected` identity.
    /// # Errors
    /// Returns `FileFault` if the file is missing or changed, or the identity-checked read cannot complete.
    fn read(&mut self, path: &ProjectPath, expected: FileIdentity, offset: u64, length: u64) -> Result<Vec<u8>, FileFault>;
    /// Atomically replaces `path` if it still has `expected` identity; returns the new identity.
    /// # Errors
    /// Returns `FileFault` if the file is missing/changed or replacement has an uncertain outcome.
    fn replace(&mut self, path: &ProjectPath, expected: FileIdentity, bytes: &[u8]) -> Result<FileIdentity, FileFault>;
}

/// A registered program recipe; never argv text or a shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recipe {
    /// `git.add-ignore`: stage exactly the ignore file under its checked identity.
    GitAddIgnore {
        /// Ignore file.
        path: ProjectPath,
        /// Identity the staged content must have.
        expected: FileIdentity,
    },
}

/// Outcome of a recipe run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecipeOutcome {
    /// Exited successfully.
    Succeeded,
    /// Exited with failure; nothing was changed.
    Failed,
    /// The program is not installed.
    Missing,
    /// Exited abnormally; the effect is unknown.
    Uncertain,
}

/// Opaque process identifier from the process port.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProcessId(pub u64);

/// Why the adapter could not start a confined worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnFault {
    /// No tested confinement provider; never an unrestricted fallback.
    UnsupportedPlatform,
    /// The executable failed revalidation or could not start.
    Failed,
}

/// Process tree supervision and registered recipe execution.
pub trait ProcessPort {
    /// Spawns the confined worker; confinement is established by the adapter before exec.
    /// # Errors
    /// Returns `SpawnFault` when confinement is unavailable or executable revalidation/spawning fails.
    fn spawn(&mut self) -> Result<ProcessId, SpawnFault>;
    /// Current OS evidence about the worker; unknown when the OS cannot tell.
    fn evidence(&mut self, process: ProcessId) -> crate::domain::liveness::ProcessEvidence;
    /// Forces termination of the worker and its descendants.
    fn force(&mut self, process: ProcessId);
    /// Attempts to reap the tree; false when the OS refuses or descendants remain.
    fn reap(&mut self, process: ProcessId) -> bool;
}

/// Registered program recipes (the brokered Git adapter); never argv text or a shell.
pub trait RecipePort {
    /// Runs `recipe` with a fixed executable identity, scrubbed environment and bounded output.
    fn run(&mut self, recipe: &Recipe) -> RecipeOutcome;
}

/// Injected monotonic clock in milliseconds; never wall-clock time.
pub trait Clock {
    /// Monotonic milliseconds.
    fn now(&self) -> u64;
}
