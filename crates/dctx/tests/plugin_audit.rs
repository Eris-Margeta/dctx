// SPDX-License-Identifier: Apache-2.0
//! PP3 durable intent/effect/completion evidence through the actual host broker.
//! Injected storage cuts distinguish written bytes from synced bytes; disk controls check
//! owner-only files and persisted evidence. PP5 full action-key recovery remains H-RECOVER.

#![forbid(unsafe_code)]

#[path = "../src/plugins/audit.rs"]
mod audit;
#[path = "../../dctx-plugin-host/tests/common/mod.rs"]
mod fixture;

use audit::{AuditStorage, DurableAudit};
use dctx_plugin_host::fakes::{FakeFiles, FakeProcess};
use dctx_plugin_host::ports::{AuditKind, AuditPort, AuditRecord, AuditUnavailable, FileFault, FileIdentity, FilePort};
use dctx_plugin_host::{ArgDigest, Authority, Broker, HandleKind, Ledger, Reply};
use dctx_plugin_protocol::records::broker::FileStage;
use dctx_plugin_protocol::records::bytes::Chunk;
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::scalars::ProjectPath;
use dctx_plugin_protocol::{EffectState, ErrorCode};
use proptest::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

const SECRET: &[u8] = b"SOURCE-and-replacement-private-7ea5\n";
const CAPACITY: usize = 8192;

#[derive(Default)]
struct Io {
    bytes: Vec<u8>,
    durable: Vec<u8>,
    fail: Option<(&'static str, usize)>,
    read_fails: bool,
    trace: Vec<&'static str>,
}

#[derive(Clone, Default)]
struct Memory(Rc<RefCell<Io>>);

impl AuditStorage for Memory {
    fn read(&mut self, _: usize) -> Result<Vec<u8>, AuditUnavailable> {
        let io = self.0.borrow();
        if io.read_fails { Err(AuditUnavailable) } else { Ok(io.bytes.clone()) }
    }
    fn append_sync(&mut self, frame: &[u8]) -> Result<(), AuditUnavailable> {
        let mut io = self.0.borrow_mut();
        io.trace.push("append");
        if let Some((kind, cut)) = io.fail
            && std::str::from_utf8(frame).unwrap().contains(kind)
        {
            io.bytes.extend_from_slice(&frame[..cut.min(frame.len())]);
            return Err(AuditUnavailable);
        }
        io.bytes.extend_from_slice(frame);
        let Io { durable, bytes, .. } = &mut *io;
        durable.clone_from(bytes);
        io.trace.push("sync");
        Ok(())
    }
}

type BeforeReplace = Box<dyn FnMut(&[u8])>;

struct ObservedFiles {
    inner: FakeFiles,
    before: BeforeReplace,
}

impl FilePort for ObservedFiles {
    fn identity(&mut self, p: &ProjectPath) -> Result<FileIdentity, FileFault> {
        self.inner.identity(p)
    }
    fn read(&mut self, p: &ProjectPath, id: FileIdentity, offset: u64, len: u64) -> Result<Vec<u8>, FileFault> {
        self.inner.read(p, id, offset, len)
    }
    fn replace(&mut self, p: &ProjectPath, id: FileIdentity, bytes: &[u8]) -> Result<FileIdentity, FileFault> {
        (self.before)(bytes);
        self.inner.replace(p, id, bytes)
    }
}

fn prepared<A: AuditPort>(audit: A, before: impl FnMut(&[u8]) + 'static) -> (Broker<A, ObservedFiles, FakeProcess>, Request) {
    let mut authority = Authority::new(fixture::layers(fixture::all_caps()), fixture::included(), fixture::binding("op:1"), 0);
    authority.initialize().unwrap();
    let mut inner = FakeFiles::default();
    inner.put(&fixture::path(), SECRET);
    let files = ObservedFiles { inner, before: Box::new(before) };
    let mut broker = Broker::new(authority, Ledger::new(64), fixture::budget(8192, 8), 8192, (audit, files, FakeProcess::default()));
    let handle = broker.register(HandleKind::File { path: fixture::path() }).unwrap();
    let precondition = broker.issue_precondition(&handle).unwrap();
    let staged = broker
        .handle(&Request::FileStage(FileStage {
            resource: handle.clone(),
            precondition: precondition.clone(),
            upload: None,
            offset: 0,
            bytes: Chunk::new(SECRET).unwrap(),
            final_chunk: true,
        }))
        .unwrap();
    let Reply::Staged { upload, .. } = staged else { panic!("upload") };
    (broker, fixture::replace_req(&handle, &precondition, &upload, "a:1"))
}

fn record(kind: AuditKind) -> AuditRecord {
    AuditRecord {
        kind,
        plugin: fixture::binding("op:1").plugin,
        invocation: fixture::token("op:1"),
        epoch: 1,
        method: "file.replace",
        handle: Some(fixture::token("handle:1")),
        args: Some(ArgDigest::of(SECRET).unwrap()),
        bytes: SECRET.len() as u64,
        receipt: Some(fixture::token("receipt:1")),
        reason: if kind == AuditKind::Intent { "intent" } else { "committed" },
    }
}

#[test]
fn synced_intent_precedes_effect_and_completion_links_without_source_bytes() {
    let memory = Memory::default();
    let witness = memory.clone();
    let (mut broker, request) = prepared(DurableAudit::new(memory.clone(), CAPACITY).unwrap(), move |_| {
        let mut io = witness.0.borrow_mut();
        assert_eq!(io.bytes, io.durable);
        assert!(std::str::from_utf8(&io.durable).unwrap().contains("\tintent\t"));
        io.trace.push("effect");
    });
    let Reply::Receipt { receipt, .. } = broker.handle(&request).unwrap() else { panic!("receipt") };
    let io = memory.0.borrow();
    assert_eq!(io.trace, ["append", "sync", "effect", "append", "sync"]);
    let text = std::str::from_utf8(&io.durable).unwrap();
    assert_eq!(text.matches(receipt.as_str()).count(), 2);
    assert!(!text.contains(std::str::from_utf8(SECRET).unwrap()));
    assert_eq!(broker.audit().uncertain(), [] as [(std::string::String, std::string::String, std::string::String); 0]);
}

#[test]
fn write_and_sync_failures_before_intent_refuse_effect_with_restore_control() {
    for cut in [0, 1, usize::MAX] {
        let memory = Memory::default();
        memory.0.borrow_mut().fail = Some(("\tintent\t", cut));
        let (mut broker, request) = prepared(DurableAudit::new(memory.clone(), CAPACITY).unwrap(), |_| {});
        let error = broker.handle(&request).unwrap_err();
        assert_eq!((error.code, error.effect_state), (ErrorCode::AuditUnavailable, EffectState::None));
        assert_eq!(broker.files().inner.replaces, 0);
        memory.0.borrow_mut().fail = None;
        assert!(broker.handle(&request).is_err(), "failed writer stays closed");
        memory.0.borrow_mut().bytes.clear(); // Simulated crash: no bytes were synced.
        *broker.audit_mut() = DurableAudit::new(memory.clone(), CAPACITY).unwrap();
        assert!(broker.handle(&request).is_ok());
        assert_eq!(broker.files().inner.replaces, 1);
    }
}

#[test]
fn completion_failure_and_missing_completion_recover_uncertain_without_retry() {
    for cut in [0, 1, usize::MAX] {
        let memory = Memory::default();
        memory.0.borrow_mut().fail = Some(("\tcompletion\t", cut));
        let (mut broker, request) = prepared(DurableAudit::new(memory.clone(), CAPACITY).unwrap(), |_| {});
        let error = broker.handle(&request).unwrap_err();
        assert_eq!((error.code, error.effect_state), (ErrorCode::EffectUnknown, EffectState::Unknown));
        assert_eq!(broker.files().inner.replaces, 1);
        assert!(broker.handle(&request).is_err());
        let mut io = memory.0.borrow_mut();
        io.bytes = io.durable.clone();
        io.fail = None;
        drop(io);
        let reopened = DurableAudit::new(memory, CAPACITY).unwrap();
        assert_eq!(reopened.uncertain()[0].2, error.receipt.unwrap().as_str());
        assert_eq!(broker.files().inner.replaces, 1);
    }
    let (mut control, request) = prepared(DurableAudit::new(Memory::default(), CAPACITY).unwrap(), |_| {});
    assert!(control.handle(&request).is_ok());
    assert_eq!(control.audit().uncertain(), [] as [(std::string::String, std::string::String, std::string::String); 0]);
}

#[test]
fn capacity_and_read_failures_refuse_with_restore_control() {
    let memory = Memory::default();
    memory.0.borrow_mut().read_fails = true;
    assert!(DurableAudit::new(memory.clone(), CAPACITY).is_err());
    memory.0.borrow_mut().read_fails = false;
    assert!(DurableAudit::new(memory.clone(), usize::MAX).is_err());
    let (mut broker, request) = prepared(DurableAudit::new(memory.clone(), 0).unwrap(), |_| {});
    assert_eq!(broker.handle(&request).unwrap_err().code, ErrorCode::AuditUnavailable);
    assert_eq!(broker.files().inner.replaces, 0);
    assert_eq!(memory.0.borrow().bytes, [] as [u8; 0]);
    *broker.audit_mut() = DurableAudit::new(memory.clone(), CAPACITY).unwrap();
    assert!(broker.handle(&request).is_ok());
    let bytes = memory.0.borrow().bytes.clone();
    assert!(DurableAudit::new(memory.clone(), bytes.len() - 1).is_err());
    let mut full = DurableAudit::new(memory.clone(), bytes.len()).unwrap();
    let mut next = record(AuditKind::Intent);
    next.invocation = fixture::token("op:2");
    assert!(full.append(&next).is_err());
    assert_eq!(memory.0.borrow().bytes, bytes);
    assert!(DurableAudit::new(memory, CAPACITY).unwrap().append(&next).is_ok());
}

#[test]
fn links_unknown_outcomes_and_metadata_validation_have_controls() {
    let memory = Memory::default();
    let mut audit = DurableAudit::new(memory.clone(), CAPACITY).unwrap();
    let mut completion = record(AuditKind::Completion);
    assert!(audit.append(&completion).is_err(), "no unlinked completions");
    let mut intent = record(AuditKind::Intent);
    intent.method = "source\nbytes";
    assert!(audit.append(&intent).is_err());
    intent.method = "file.replace";
    audit.append(&intent).unwrap();
    assert!(audit.append(&intent).is_err(), "no ambiguous receipt reuse");
    completion.invocation = fixture::token("op:other");
    assert!(audit.append(&completion).is_err());
    completion.invocation = intent.invocation.clone();
    completion.reason = "unknown";
    audit.append(&completion).unwrap();
    let mut reopened = DurableAudit::new(memory.clone(), CAPACITY).unwrap();
    assert_eq!(reopened.uncertain().len(), 1);
    completion.method = "reconcile";
    completion.reason = "reconciled";
    reopened.append(&completion).unwrap();
    assert_eq!(DurableAudit::new(memory, CAPACITY).unwrap().uncertain(), [] as [(std::string::String, std::string::String, std::string::String); 0]);
}

proptest! {
    #[test]
    fn every_torn_completion_preserves_intent_and_refuses_append(cut in 0usize..2048) {
        let memory = Memory::default();
        let mut audit = DurableAudit::new(memory.clone(), CAPACITY).unwrap();
        audit.append(&record(AuditKind::Intent)).unwrap();
        let prefix = memory.0.borrow().bytes.len();
        audit.append(&record(AuditKind::Completion)).unwrap();
        let full = memory.0.borrow().bytes.clone();
        prop_assert!(DurableAudit::new(memory.clone(), CAPACITY).unwrap().uncertain().is_empty());
        let tail = cut % (full.len() - prefix);
        memory.0.borrow_mut().bytes.truncate(prefix + tail);
        let mut reopened = DurableAudit::new(memory.clone(), CAPACITY).unwrap();
        prop_assert_eq!(reopened.uncertain().len(), 1);
        if tail > 0 { prop_assert!(reopened.append(&record(AuditKind::Completion)).is_err()); }
        memory.0.borrow_mut().bytes = full;
        prop_assert!(DurableAudit::new(memory, CAPACITY).unwrap().uncertain().is_empty());
    }
}

#[cfg(all(feature = "fs", unix))]
#[test]
fn disk_effect_observes_intent_and_reopen_observes_completion() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let log = dir.path().join("effects.audit");
    let target = dir.path().join("effect");
    let (seen_log, seen_target) = (log.clone(), target.clone());
    let durable = DurableAudit::new(audit::disk::Storage::open(dir.path()).unwrap(), CAPACITY).unwrap();
    let (mut broker, request) = prepared(durable, move |bytes| {
        let text = fs::read_to_string(&seen_log).unwrap();
        assert!(text.contains("\tintent\t") && !text.contains("\tcompletion\t"));
        fs::write(&seen_target, bytes).unwrap();
    });
    assert!(broker.handle(&request).is_ok());
    assert_eq!(fs::read(target).unwrap(), SECRET);
    assert_eq!(fs::metadata(&log).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(audit::open(dir.path(), CAPACITY).is_err(), "exclusive writer");
    drop(broker);
    let reopened = DurableAudit::new(audit::disk::Storage::open(dir.path()).unwrap(), CAPACITY).unwrap();
    assert_eq!(reopened.uncertain(), [] as [(std::string::String, std::string::String, std::string::String); 0]);
    drop(reopened);
    let host = dctx::PluginHost::platform_with_audit(dir.path(), CAPACITY).unwrap();
    assert!(audit::open(dir.path(), CAPACITY).is_err(), "platform retains real audit");
    drop(host);
    assert!(audit::open(dir.path(), CAPACITY).is_ok());
    let bytes = fs::read(&log).unwrap();
    assert!(!bytes.windows(SECRET.len()).any(|w| w == SECRET));
    fs::write(&log, &bytes[..bytes.len() - 1]).unwrap();
    let mut torn = DurableAudit::new(audit::disk::Storage::open(dir.path()).unwrap(), CAPACITY).unwrap();
    assert_eq!(torn.uncertain().len(), 1);
    assert!(torn.append(&record(AuditKind::Completion)).is_err());
}

#[cfg(all(feature = "fs", unix))]
#[test]
fn unsafe_modes_and_links_refuse_with_private_file_control() {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(audit::open(dir.path(), CAPACITY).is_err());
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    drop(audit::open(dir.path(), CAPACITY).unwrap());
    let log = dir.path().join("effects.audit");
    fs::set_permissions(&log, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(audit::open(dir.path(), CAPACITY).is_err());
    fs::set_permissions(&log, fs::Permissions::from_mode(0o600)).unwrap();
    drop(audit::open(dir.path(), CAPACITY).unwrap());
    let alias = dir.path().join("alias");
    fs::hard_link(&log, &alias).unwrap();
    assert!(audit::open(dir.path(), CAPACITY).is_err());
    fs::remove_file(&alias).unwrap();
    drop(audit::open(dir.path(), CAPACITY).unwrap());
    fs::rename(&log, &alias).unwrap();
    symlink(&alias, &log).unwrap();
    assert!(audit::open(dir.path(), CAPACITY).is_err());
    fs::remove_file(&log).unwrap();
    fs::rename(&alias, &log).unwrap();
    assert!(audit::open(dir.path(), CAPACITY).is_ok());
}
