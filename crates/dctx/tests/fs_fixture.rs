//! Filesystem fixture. The checked-in tree has no `.git` directory.
//! The fixture `.gitignore` names `secrets.env`, so that file is not in the checkout.
//! The test writes both in a temp copy. `.gitignore` text is not applied unless the caller passes it as selection text.
//! The facade does not open or rewrite `.git/config`.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dctx::memory::{CancelFlag, FixedClock};
use dctx::{
    AutoCleanConfig, FsSource, HostCeiling, HostGrants, IdentityOutcome, OperationLimits, RuleTexts, SelectionPolicy,
    Session, SnapshotRequest, Source, StoragePolicy, Timestamp, VecSink,
};

#[test]
fn gitignore_is_not_automatic_and_git_config_is_not_rewritten() {
    let root = tempfile::tempdir().expect("temp");
    copy_fixture(root.path());
    let git_config = root.path().join(".git/config");
    std::fs::create_dir_all(root.path().join(".git")).expect("git dir");
    let config_bytes = b"[core]\n\trepositoryformatversion = 0\n";
    std::fs::write(&git_config, config_bytes).expect("config");
    // Named by the fixture `.gitignore`, so the checkout cannot contain this file.
    std::fs::write(root.path().join("secrets.env"), b"TOKEN=secret\n").expect("secret");
    let ignore_path = root.path().join(".gitignore");
    let ignore_before = std::fs::read(&ignore_path).expect("ignore");

    let included = run(root.path(), SelectionPolicy::from_texts(&RuleTexts::default()));
    assert_eq!(included.reads, 4);
    assert_eq!(included.secret.as_deref(), Some("secrets.env"));
    assert!(included.git_gone);
    assert_eq!(std::fs::read(&git_config).expect("config after"), config_bytes);
    assert_eq!(std::fs::read(&ignore_path).expect("ignore after"), ignore_before);
    assert!(!root.path().join(".dctx").exists());

    let omitted = run(
        root.path(),
        SelectionPolicy::from_texts(&RuleTexts { local_omit: Some("secrets.env\n"), ..RuleTexts::default() }),
    );
    assert!(omitted.reads < included.reads);
    assert_eq!(omitted.reads, 3);
    assert!(omitted.secret.is_none());
    assert!(omitted.git_gone);
    assert_eq!(std::fs::read(&git_config).expect("config still"), config_bytes);
    assert_eq!(std::fs::read(&ignore_path).expect("ignore still"), ignore_before);
}

struct Probe {
    reads: u64,
    secret: Option<String>,
    git_gone: bool,
}

fn run(root: &Path, selection: SelectionPolicy) -> Probe {
    let source = FsSource::open(root).expect("open");
    let mut session = common::session_parts(
        source,
        Arc::new(CancelFlag::new()),
        HostGrants::PermitAll,
        StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 },
        selection,
    );
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let reads = session.content_reads();
    let mut sink = VecSink::new();
    let secret = session.explain(&handle, "m:secrets.env", &mut sink).expect("secret");
    let secret = secret.records.first().map(|record| record.path.clone());
    let mut sink = VecSink::new();
    let git = session.explain(&handle, "m:.git/config", &mut sink).expect("git");
    let git_gone = matches!(git.outcome, Some(IdentityOutcome::Gone { .. }));
    assert!(git.records.iter().all(|record| record.path != ".git" && !record.path.starts_with(".git/")));
    let mut sink = VecSink::new();
    let ping = session.explain(&handle, "s:src/lib.rs#ping", &mut sink).expect("ping");
    assert_eq!(ping.records[0].summary, "Respond.");
    Probe { reads, secret, git_gone }
}

fn copy_fixture(dest: &Path) {
    let from = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repository");
    copy_dir(&from, dest);
}

#[test]
fn managed_cache_round_trips_and_isolation_does_not_clean() {
    let isolated = common::open_session(common::repository());
    assert!(isolated.cache_ledger().is_none());
    assert_eq!(isolated.cache_status().reclaimed_bytes, 0);

    let refused = Session::builder(common::repository())
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 })
        .place("fixture")
        .view("map")
        .cache_root(Path::new("relative-cache"), 7)
        .build()
        .expect_err("relative");
    assert_eq!(refused.code(), dctx::ErrorCode::InvalidRequest);

    let dir = tempfile::tempdir().expect("temp");
    let cache = dir.path().join("cache");
    std::fs::create_dir(&cache).expect("dir");
    let mut first = open_cached(common::repository(), 0, &cache, 7, AutoCleanConfig::default());
    let handle = first.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    assert_eq!(handle.generation(), 1);
    let admitted = first.cache_ledger().expect("ledger").admitted_bytes;
    assert!(admitted > 0);
    assert_eq!(first.cache_status().reclaimed_bytes, 0);
    assert!(object_files(&cache) >= 1);
    drop(handle);
    drop(first);

    let second = open_cached(common::repository(), 0, &cache, 7, AutoCleanConfig::default());
    let ledger = second.cache_ledger().expect("reloaded");
    assert_eq!(ledger.roots, 1);
    assert_eq!(ledger.admitted_bytes, admitted);
    assert_eq!(ledger.last_auto_clean_ms, Some(0));
    assert_eq!(ledger.expired_roots, 0);
    assert_eq!(second.cache_status().reclaimed_bytes, 0);
    assert!(object_files(&cache) >= 1);
}

#[test]
fn auto_clean_is_disabled_until_the_policy_and_the_day_say_so() {
    let dir = tempfile::tempdir().expect("temp");
    let cache = dir.path().join("cache");
    std::fs::create_dir(&cache).expect("dir");
    let disabled = AutoCleanConfig { enabled: false, ..AutoCleanConfig::default() };
    let skipped = open_cached(common::repository(), 50_000_000, &cache, 1, disabled);
    assert!(skipped.cache_ledger().expect("ledger").last_auto_clean_ms.is_none());
    assert_eq!(skipped.cache_status().reclaimed_bytes, 0);
    drop(skipped);

    let fresh = dir.path().join("fresh");
    std::fs::create_dir(&fresh).expect("dir");
    let opened = open_cached(common::repository(), 10, &fresh, 2, AutoCleanConfig::default());
    assert_eq!(opened.cache_ledger().expect("ledger").last_auto_clean_ms, Some(10));
    drop(opened);
    let same_day = open_cached(common::repository(), 10, &fresh, 2, AutoCleanConfig::default());
    assert_eq!(same_day.cache_ledger().expect("ledger").last_auto_clean_ms, Some(10));
    assert_eq!(same_day.cache_status().reclaimed_bytes, 0);
    drop(same_day);
    let next_day = open_cached(common::repository(), 10 + 86_400_000, &fresh, 2, AutoCleanConfig::default());
    assert_eq!(next_day.cache_ledger().expect("ledger").last_auto_clean_ms, Some(10 + 86_400_000));
}

#[test]
fn cap_and_ttl_follow_the_injected_clock() {
    let dir = tempfile::tempdir().expect("temp");
    let cache = dir.path().join("cache");
    std::fs::create_dir(&cache).expect("dir");
    let mut policy = AutoCleanConfig { min_interval_ms: 0, root_ttl_ms: 1_000, ..AutoCleanConfig::default() };
    let mut first = open_cached(common::repository(), 0, &cache, 3, policy);
    first.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    assert!(first.cache_ledger().expect("ledger").admitted_bytes > 0);
    assert_eq!(first.cache_ledger().expect("ledger").expired_roots, 0);
    assert!(object_files(&cache) >= 1);
    drop(first);

    policy.system_cap_bytes = 65_536;
    let aged = open_cached(common::repository(), 1_000, &cache, 3, policy);
    let ledger = aged.cache_ledger().expect("aged");
    assert_eq!(ledger.hard_cap, 65_536);
    assert!(ledger.expired_roots >= 1);
    assert_eq!(aged.cache_status().reclaimed_bytes, 0);
    drop(aged);

    let grace = dctx_adapter_cache::UNLINK_GRACE_MS;
    let mut when = 1_000u64;
    let mut reclaimed = 0u64;
    for _ in 0..6 {
        when = when.saturating_add(grace);
        let session = open_cached(common::repository(), when, &cache, 3, policy);
        reclaimed = session.cache_status().reclaimed_bytes;
        if reclaimed > 0 {
            break;
        }
    }
    assert!(reclaimed > 0);
    assert_eq!(object_files(&cache), 0);
}

fn open_cached(
    source: impl Source + Send + 'static,
    clock_ms: u64,
    cache: &Path,
    root_id: u64,
    clean: AutoCleanConfig,
) -> Session {
    Session::builder(source)
        .clock(FixedClock(Timestamp(clock_ms)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 })
        .limits(OperationLimits::default())
        .place("fixture")
        .view("map")
        .cache_root(cache, root_id)
        .auto_clean(clean)
        .build()
        .expect("session")
}

fn object_files(cache: &Path) -> usize {
    let objects = cache.join("objects");
    let Ok(entries) = std::fs::read_dir(objects) else {
        return 0;
    };
    entries.filter(|entry| entry.as_ref().is_ok_and(|item| item.path().is_file())).count()
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("mkdir");
    for entry in std::fs::read_dir(from).expect("list") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        let kind = entry.file_type().expect("kind");
        if kind.is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy");
        }
    }
}
