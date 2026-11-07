// SPDX-License-Identifier: Apache-2.0
//! Deterministic fake ports with spy counters for host property tests and facade (PL-F) tests.
//! Implements the "fake process, filesystem and audit ports" of PLUGINS.md §7.2 PL-H and the
//! properties.md rule that counts are spy observations. No real IO, threads or wall-clock time.
//! Depends on ports; used by tests/ and by downstream crates that need a host without PL-A.

use std::cell::Cell;
use std::collections::BTreeMap;

use dctx_plugin_protocol::records::scalars::ProjectPath;

use crate::domain::liveness::ProcessEvidence;
use crate::ports::{
    AuditKind, AuditPort, AuditRecord, AuditUnavailable, Clock, FileFault, FileIdentity, FilePort,
    ProcessId, ProcessPort, Recipe, RecipeOutcome, RecipePort, SpawnFault,
};

/// In-memory audit log; `available` and `capacity` inject outages and full logs.
#[derive(Clone, Debug)]
pub struct FakeAudit {
    /// Appended records.
    pub records: Vec<AuditRecord>,
    /// Whether appends succeed.
    pub available: bool,
    /// Most records retained before the log is full.
    pub capacity: usize,
    /// Fail the next append of this kind once (e.g. completion after an effect).
    pub fail_next: Option<AuditKind>,
}

impl Default for FakeAudit {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            available: true,
            capacity: 1024,
            fail_next: None,
        }
    }
}

impl FakeAudit {
    /// Number of records of `kind`.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn count(&self, kind: AuditKind) -> usize {
        self.records.iter().filter(|r| r.kind == kind).count()
    }
}

impl AuditPort for FakeAudit {
    fn append(&mut self, record: &AuditRecord) -> Result<(), AuditUnavailable> {
        if self.fail_next == Some(record.kind) {
            self.fail_next = None;
            return Err(AuditUnavailable);
        }
        if !self.available || self.records.len() >= self.capacity {
            return Err(AuditUnavailable);
        }
        self.records.push(record.clone());
        Ok(())
    }
}

/// In-memory project files with read/identity/replace spies and injectable races.
#[derive(Clone, Debug, Default)]
pub struct FakeFiles {
    files: BTreeMap<ProjectPath, (FileIdentity, Vec<u8>)>,
    next_object: u64,
    /// `identity` calls (opening/hashing evidence).
    pub identities: usize,
    /// Successful content reads.
    pub reads: usize,
    /// Committed replacements.
    pub replaces: usize,
    /// Make the next replace report an uncertain outcome after writing.
    pub uncertain_next_replace: bool,
}

impl FakeFiles {
    /// Creates or overwrites `path` with `bytes`, as an outside writer would.
    pub fn put(&mut self, path: &ProjectPath, bytes: &[u8]) {
        let version = self.files.get(path).map_or(1, |(id, _)| id.version + 1);
        let object = if let Some((id, _)) = self.files.get(path) {
            id.object
        } else {
            self.next_object += 1;
            self.next_object
        };
        let identity = FileIdentity {
            object,
            version,
            size: bytes.len() as u64,
        };
        self.files.insert(path.clone(), (identity, bytes.to_vec()));
    }

    /// Current content of `path`.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn content(&self, path: &ProjectPath) -> Option<&[u8]> {
        self.files.get(path).map(|(_, bytes)| bytes.as_slice())
    }

    fn checked(
        &self,
        path: &ProjectPath,
        expected: FileIdentity,
    ) -> Result<&(FileIdentity, Vec<u8>), FileFault> {
        let entry = self.files.get(path).ok_or(FileFault::Missing)?;
        if entry.0 == expected {
            Ok(entry)
        } else {
            Err(FileFault::Changed)
        }
    }
}

impl FilePort for FakeFiles {
    fn identity(&mut self, path: &ProjectPath) -> Result<FileIdentity, FileFault> {
        self.identities += 1;
        self.files
            .get(path)
            .map(|(id, _)| *id)
            .ok_or(FileFault::Missing)
    }

    fn read(
        &mut self,
        path: &ProjectPath,
        expected: FileIdentity,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, FileFault> {
        let (_, bytes) = self.checked(path, expected)?;
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        let end = usize::try_from(offset.saturating_add(length))
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        let chunk = bytes[start..end].to_vec();
        self.reads += 1;
        Ok(chunk)
    }

    fn replace(
        &mut self,
        path: &ProjectPath,
        expected: FileIdentity,
        bytes: &[u8],
    ) -> Result<FileIdentity, FileFault> {
        self.checked(path, expected)?;
        self.put(path, bytes);
        self.replaces += 1;
        if std::mem::take(&mut self.uncertain_next_replace) {
            return Err(FileFault::Uncertain);
        }
        self.files
            .get(path)
            .map(|(id, _)| *id)
            .ok_or(FileFault::Uncertain)
    }
}

/// Scripted worker process: evidence, reaping and recipe outcomes are set by the test.
#[derive(Clone, Debug)]
pub struct FakeProcess {
    /// Whether a confinement provider exists.
    pub platform_supported: bool,
    /// Evidence returned by `evidence`.
    pub evidence: ProcessEvidence,
    /// Whether the OS can reap the tree.
    pub reapable: bool,
    /// Outcome of the next recipe run.
    pub recipe: RecipeOutcome,
    /// Spawn count.
    pub spawns: usize,
    /// Force count.
    pub forced: usize,
    /// Successful reaps.
    pub reaped: usize,
    /// Recipes run.
    pub recipes: Vec<Recipe>,
}

impl Default for FakeProcess {
    fn default() -> Self {
        Self {
            platform_supported: true,
            evidence: ProcessEvidence::Alive { activity: Some(1) },
            reapable: true,
            recipe: RecipeOutcome::Succeeded,
            spawns: 0,
            forced: 0,
            reaped: 0,
            recipes: Vec::new(),
        }
    }
}

impl ProcessPort for FakeProcess {
    fn spawn(&mut self) -> Result<ProcessId, SpawnFault> {
        if !self.platform_supported {
            return Err(SpawnFault::UnsupportedPlatform);
        }
        self.spawns += 1;
        Ok(ProcessId(self.spawns as u64))
    }

    fn evidence(&mut self, _process: ProcessId) -> ProcessEvidence {
        self.evidence
    }

    fn force(&mut self, _process: ProcessId) {
        self.forced += 1;
        self.evidence = ProcessEvidence::Exited;
    }

    fn reap(&mut self, _process: ProcessId) -> bool {
        if self.reapable {
            self.reaped += 1;
        }
        self.reapable
    }
}

impl RecipePort for FakeProcess {
    fn run(&mut self, recipe: &Recipe) -> RecipeOutcome {
        self.recipes.push(recipe.clone());
        self.recipe
    }
}

/// Manually advanced monotonic clock.
#[derive(Debug, Default)]
pub struct FakeClock(Cell<u64>);

impl FakeClock {
    /// Advances by `ms`.
    pub fn advance(&self, ms: u64) {
        self.0.set(self.0.get().saturating_add(ms));
    }
}

impl Clock for FakeClock {
    fn now(&self) -> u64 {
        self.0.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_detect_outside_changes() {
        let mut files = FakeFiles::default();
        let path = ProjectPath::new(".gitignore").expect("path");
        files.put(&path, b"abc");
        let id = files.identity(&path).expect("identity");
        assert_eq!(files.read(&path, id, 1, 5).expect("read"), b"bc");
        files.put(&path, b"abcd");
        assert_eq!(files.read(&path, id, 0, 1), Err(FileFault::Changed));
        assert_eq!(files.reads, 1);
    }

    #[test]
    fn audit_fails_once_by_kind() {
        let mut audit = FakeAudit {
            fail_next: Some(AuditKind::Completion),
            ..FakeAudit::default()
        };
        let record = |kind| AuditRecord {
            kind,
            plugin: dctx_plugin_protocol::records::scalars::PluginId::new("dctx/git").expect("id"),
            invocation: dctx_plugin_protocol::records::scalars::Token::new("op:1").expect("op"),
            epoch: 0,
            method: "file.replace",
            handle: None,
            args: None,
            bytes: 0,
            receipt: None,
            reason: "ok",
        };
        assert!(audit.append(&record(AuditKind::Intent)).is_ok());
        assert!(audit.append(&record(AuditKind::Completion)).is_err());
        assert!(audit.append(&record(AuditKind::Completion)).is_ok());
    }
}
