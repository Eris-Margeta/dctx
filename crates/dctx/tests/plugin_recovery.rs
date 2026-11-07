// SPDX-License-Identifier: Apache-2.0
//! PP3/PP5 crash recovery through the facade's owner-only filesystem audit adapter.
//! Cuts surround real synced frames and observed file writes in a temporary directory.
//! The broker reopens the original action; uncertainty, conflicts and archives survive restart.

#![cfg(all(feature = "fs", unix))]
#![forbid(unsafe_code)]

#[path = "../src/plugins/audit.rs"]
mod audit;
#[path = "../../dctx-plugin-host/tests/common/mod.rs"]
mod fixture;

use audit::{AuditStorage, DurableAudit};
use dctx_plugin_host::fakes::{FakeFiles, FakeProcess};
use dctx_plugin_host::ports::{
    AuditKind, AuditPort, AuditRecord, AuditUnavailable, FileFault, FileIdentity, FilePort,
    MAX_RECOVERY_RECORDS, RecoveryState,
};
use dctx_plugin_host::{Authority, Binding, Broker, EntryStatus, HandleKind, Ledger, Reply};
use dctx_plugin_protocol::records::broker::FileStage;
use dctx_plugin_protocol::records::bytes::Chunk;
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::scalars::{Digest, ProjectPath};
use dctx_plugin_protocol::{EffectState, ErrorCode};
use std::cell::RefCell;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::rc::Rc;

const CAPACITY: usize = 64 * 1024;
const PRIVATE: &[u8] = b"private replacement content never in evidence";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cut {
    BeforeIntent,
    AfterIntent,
    AfterEffect,
    AfterCompletion,
}

struct Disk {
    storage: audit::disk::Storage,
    cut: Cut,
}
impl AuditStorage for Disk {
    fn read(&mut self, capacity: usize) -> Result<Vec<u8>, AuditUnavailable> {
        self.storage.read(capacity)
    }
    fn append_sync(&mut self, frame: &[u8]) -> Result<(), AuditUnavailable> {
        let text = std::str::from_utf8(frame).unwrap();
        let intent = text.contains("\tintent\t");
        let completion = text.contains("\tcompletion\t");
        assert!(
            !((intent && self.cut == Cut::BeforeIntent)
                || (completion && self.cut == Cut::AfterEffect)),
            "crash before sync"
        );
        self.storage.append_sync(frame)?;
        assert!(
            !((intent && self.cut == Cut::AfterIntent)
                || (completion && self.cut == Cut::AfterCompletion)),
            "crash after sync"
        );
        Ok(())
    }
}

struct Files {
    inner: Rc<RefCell<FakeFiles>>,
    directory: PathBuf,
}
impl FilePort for Files {
    fn identity(&mut self, path: &ProjectPath) -> Result<FileIdentity, FileFault> {
        self.inner.borrow_mut().identity(path)
    }
    fn read(
        &mut self,
        path: &ProjectPath,
        id: FileIdentity,
        offset: u64,
        len: u64,
    ) -> Result<Vec<u8>, FileFault> {
        self.inner.borrow_mut().read(path, id, offset, len)
    }
    fn replace(
        &mut self,
        path: &ProjectPath,
        id: FileIdentity,
        bytes: &[u8],
    ) -> Result<FileIdentity, FileFault> {
        let log = fs::read_to_string(self.directory.join("effects.audit")).unwrap();
        assert!(log.contains("\tintent\t"));
        let result = self.inner.borrow_mut().replace(path, id, bytes)?;
        let mut observed = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.directory.join("observed"))
            .unwrap();
        observed.write_all(b"effect\n").unwrap();
        observed.sync_all().unwrap();
        fs::write(self.directory.join("replacement"), bytes).unwrap();
        Ok(result)
    }
}

fn directory() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}
fn files() -> Rc<RefCell<FakeFiles>> {
    let mut files = FakeFiles::default();
    files.put(&fixture::path(), b"abc");
    Rc::new(RefCell::new(files))
}
fn observed(directory: &Path) -> usize {
    fs::read_to_string(directory.join("observed"))
        .unwrap_or_default()
        .lines()
        .count()
}
fn reopen(directory: &Path) -> DurableAudit<audit::disk::Storage> {
    DurableAudit::new(audit::disk::Storage::open(directory).unwrap(), CAPACITY).unwrap()
}
fn prepared<A: AuditPort>(
    audit: A,
    files: Rc<RefCell<FakeFiles>>,
    directory: &Path,
    binding: Binding,
) -> (Broker<A, Files, FakeProcess>, Request) {
    let mut authority = Authority::new(
        fixture::layers(fixture::all_caps()),
        fixture::included(),
        binding,
        0,
    );
    authority.initialize().unwrap();
    let mut broker = Broker::new(
        authority,
        Ledger::new(64),
        fixture::budget(8192, 8),
        8192,
        (
            audit,
            Files {
                inner: files,
                directory: directory.to_owned(),
            },
            FakeProcess::default(),
        ),
    );
    let handle = broker
        .register(HandleKind::File {
            path: fixture::path(),
        })
        .unwrap();
    let pre = broker.issue_precondition(&handle).unwrap();
    let Reply::Staged { upload, .. } = broker
        .handle(&Request::FileStage(FileStage {
            resource: handle.clone(),
            precondition: pre.clone(),
            upload: None,
            offset: 0,
            bytes: Chunk::new(PRIVATE).unwrap(),
            final_chunk: true,
        }))
        .unwrap()
    else {
        panic!("upload")
    };
    let request = fixture::replace_req(&handle, &pre, &upload, "a:1");
    (broker, request)
}

#[test]
fn synced_disk_crash_cuts_preserve_at_most_one_effect_and_conflicts() {
    for cut in [
        Cut::BeforeIntent,
        Cut::AfterIntent,
        Cut::AfterEffect,
        Cut::AfterCompletion,
    ] {
        let directory = directory();
        let path = directory.path();
        let files = files();
        let disk = Disk {
            storage: audit::disk::Storage::open(path).unwrap(),
            cut,
        };
        let (mut first, request) = prepared(
            DurableAudit::new(disk, CAPACITY).unwrap(),
            files.clone(),
            path,
            fixture::binding("op:1"),
        );
        assert!(catch_unwind(AssertUnwindSafe(|| first.handle(&request))).is_err());
        drop(first);
        let count = observed(path);
        assert_eq!(
            count,
            usize::from(matches!(cut, Cut::AfterEffect | Cut::AfterCompletion))
        );
        let (mut second, _) = prepared(reopen(path), files.clone(), path, fixture::binding("op:1"));
        let result = second.handle(&request);
        if matches!(cut, Cut::BeforeIntent | Cut::AfterCompletion) {
            assert!(
                matches!(result, Ok(Reply::Receipt { replayed, .. }) if replayed == (cut == Cut::AfterCompletion))
            );
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.effect_state, EffectState::Unknown);
            assert!(!error.retryable());
            assert_eq!(observed(path), count);
            let (key, _) = second.replay_key(&request).unwrap();
            assert!(second.rotate(&key).is_err());
        }
        let Request::FileReplace(mut changed) = request.clone() else {
            unreachable!()
        };
        changed.upload = fixture::token("changed:args");
        assert_eq!(
            second
                .handle(&Request::FileReplace(changed))
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert!(observed(path) <= 1);
        if matches!(cut, Cut::BeforeIntent | Cut::AfterCompletion) {
            let (key, _) = second.replay_key(&request).unwrap();
            let receipt = second.ledger().get(&key).unwrap().receipt.clone();
            second.rotate(&key).unwrap();
            drop(second);
            let (mut third, _) =
                prepared(reopen(path), files.clone(), path, fixture::binding("op:1"));
            assert!(
                matches!(third.handle(&request), Ok(Reply::Receipt { receipt: old, replayed: true, .. }) if old == receipt)
            );
            assert!(third.ledger().get(&key).unwrap().archived);
            assert_eq!(observed(path), 1);
            drop(third);
            let host = dctx::PluginHost::platform_with_audit(path, CAPACITY).unwrap();
            assert!(audit::open(path, CAPACITY).is_err());
            drop(host);
        } else {
            drop(second);
        }
        let bytes = fs::read(path.join("effects.audit")).unwrap();
        assert!(!bytes.windows(PRIVATE.len()).any(|part| part == PRIVATE));
        assert_eq!(
            fs::metadata(path.join("effects.audit"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn disk_partial_and_unknown_require_reconciliation_before_archival() {
    for state in [RecoveryState::Partial, RecoveryState::Unknown] {
        let directory = directory();
        let path = directory.path();
        let files = files();
        let disk = Disk {
            storage: audit::disk::Storage::open(path).unwrap(),
            cut: Cut::AfterEffect,
        };
        let (mut first, request) = prepared(
            DurableAudit::new(disk, CAPACITY).unwrap(),
            files.clone(),
            path,
            fixture::binding("op:1"),
        );
        assert!(catch_unwind(AssertUnwindSafe(|| first.handle(&request))).is_err());
        drop(first);
        let mut audit = reopen(path);
        let mut evidence = audit.recover(MAX_RECOVERY_RECORDS).unwrap().remove(0);
        evidence.state = state;
        let record = AuditRecord {
            kind: AuditKind::Completion,
            plugin: fixture::binding("op:1").plugin,
            invocation: evidence.key.invocation.clone(),
            epoch: 0,
            method: "file.replace",
            handle: None,
            args: Some(evidence.args),
            bytes: 0,
            receipt: Some(evidence.receipt.clone()),
            reason: state.as_str(),
        };
        audit.append_effect(&record, &evidence).unwrap();
        assert_eq!(audit.uncertain().len(), 1);
        drop(audit);
        let (mut second, _) = prepared(reopen(path), files.clone(), path, fixture::binding("op:1"));
        assert!(!second.handle(&request).unwrap_err().retryable());
        assert_eq!(
            second.ledger().get(&evidence.key).unwrap().status,
            if state == RecoveryState::Partial {
                EntryStatus::Partial
            } else {
                EntryStatus::Unknown
            }
        );
        assert!(second.rotate(&evidence.key).is_err());
        assert!(second.reconcile(&evidence.key, false).is_err());
        second.reconcile(&evidence.key, true).unwrap();
        second.rotate(&evidence.key).unwrap();
        drop(second);
        let (mut third, _) = prepared(reopen(path), files, path, fixture::binding("op:1"));
        assert!(matches!(
            third.handle(&request),
            Ok(Reply::Receipt { replayed: true, .. })
        ));
        assert_eq!(observed(path), 1);
    }
}

#[test]
fn disk_retains_full_keys_and_allocates_distinct_receipts_across_restarts() {
    let directory = directory();
    let path = directory.path();
    let files = files();
    let mut receipts = std::collections::BTreeSet::new();
    for field in 0..5 {
        let mut context = fixture::binding("op:1");
        match field {
            1 => context.artifact = Digest::new(&format!("sha256:{}", "b".repeat(64))).unwrap(),
            2 => context.root = fixture::token("project:2"),
            3 => context.invocation = fixture::token("op:2"),
            _ => {}
        }
        let (mut broker, mut request) = prepared(reopen(path), files.clone(), path, context);
        if field == 4 {
            let Request::FileReplace(replace) = &mut request else {
                unreachable!()
            };
            replace.action_id = fixture::token("a:2");
        }
        let Reply::Receipt {
            receipt,
            replayed: false,
            ..
        } = broker.handle(&request).unwrap()
        else {
            panic!("new effect")
        };
        assert!(receipts.insert(receipt));
        let (key, _) = broker.replay_key(&request).unwrap();
        broker.rotate(&key).unwrap();
    }
    assert_eq!(observed(path), 5);
    let (mut broker, request) = prepared(reopen(path), files, path, fixture::binding("op:1"));
    assert!(matches!(
        broker.handle(&request),
        Ok(Reply::Receipt { replayed: true, .. })
    ));
    assert_eq!(observed(path), 5);
    assert!(
        broker.audit_mut().recover(1).is_err(),
        "no truncated recovery snapshot"
    );
}

#[test]
fn torn_or_legacy_effect_evidence_never_becomes_an_empty_ledger() {
    for legacy in [false, true] {
        let directory = directory();
        let path = directory.path();
        let files = files();
        if legacy {
            let mut audit = reopen(path);
            let record = AuditRecord {
                kind: AuditKind::Intent,
                plugin: fixture::binding("op:1").plugin,
                invocation: fixture::token("op:1"),
                epoch: 0,
                method: "file.replace",
                handle: None,
                args: None,
                bytes: 0,
                receipt: Some(fixture::token("receipt:1")),
                reason: "intent",
            };
            audit.append(&record).unwrap();
        } else {
            drop(reopen(path));
            OpenOptions::new()
                .append(true)
                .open(path.join("effects.audit"))
                .unwrap()
                .write_all(b"torn")
                .unwrap();
        }
        let (mut broker, request) = prepared(reopen(path), files, path, fixture::binding("op:1"));
        assert_eq!(
            broker.handle(&request).unwrap_err().code,
            ErrorCode::AuditUnavailable
        );
        assert_eq!(observed(path), 0);
    }
}
