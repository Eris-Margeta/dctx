// SPDX-License-Identifier: Apache-2.0
//! Shared session construction for the public facade tests.
//! Every session gets an explicit source, clock, policy, storage policy and place.
//! Nothing in this helper reads a home directory or a git repository.
//! Each integration test compiles this module and uses a different subset.
#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use dctx::memory::{CancelFlag, FixedClock, MemorySource};
use dctx::{
    Cancellation, HostCeiling, HostGrants, OperationLimits, RuleTexts, SelectionPolicy, Session,
    Source, StoragePolicy, Timestamp,
};
use dctx_core::datatypes::NodeType;
use dctx_core::error::DctxError as CoreError;
use dctx_core::facts::{EntryMeta, ReadGrant, SourceEntry};
use dctx_core::ids::RelativePath;

/// Golden `src/lib.rs`: 30 bytes, span `[13, 29)` is `pub fn ping() {}`.
pub const PING: &[u8] = b"/// Respond.\npub fn ping() {}\n";
/// Same length and modification time as [`PING`], different bytes.
pub const PONG: &[u8] = b"/// Respond.\npub fn pong() {}\n";
/// Fixture heading.
pub const README: &[u8] = b"# Fixture\n";

/// A session rooted at `source` with a frozen clock and permit-all grants.
pub fn open_session(source: impl Source + Send + 'static) -> Session {
    session_parts(
        source,
        Arc::new(CancelFlag::new()),
        HostGrants::PermitAll,
        StoragePolicy::Ephemeral {
            max_bytes: 8 * 1024 * 1024,
        },
        SelectionPolicy::from_texts(&RuleTexts::default()),
    )
}

/// A session with the caller’s cancellation, grants, storage and selection.
pub fn session_parts(
    source: impl Source + Send + 'static,
    cancel: Arc<dyn Cancellation + Send + Sync>,
    grants: HostGrants,
    storage: StoragePolicy,
    selection: SelectionPolicy,
) -> Session {
    Session::builder(source)
        .clock(FixedClock(Timestamp(0)))
        .cancellation(cancel)
        .selection(selection)
        .ceiling(HostCeiling::base())
        .grants(grants)
        .storage(storage)
        .limits(OperationLimits::default())
        .place("fixture")
        .view("map")
        .build()
        .expect("session")
}

/// The repository fixture as an in-memory source. `.git` is not part of the fixture.
pub fn repository() -> MemorySource {
    let mut source = MemorySource::new();
    source.insert_file("src/lib.rs", PING, 0).expect("lib");
    source.insert_file("README.md", README, 0).expect("readme");
    source
        .insert_file(".gitignore", b"# kept\nsecrets.env\n", 0)
        .expect("ignore");
    source
        .insert_file("secrets.env", b"TOKEN=secret\n", 0)
        .expect("secret");
    source
}

/// `MemorySource` shared so a test can change bytes after the session takes ownership.
pub struct SharedSource {
    inner: Arc<Mutex<MemorySource>>,
}

impl SharedSource {
    /// Wraps `source`.
    pub fn new(source: MemorySource) -> Self {
        Self {
            inner: Arc::new(Mutex::new(source)),
        }
    }

    /// Clone of the handle. Call this before moving the source into a session.
    pub fn handle(&self) -> Arc<Mutex<MemorySource>> {
        Arc::clone(&self.inner)
    }
}

impl Source for SharedSource {
    fn root_kind(&self) -> Result<NodeType, CoreError> {
        self.inner.lock().expect("source").root_kind()
    }

    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, CoreError> {
        self.inner.lock().expect("source").entries(dir)
    }

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, CoreError> {
        self.inner.lock().expect("source").metadata(path)
    }

    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, CoreError> {
        self.inner.lock().expect("source").read(grant, max_bytes)
    }
}

/// Any source that records the path of every granted content read, in order.
/// Lets filesystem tests prove from a read log, as `MemorySource::reads` does, that excluded files stay unopened.
pub struct LoggedSource<S> {
    inner: S,
    reads: Arc<Mutex<Vec<String>>>,
}

impl<S: Source> LoggedSource<S> {
    /// Wraps `inner` with an empty read log.
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            reads: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Clone of the read log. Call this before moving the source into a session.
    pub fn log(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.reads)
    }
}

impl<S: Source> Source for LoggedSource<S> {
    fn root_kind(&self) -> Result<NodeType, CoreError> {
        self.inner.root_kind()
    }

    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, CoreError> {
        self.inner.entries(dir)
    }

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, CoreError> {
        self.inner.metadata(path)
    }

    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, CoreError> {
        self.reads
            .lock()
            .expect("read log")
            .push(grant.path().as_str().to_string());
        self.inner.read(grant, max_bytes)
    }
}

/// Lets `allow` checkpoints succeed, then reports cancellation.
pub struct CancelAfter {
    allow: u64,
    checks: AtomicU64,
}

impl CancelAfter {
    /// `allow` successful checks, then cancel.
    pub fn new(allow: u64) -> Self {
        Self {
            allow,
            checks: AtomicU64::new(0),
        }
    }
}

impl Cancellation for CancelAfter {
    fn is_cancelled(&self) -> bool {
        let seen = self.checks.fetch_add(1, Ordering::SeqCst);
        seen >= self.allow
    }
}
