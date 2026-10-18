//! PP3/PP5 restart witnesses using durable fake evidence and observed external effects.
//! Crash cuts exercise intent/effect/completion ordering through the broker, not execution counters.
//! Recovery retains complete keys, conflicts, uncertainty and archives; IO is injected.

mod common;

use common::*;
use dctx_plugin_host::fakes::{FakeFiles, FakeProcess};
use dctx_plugin_host::ports::{
    AuditKind, AuditPort, AuditRecord, AuditUnavailable, FileFault, FileIdentity, FilePort, MAX_RECOVERY_RECORDS, RecoveryRecord,
    RecoveryState,
};
use dctx_plugin_host::{ActionKey, ArgDigest, Authority, Binding, Broker, EntryStatus, HandleKind, Ledger, Lookup, Reply};
use dctx_plugin_protocol::records::broker::FileStage;
use dctx_plugin_protocol::records::bytes::Chunk;
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::scalars::{Digest, ProjectPath};
use dctx_plugin_protocol::{EffectState, ErrorCode};
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cut {
    BeforeIntent,
    AfterIntent,
    AfterEffect,
    AfterCompletion,
}

#[derive(Clone, Default)]
struct Audit {
    records: Rc<RefCell<Vec<RecoveryRecord>>>,
    cut: Option<Cut>,
    unavailable: bool,
}

impl AuditPort for Audit {
    fn append(&mut self, _: &AuditRecord) -> Result<(), AuditUnavailable> {
        Ok(())
    }
    fn append_effect(&mut self, record: &AuditRecord, recovery: &RecoveryRecord) -> Result<(), AuditUnavailable> {
        if record.kind == AuditKind::Intent && self.cut == Some(Cut::BeforeIntent) {
            panic!("crash before intent");
        }
        self.records.borrow_mut().push(recovery.clone());
        if (record.kind == AuditKind::Intent && self.cut == Some(Cut::AfterIntent))
            || (record.kind == AuditKind::Completion && self.cut == Some(Cut::AfterCompletion))
        {
            panic!("crash after sync");
        }
        Ok(())
    }
    fn recover(&mut self, limit: usize) -> Result<Vec<RecoveryRecord>, AuditUnavailable> {
        let records = self.records.borrow();
        if self.unavailable || records.len() > limit {
            return Err(AuditUnavailable);
        }
        Ok(records.clone())
    }
}

struct Files {
    inner: Rc<RefCell<FakeFiles>>,
    cut: bool,
}
impl FilePort for Files {
    fn identity(&mut self, path: &ProjectPath) -> Result<FileIdentity, FileFault> {
        self.inner.borrow_mut().identity(path)
    }
    fn read(&mut self, path: &ProjectPath, id: FileIdentity, offset: u64, len: u64) -> Result<Vec<u8>, FileFault> {
        self.inner.borrow_mut().read(path, id, offset, len)
    }
    fn replace(&mut self, path: &ProjectPath, id: FileIdentity, bytes: &[u8]) -> Result<FileIdentity, FileFault> {
        let result = self.inner.borrow_mut().replace(path, id, bytes);
        assert!(!self.cut, "crash after observed effect");
        result
    }
}

fn prepared(audit: Audit, files: Rc<RefCell<FakeFiles>>, cut: bool, context: Binding) -> (Broker<Audit, Files, FakeProcess>, Request) {
    let mut authority = Authority::new(layers(all_caps()), included(), context, 0);
    authority.initialize().unwrap();
    let mut broker =
        Broker::new(authority, Ledger::new(64), budget(8192, 8), 8192, (audit, Files { inner: files, cut }, FakeProcess::default()));
    let handle = broker.register(HandleKind::File { path: path() }).unwrap();
    let pre = broker.issue_precondition(&handle).unwrap();
    let Reply::Staged { upload, .. } = broker
        .handle(&Request::FileStage(FileStage {
            resource: handle.clone(),
            precondition: pre.clone(),
            upload: None,
            offset: 0,
            bytes: Chunk::new(b"private replacement").unwrap(),
            final_chunk: true,
        }))
        .unwrap()
    else {
        panic!("upload")
    };
    let request = replace_req(&handle, &pre, &upload, "a:1");
    (broker, request)
}

fn files() -> Rc<RefCell<FakeFiles>> {
    let mut files = FakeFiles::default();
    files.put(&path(), b"abc");
    Rc::new(RefCell::new(files))
}

#[test]
fn every_crash_cut_reopens_without_blind_replay() {
    for cut in [Cut::BeforeIntent, Cut::AfterIntent, Cut::AfterEffect, Cut::AfterCompletion] {
        let audit = Audit::default();
        let files = files();
        let (mut first, request) =
            prepared(Audit { cut: Some(cut), ..audit.clone() }, files.clone(), cut == Cut::AfterEffect, binding("op:1"));
        assert!(catch_unwind(AssertUnwindSafe(|| first.handle(&request))).is_err());
        drop(first);
        let observed = files.borrow().replaces;
        assert_eq!(observed, usize::from(matches!(cut, Cut::AfterEffect | Cut::AfterCompletion)));
        let (mut reopened, _) = prepared(audit.clone(), files.clone(), false, binding("op:1"));
        let reply = reopened.handle(&request);
        if matches!(cut, Cut::BeforeIntent | Cut::AfterCompletion) {
            assert!(matches!(reply, Ok(Reply::Receipt { replayed, .. }) if replayed == (cut == Cut::AfterCompletion)));
        } else {
            let error = reply.unwrap_err();
            assert_eq!(error.effect_state, EffectState::Unknown);
            assert!(!error.retryable());
            assert_eq!(files.borrow().replaces, observed);
            let (key, _) = reopened.replay_key(&request).unwrap();
            assert!(reopened.rotate(&key).is_err());
        }
        let Request::FileReplace(mut changed) = request.clone() else { unreachable!() };
        changed.upload = token("different:args");
        assert_eq!(reopened.handle(&Request::FileReplace(changed)).unwrap_err().code, ErrorCode::Conflict);
        assert!(files.borrow().replaces <= 1);
        if matches!(cut, Cut::BeforeIntent | Cut::AfterCompletion) {
            let (key, _) = reopened.replay_key(&request).unwrap();
            reopened.rotate(&key).unwrap();
            drop(reopened);
            let (mut archived, _) = prepared(audit, files.clone(), false, binding("op:1"));
            assert!(matches!(archived.handle(&request), Ok(Reply::Receipt { replayed: true, .. })));
            assert!(archived.ledger().get(&key).unwrap().archived);
            assert_eq!(files.borrow().replaces, 1);
        }
    }
}

fn evidence(state: RecoveryState) -> RecoveryRecord {
    RecoveryRecord {
        key: ActionKey::bind(&binding("op:1"), &token("a:1")),
        args: ArgDigest::of(b"args").unwrap(),
        receipt: token("receipt:41"),
        state,
    }
}

#[test]
fn pending_partial_and_unknown_pin_until_durable_reconciliation() {
    for state in [RecoveryState::Pending, RecoveryState::Partial, RecoveryState::Unknown] {
        let audit = Audit::default();
        let files = files();
        let (initial, request) = prepared(audit.clone(), files.clone(), false, binding("op:1"));
        let (key, args) = initial.replay_key(&request).unwrap();
        let intent = RecoveryRecord { key: key.clone(), args, ..evidence(RecoveryState::Pending) };
        audit.records.borrow_mut().push(intent.clone());
        if state != RecoveryState::Pending {
            audit.records.borrow_mut().push(RecoveryRecord { state, ..intent });
        }
        drop(initial);
        let (mut restored, _) = prepared(audit.clone(), files.clone(), false, binding("op:1"));
        assert!(!restored.handle(&request).unwrap_err().retryable());
        assert_eq!(
            restored.ledger().get(&key).unwrap().status,
            if state == RecoveryState::Partial { EntryStatus::Partial } else { EntryStatus::Unknown }
        );
        assert!(restored.rotate(&key).is_err());
        let before = audit.records.borrow().len();
        assert!(restored.reconcile(&key, false).is_err());
        assert_eq!(audit.records.borrow().len(), before);
        restored.reconcile(&key, true).unwrap();
        drop(restored);
        let (mut again, _) = prepared(audit, files.clone(), false, binding("op:1"));
        assert!(matches!(again.handle(&request), Ok(Reply::Receipt { replayed: true, .. })));
        assert_eq!(files.borrow().replaces, 0);
    }
}

#[test]
fn full_key_separates_artifact_project_invocation_and_action() {
    let records = [evidence(RecoveryState::Pending), evidence(RecoveryState::Committed), evidence(RecoveryState::Archived)];
    let mut ledger = Ledger::new(5);
    ledger.restore(&records).unwrap();
    let r = &records[0];
    assert_eq!(ledger.lookup(&r.key, &r.args), Ok(Lookup::Existing(r.receipt.clone())));
    for field in 0..4 {
        let mut key = r.key.clone();
        match field {
            0 => key.artifact = Digest::new(&format!("sha256:{}", "b".repeat(64))).unwrap(),
            1 => key.root = token("project:2"),
            2 => key.invocation = token("op:2"),
            _ => key.action_id = token("a:2"),
        }
        assert_eq!(ledger.lookup(&key, &r.args), Ok(Lookup::Absent));
    }
    assert_eq!(ledger.reserve().unwrap(), token("receipt:42"));
}

#[test]
fn corrupt_over_limit_and_unavailable_recovery_fail_closed() {
    let intent = evidence(RecoveryState::Pending);
    let completion = evidence(RecoveryState::Committed);
    let bad_args = RecoveryRecord { args: ArgDigest::of(b"changed").unwrap(), ..completion.clone() };
    let bad_receipt = RecoveryRecord { receipt: token("receipt:99"), ..completion.clone() };
    for records in [
        vec![completion.clone()],
        vec![intent.clone(), bad_args],
        vec![intent.clone(), bad_receipt],
        vec![intent.clone(), intent.clone()],
        vec![intent.clone(), completion.clone(), completion],
        vec![intent.clone(); MAX_RECOVERY_RECORDS + 1],
    ] {
        let mut ledger = Ledger::new(64);
        assert!(ledger.restore(&records).is_err());
        assert!(ledger.get(&intent.key).is_none(), "failed restoration is atomic");
    }
    assert!(Ledger::new(0).restore(&[intent]).is_err());
    let files = files();
    let (mut broker, request) = prepared(Audit { unavailable: true, ..Audit::default() }, files.clone(), false, binding("op:1"));
    assert_eq!(broker.handle(&request).unwrap_err().code, ErrorCode::AuditUnavailable);
    assert_eq!(files.borrow().replaces, 0);
    broker.audit_mut().unavailable = false;
    assert!(broker.handle(&request).is_ok());
    assert_eq!(files.borrow().replaces, 1);
}

#[test]
fn known_not_run_releases_key_but_never_reuses_receipt() {
    let records = [evidence(RecoveryState::Pending), evidence(RecoveryState::NotRun)];
    let mut ledger = Ledger::new(1);
    ledger.restore(&records).unwrap();
    assert_eq!(ledger.lookup(&records[0].key, &records[0].args), Ok(Lookup::Absent));
    assert_eq!(ledger.reserve().unwrap(), token("receipt:42"));
    assert_eq!(ArgDigest::from_hex(&"ab".repeat(32)), ArgDigest::from_hex(&"AB".repeat(32)));
    assert!(ArgDigest::from_hex(&"é".repeat(32)).is_err());
    assert!(ArgDigest::from_hex(&"x".repeat(64)).is_err());
}
