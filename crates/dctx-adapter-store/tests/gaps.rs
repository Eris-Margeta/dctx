// SPDX-License-Identifier: Apache-2.0
//! Gap oracles for store confinement and bounded corrupt reads.
//! Implements DX-G01 S1 and S2. Depends on the public store API.
//! Unix symlink fixtures use `tempfile`. Every refusal has a positive control beside it.
//! A hostile length is a logical size. These tests never allocate a gibibyte buffer.

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use common::{req, segment};
use dctx_adapter_store::ports::{FsCapabilities, StoreFs};
use dctx_adapter_store::{Boundary, MemoryFs, SegmentStore, StdFs, StoreError, StoreOptions};

const HUGE: u64 = 1u64 << 30;

/// Lockstep with `ports::OBJECT_MAX`: the greater of the segment codec ceiling and the map writer's admission cap.
fn writer_maximum() -> u64 {
    let codec = dctx_map_segment::DecoderLimits::standard().max_segment_bytes;
    let admission = 512 * 1024 * 1024;
    admission.max(codec)
}

fn cap() -> StoreOptions {
    StoreOptions::capped(8 * 1024 * 1024)
}

fn hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

struct Gate {
    logical: std::collections::BTreeMap<String, u64>,
    grow: Option<String>,
}

/// Instrumented port: an oversize logical length must not be copied, and a length that grows
/// after metadata is observed is refused inside `read` before the bytes are stored.
#[derive(Clone)]
struct CapFs {
    inner: MemoryFs,
    gate: Arc<Mutex<Gate>>,
    log: Arc<Mutex<Vec<String>>>,
    violations: Arc<AtomicU64>,
}

impl CapFs {
    fn new() -> Self {
        Self {
            inner: MemoryFs::posix(),
            gate: Arc::new(Mutex::new(Gate {
                logical: std::collections::BTreeMap::new(),
                grow: None,
            })),
            log: Arc::new(Mutex::new(Vec::new())),
            violations: Arc::new(AtomicU64::new(0)),
        }
    }

    fn inflate(&self, path: &str) {
        self.gate
            .lock()
            .expect("gate")
            .logical
            .insert(path.to_string(), HUGE);
    }

    fn arm_grow(&self, path: &str) {
        self.gate.lock().expect("gate").grow = Some(path.to_string());
    }

    fn reset(&self) {
        let mut gate = self.gate.lock().expect("gate");
        gate.logical.clear();
        gate.grow = None;
    }
}

impl StoreFs for CapFs {
    fn capabilities(&self) -> FsCapabilities {
        self.inner.capabilities()
    }
    fn checkpoint(&self, boundary: Boundary) -> Result<(), StoreError> {
        self.inner.checkpoint(boundary)
    }
    fn try_lock(&self, root: &str) -> Result<(), StoreError> {
        self.inner.try_lock(root)
    }
    fn unlock(&self, root: &str) -> Result<(), StoreError> {
        self.inner.unlock(root)
    }
    fn lock_held(&self, root: &str) -> bool {
        self.inner.lock_held(root)
    }
    fn held_roots(&self) -> Vec<String> {
        self.inner.held_roots()
    }
    fn create(&self, path: &str) -> Result<(), StoreError> {
        self.inner.create(path)
    }
    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
        {
            let mut gate = self.gate.lock().expect("gate");
            gate.logical.remove(path);
            if gate.grow.as_deref() == Some(path) {
                gate.grow = None;
            }
        }
        self.inner.write(path, bytes)
    }
    fn flush(&self, path: &str) -> Result<(), StoreError> {
        self.inner.flush(path)
    }
    fn sync_file(&self, path: &str) -> Result<(), StoreError> {
        self.inner.sync_file(path)
    }
    fn sync_dir(&self) -> Result<(), StoreError> {
        self.inner.sync_dir()
    }
    fn rename_new(&self, from: &str, to: &str) -> Result<(), StoreError> {
        self.inner.rename_new(from, to)
    }
    fn rename_replace(&self, from: &str, to: &str) -> Result<(), StoreError> {
        self.inner.rename_replace(from, to)
    }
    fn read(&self, path: &str) -> Result<Vec<u8>, StoreError> {
        let gate = self.gate.lock().expect("gate");
        let logical = gate.logical.contains_key(path);
        let grew = gate.grow.as_deref() == Some(path);
        drop(gate);
        if logical {
            self.violations.fetch_add(1, Ordering::SeqCst);
            return Err(StoreError::CorruptIndex { reason: "limit" });
        }
        if grew {
            return Err(StoreError::CorruptIndex { reason: "limit" });
        }
        let bytes = self.inner.read(path)?;
        self.log.lock().expect("log").push(path.to_string());
        Ok(bytes)
    }
    fn exists(&self, path: &str) -> Result<bool, StoreError> {
        self.inner.exists(path)
    }
    fn len(&self, path: &str) -> Result<u64, StoreError> {
        if let Some(len) = self.gate.lock().expect("gate").logical.get(path).copied() {
            return Ok(len);
        }
        self.inner.len(path)
    }
    fn unlink(&self, path: &str) -> Result<(), StoreError> {
        self.inner.unlink(path)
    }
    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        self.inner.list_prefix(prefix)
    }
}

fn assert_reads(store: &SegmentStore<CapFs>, bytes: &[u8]) {
    let pin = store.pin("main").expect("pin");
    let got = store
        .read_segment(&pin, pin.segments()[0].digest())
        .expect("segment");
    assert_eq!(got, bytes);
    store.release(pin).expect("release");
}

fn assert_closed(store: &SegmentStore<CapFs>, fs: &CapFs, hostile: &str) {
    fs.log.lock().expect("log").clear();
    fs.violations.store(0, Ordering::SeqCst);
    assert_eq!(
        store.pin("main").expect_err("hostile"),
        StoreError::CorruptIndex { reason: "limit" }
    );
    assert_eq!(
        fs.violations.load(Ordering::SeqCst),
        0,
        "{hostile} was read"
    );
    assert!(
        !fs.log
            .lock()
            .expect("log")
            .iter()
            .any(|path| path == hostile)
    );
    fs.reset();
}

#[test]
fn bounded_corrupt_reads() {
    // Positive control: a pointer at the fixed width is readable. One extra byte is refused.
    let mem = MemoryFs::posix();
    mem.create("roots/main/current").expect("create");
    mem.write("roots/main/current", &[1u8; 92]).expect("write");
    assert_eq!(mem.read("roots/main/current").expect("read").len(), 92);
    mem.write("roots/main/current", &[1u8; 93]).expect("write");
    assert_eq!(
        mem.read("roots/main/current").expect_err("over"),
        StoreError::CorruptIndex { reason: "limit" }
    );
    // A non-pointer wider than a pointer, and still under the writer maximum, is not a pointer ceiling.
    mem.create("segments/wide.seg").expect("create");
    mem.write("segments/wide.seg", &[7u8; 200]).expect("write");
    assert_eq!(mem.read("segments/wide.seg").expect("wide").len(), 200);

    let dir = tempfile::tempdir().expect("dir");
    let disk = StdFs::open(dir.path()).expect("std");
    disk.create("roots/main/current").expect("create");
    disk.write("roots/main/current", &[1u8; 92]).expect("write");
    assert_eq!(disk.read("roots/main/current").expect("read").len(), 92);
    disk.write("roots/main/current", &[1u8; 93]).expect("write");
    assert_eq!(
        disk.read("roots/main/current").expect_err("over"),
        StoreError::CorruptIndex { reason: "limit" }
    );

    // Positive control: a segment of exactly the writer maximum is accepted. One byte over is refused
    // from metadata, before that extra byte is stored. The hole is sparse; only the accepted read allocates.
    let max = writer_maximum();
    disk.create("segments/max.seg").expect("create");
    let max_path = dir.path().join("segments/max.seg");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&max_path)
        .expect("open")
        .set_len(max)
        .expect("len");
    let got = disk.read("segments/max.seg").expect("max");
    assert_eq!(got.len() as u64, max);
    drop(got);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&max_path)
        .expect("open")
        .set_len(max + 1)
        .expect("over");
    assert_eq!(
        disk.read("segments/max.seg").expect_err("over"),
        StoreError::CorruptIndex { reason: "limit" }
    );

    let fs = CapFs::new();
    let store = SegmentStore::open(fs.clone(), cap()).expect("open");
    let first = segment("kept");
    let second = segment("next");
    store.publish(req("main", 1, first.clone())).expect("gen1");
    store.publish(req("main", 2, second.clone())).expect("gen2");
    assert_reads(&store, &second);
    let pin = store.pin("main").expect("pin");
    let manifest = format!("manifests/{}.man", hex(pin.manifest()));
    let segment_path = format!("segments/{}.seg", hex(pin.segments()[0].digest()));
    store.release(pin).expect("release");

    fs.inflate("roots/main/current");
    assert_closed(&store, &fs, "roots/main/current");
    assert_reads(&store, &second);
    fs.inflate(&manifest);
    assert_closed(&store, &fs, &manifest);
    assert_reads(&store, &second);
    fs.inflate(&segment_path);
    assert_closed(&store, &fs, &segment_path);
    assert_reads(&store, &second);

    fs.arm_grow("roots/main/current");
    assert_closed(&store, &fs, "roots/main/current");
    assert_reads(&store, &second);
    fs.arm_grow(&manifest);
    assert_closed(&store, &fs, &manifest);
    assert_reads(&store, &second);
    fs.arm_grow(&segment_path);
    assert_closed(&store, &fs, &segment_path);
    assert_reads(&store, &second);

    fs.write("roots/main/current", &[0u8; 10]).expect("corrupt");
    let recovery = store.recover().expect("recover");
    assert!(recovery.repaired_pointer);
    assert_eq!(recovery.generation, Some(1));
    let pin = store.pin("main").expect("repaired");
    assert_eq!(pin.generation(), 1);
    assert_eq!(
        store
            .read_segment(&pin, pin.segments()[0].digest())
            .expect("kept"),
        first
    );
}

#[test]
fn symlink_confinement() {
    let dir = tempfile::tempdir().expect("dir");
    let store = SegmentStore::open(StdFs::open(dir.path()).expect("fs"), cap()).expect("open");
    let bytes = segment("kept");
    store
        .publish(req("main", 1, bytes.clone()))
        .expect("publish");
    let pin = store.pin("main").expect("pin");
    assert_eq!(
        store
            .read_segment(&pin, pin.segments()[0].digest())
            .expect("read"),
        bytes
    );
    drop(store);
    let fs = StdFs::open(dir.path()).expect("fs");
    fs.create("segments/plain.seg").expect("create");
    fs.write("segments/plain.seg", b"inside").expect("write");
    assert_eq!(fs.len("segments/plain.seg").expect("len"), 6);
    assert_eq!(fs.read("segments/plain.seg").expect("read"), b"inside");
    #[cfg(unix)]
    unix_symlinks();
}

// Threat model: a symlink component present when the call starts is a typed refusal.
// The leaf is opened with O_NOFOLLOW, so a leaf replaced by a symlink fails at that open.
// A parent directory swapped for a symlink after the walk and before the open is a residual
// race. Lexical spelling checks are not confinement. Sentinels are the spy for bytes that escape.
#[cfg(unix)]
fn unix_symlinks() {
    let root = tempfile::tempdir().expect("root");
    let outside = tempfile::tempdir().expect("out");
    let sentinel = seed(outside.path());
    std::os::unix::fs::symlink(outside.path(), root.path().join("segments")).expect("link");
    refuse(
        &StdFs::open(root.path()).expect("fs"),
        "segments/sentinel",
        &sentinel,
    );
    publish_refuses(root.path(), &sentinel);

    let root = tempfile::tempdir().expect("root");
    let outside = tempfile::tempdir().expect("out");
    let sentinel = seed(outside.path());
    std::os::unix::fs::symlink(outside.path(), root.path().join("roots")).expect("link");
    publish_refuses(root.path(), &sentinel);

    let root = tempfile::tempdir().expect("root");
    let outside = tempfile::tempdir().expect("out");
    let sentinel = seed(outside.path());
    std::fs::create_dir(root.path().join("segments")).expect("dir");
    std::os::unix::fs::symlink(&sentinel, root.path().join("segments/leaf")).expect("link");
    refuse(
        &StdFs::open(root.path()).expect("fs"),
        "segments/leaf",
        &sentinel,
    );

    let root = tempfile::tempdir().expect("root");
    let outside = tempfile::tempdir().expect("out");
    let sentinel = seed(outside.path());
    let fs = StdFs::open(root.path()).expect("fs");
    fs.create("segments/leaf").expect("create");
    std::fs::remove_file(root.path().join("segments/leaf")).expect("replace");
    std::os::unix::fs::symlink(&sentinel, root.path().join("segments/leaf")).expect("link");
    refuse(&fs, "segments/leaf", &sentinel);

    let root = tempfile::tempdir().expect("root");
    let outside = tempfile::tempdir().expect("out");
    let sentinel = seed(outside.path());
    std::fs::create_dir_all(root.path().join("roots/main")).expect("dir");
    std::os::unix::fs::symlink(&sentinel, root.path().join("roots/main/current")).expect("link");
    publish_refuses(root.path(), &sentinel);
}

#[cfg(unix)]
fn seed(dir: &Path) -> PathBuf {
    let path = dir.join("sentinel");
    std::fs::write(&path, b"SECRET").expect("seed");
    path
}

#[cfg(unix)]
fn refuse(fs: &StdFs, rel: &str, sentinel: &Path) {
    let err = fs.read(rel).expect_err("read");
    assert_eq!(err, StoreError::InvalidRequest { reason: "path" });
    assert!(!err.to_string().contains("SECRET"));
    assert_eq!(
        fs.write(rel, b"no").expect_err("write"),
        StoreError::InvalidRequest { reason: "path" }
    );
    assert_eq!(
        fs.len(rel).expect_err("len"),
        StoreError::InvalidRequest { reason: "path" }
    );
    assert_eq!(std::fs::read(sentinel).expect("sentinel"), b"SECRET");
}

#[cfg(unix)]
fn publish_refuses(root: &Path, sentinel: &Path) {
    let store = SegmentStore::open(StdFs::open(root).expect("fs"), cap()).expect("open");
    let err = store
        .publish(req("main", 1, segment("gone")))
        .expect_err("publish");
    assert_eq!(err, StoreError::InvalidRequest { reason: "path" });
    assert!(!err.to_string().contains("SECRET"));
    assert_eq!(std::fs::read(sentinel).expect("sentinel"), b"SECRET");
    assert_eq!(
        std::fs::read_dir(sentinel.parent().expect("parent"))
            .expect("dir")
            .count(),
        1
    );
}
