// SPDX-License-Identifier: Apache-2.0
//! Compare traversal metadata against the original listing plus per-path stat.
#![forbid(unsafe_code)]
#![cfg(feature = "fs")]

use std::fs::{self, File};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use dctx::memory::{CancelFlag, FixedClock};
use dctx::{
    AutoCleanConfig, FsSource, HostCeiling, HostGrants, MapRequest, RuleTexts, SelectionPolicy,
    Session, SnapshotRequest, Source, StoragePolicy, Timestamp, VecSink, Verb,
};
use dctx_core::datatypes::NodeType;
use dctx_core::error::DctxError;
use dctx_core::facts::{EntryMeta, EntryStamp, ReadGrant, SourceEntry};
use dctx_core::ids::RelativePath;

/// Leave Source::inventory at its default, exercising the pre-cursor path.
struct PerPath(FsSource);

impl Source for PerPath {
    fn root_kind(&self) -> Result<NodeType, DctxError> {
        self.0.root_kind()
    }
    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, DctxError> {
        self.0.entries(dir)
    }
    fn kind(&self, path: &RelativePath) -> Result<NodeType, DctxError> {
        self.0.kind(path)
    }
    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, DctxError> {
        self.0.metadata(path)
    }
    fn stat(&self, path: &RelativePath) -> Result<(EntryMeta, Option<EntryStamp>), DctxError> {
        self.0.stat(path)
    }
    fn read(&self, grant: &ReadGrant, cap: u64) -> Result<Vec<u8>, DctxError> {
        self.0.read(grant, cap)
    }
}

fn open(source: impl Source + Send + 'static, cache: &Path) -> Session {
    fs::create_dir_all(cache).unwrap();
    Session::builder(source)
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral {
            max_bytes: 8 * 1024 * 1024,
        })
        .place("fixture")
        .view("map")
        .cache_root(cache, 1)
        .auto_clean(AutoCleanConfig {
            enabled: false,
            ..AutoCleanConfig::default()
        })
        .build()
        .unwrap()
}

fn outcome(mut session: Session) -> Result<(bool, Vec<u8>), dctx::ErrorCode> {
    let handle = session
        .snapshot_reusable(&SnapshotRequest::strict(), "fixture")
        .map_err(|error| error.code())?;
    let mut request = MapRequest::new(Verb::Find);
    request.query = "probe".into();
    let mut sink = VecSink::new();
    session
        .query(&handle, &request, &mut sink)
        .map_err(|error| error.code())?;
    Ok((handle.reused(), sink.as_bytes().to_vec()))
}

struct Pair {
    temp: tempfile::TempDir,
}

impl Pair {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("repo/sub/deeper")).unwrap();
        fs::write(
            temp.path().join("repo/sub/deeper/a.rs"),
            b"pub fn probe_a() {}\n",
        )
        .unwrap();
        set_mtime(&temp.path().join("repo/sub/deeper/a.rs"), aged(0));
        Self { temp }
    }

    fn root(&self) -> std::path::PathBuf {
        self.temp.path().join("repo")
    }

    fn compare(&self) -> Result<bool, dctx::ErrorCode> {
        let cursor = outcome(open(
            FsSource::open(&self.root()).unwrap(),
            &self.temp.path().join("cursor-cache"),
        ));
        let per_path = outcome(open(
            PerPath(FsSource::open(&self.root()).unwrap()),
            &self.temp.path().join("path-cache"),
        ));
        assert_eq!(cursor, per_path, "freshness verdict and answer bytes");
        cursor.map(|(reused, _)| reused)
    }
}

fn aged(step: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000 + step * 10)
}

fn set_mtime(path: &Path, time: SystemTime) {
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

#[test]
fn inventory_and_per_path_have_identical_edit_add_delete_rename_and_touch_verdicts() {
    let pair = Pair::new();
    let a = pair.root().join("sub/deeper/a.rs");
    let b = pair.root().join("sub/deeper/b.rs");
    assert!(!pair.compare().unwrap());
    assert!(pair.compare().unwrap());
    fs::write(&a, b"pub fn probe_b() {}\n").unwrap();
    set_mtime(&a, aged(1));
    assert!(!pair.compare().unwrap(), "same-size edit");
    assert!(pair.compare().unwrap());
    fs::write(&b, b"pub fn probe_c() {}\n").unwrap();
    set_mtime(&b, aged(2));
    assert!(!pair.compare().unwrap(), "addition");
    fs::remove_file(&b).unwrap();
    assert!(!pair.compare().unwrap(), "deletion");
    fs::rename(&a, &b).unwrap();
    assert!(!pair.compare().unwrap(), "rename");
    set_mtime(&b, aged(3));
    assert!(pair.compare().unwrap(), "touch keeps the content fresh");
    // Keep the old inode alive, so replacement cannot accidentally recycle it.
    fs::rename(&b, pair.temp.path().join("retired.rs")).unwrap();
    fs::write(&b, b"pub fn probe_d() {}\n").unwrap();
    set_mtime(&b, aged(3));
    assert!(
        !pair.compare().unwrap(),
        "replacement preserving size and mtime"
    );
    assert!(pair.compare().unwrap());
}

#[test]
fn inventory_and_per_path_hash_same_stamp_edits_in_the_racy_window() {
    let pair = Pair::new();
    let a = pair.root().join("sub/deeper/a.rs");
    // A future mtime deterministically stays inside the unsettled window.
    let time = SystemTime::now() + Duration::from_secs(3600);
    set_mtime(&a, time);
    assert!(!pair.compare().unwrap());
    assert!(pair.compare().unwrap());
    fs::write(&a, b"pub fn probe_b() {}\n").unwrap();
    set_mtime(&a, time);
    assert!(!pair.compare().unwrap());
    assert!(pair.compare().unwrap());
}

#[cfg(unix)]
#[test]
fn inventory_and_per_path_preserve_permission_errors() {
    use std::os::unix::fs::PermissionsExt;

    let pair = Pair::new();
    assert!(!pair.compare().unwrap());
    let sub = pair.root().join("sub");
    let permissions = fs::metadata(&sub).unwrap().permissions();
    fs::set_permissions(&sub, fs::Permissions::from_mode(0o0)).unwrap();
    let unreadable = fs::read_dir(&sub).is_err();
    let result = pair.compare();
    fs::set_permissions(&sub, permissions).unwrap();
    if unreadable {
        assert!(result.is_err());
    }
    assert!(pair.compare().unwrap());
}

#[cfg(windows)]
#[test]
fn inventory_and_per_path_preserve_locked_file_read_failures() {
    use std::os::windows::fs::OpenOptionsExt;

    let pair = Pair::new();
    let a = pair.root().join("sub/deeper/a.rs");
    set_mtime(&a, SystemTime::now() + Duration::from_secs(3600));
    assert!(!pair.compare().unwrap());
    let held = File::options().read(true).share_mode(0).open(&a).unwrap();
    assert!(!pair.compare().unwrap());
    drop(held);
    assert!(!pair.compare().unwrap());
    assert!(pair.compare().unwrap());
}

#[cfg(windows)]
#[test]
fn inventory_and_per_path_preserve_denied_file_attributes() {
    use std::process::Command;

    let pair = Pair::new();
    assert!(!pair.compare().unwrap());
    let a = pair.root().join("sub/deeper/a.rs");
    // Everyone's well-known SID avoids localized account names. Deny only
    // READ_ATTRIBUTES; directory enumeration can still report this file.
    let denied = Command::new("icacls")
        .arg(&a)
        .args(["/deny", "*S-1-1-0:(RA)"])
        .output()
        .unwrap();
    assert!(denied.status.success(), "{denied:?}");
    let source = FsSource::open(&pair.root()).unwrap();
    let stat = source.stat(&RelativePath::parse("sub/deeper/a.rs").unwrap());
    let result = pair.compare();
    let restored = Command::new("icacls")
        .arg(&a)
        .args(["/remove:d", "*S-1-1-0"])
        .output()
        .unwrap();
    assert!(restored.status.success(), "{restored:?}");
    assert!(stat.is_err(), "the live attribute stat must be denied");
    assert!(!result.unwrap(), "a stat failure cannot certify reuse");
    assert!(!pair.compare().unwrap());
    assert!(pair.compare().unwrap());
}
