// SPDX-License-Identifier: Apache-2.0

use dctx_adapter_cache::adapters::{
    FsLedgerStore, FsObjectStore, LEDGER_FILE, LeaseHold, LedgerUpdateError,
};
use dctx_adapter_cache::ports::LedgerStore;
use dctx_adapter_cache::{CacheError, LeaseId, Lifecycle, ObjectId, Quota, RootId, SessionId};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Child, Command, Stdio};

const QUOTA: Quota = Quota::scaled(100_000);
const ROOT: RootId = RootId::new(1);
const READER_ROOT: &str = "DCTX_LEASE_REAP_READER_ROOT";
const READY: &str = "lease-acquired ";
const MANIFEST_BYTES: u64 = 8;
const GENERATION_BYTES: u64 = MANIFEST_BYTES + 3;

fn open(root: &Path) -> (FsLedgerStore, FsObjectStore) {
    (
        FsLedgerStore::open(root).expect("open ledger"),
        FsObjectStore::open(root).expect("open objects"),
    )
}

fn publish(ledger: &FsLedgerStore, objects: &FsObjectStore, bytes: &[u8; 3]) -> [ObjectId; 2] {
    ledger
        .update_reclaiming(QUOTA, objects, |life| {
            if life.current_generation(ROOT).is_none() {
                life.register_root(ROOT)?;
            }
            let segment = life.insert_segment(3, 0, &[ROOT])?;
            let generation = life.publish_view(ROOT, MANIFEST_BYTES, &[segment])?;
            let manifest = life
                .manifest_of(ROOT, generation)
                .expect("published manifest");
            let manifest = life.manifest_object(manifest).expect("manifest object");
            objects.put_object(segment.raw(), bytes)?;
            objects.put_object(manifest.raw(), &segment.raw().to_le_bytes())?;
            Ok::<_, CacheError>(([segment, manifest], true))
        })
        .expect("publish")
        .0
}

fn pin(ledger: &FsLedgerStore, objects: &FsObjectStore) -> (LeaseId, LeaseHold) {
    ledger
        .update_reclaiming(QUOTA, objects, |life| {
            let lease = life.reader_begin(ROOT, SessionId::new(1))?;
            life.reader_confirm(lease)?;
            Ok::<_, CacheError>(((lease, ledger.hold_lease(lease)?), true))
        })
        .expect("pin generation")
        .0
}

fn child_reader(root: &Path) -> ! {
    let (ledger, objects) = open(root);
    let (lease, _hold) = pin(&ledger, &objects);
    println!("{READY}{}", lease.raw());
    io::stdout().flush().expect("send acquisition signal");
    assert_eq!(io::stdin().read(&mut [0]).expect("wait for exit signal"), 0);
    process::exit(0);
}

fn start_reader(root: &Path) -> (Child, PathBuf) {
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--quiet",
            "--nocapture",
            "--exact",
            "dead_charge_survives_body_refusal_and_is_settled_once_after_restart",
        ])
        .env(READER_ROOT, root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start reader");
    let stdout = BufReader::new(child.stdout.take().expect("child stdout"));
    let signal = stdout
        .lines()
        .map(|line| line.expect("read acquisition signal"))
        .find_map(|line| line.strip_prefix(READY).map(str::to_owned));
    let lease = signal
        .expect("reader exited before acquisition")
        .parse()
        .expect("lease ID in acquisition signal");
    (child, evidence_path(root, lease))
}

fn exit_without_release(mut child: Child) {
    drop(child.stdin.take().expect("child stdin"));
    let status = child.wait().expect("wait for reader exit");
    assert!(status.success(), "reader exit status {status:?}");
}

fn evidence_path(root: &Path, lease: u64) -> PathBuf {
    root.join(format!("lease.{lease}.lock"))
}

fn load(ledger: &FsLedgerStore) -> Lifecycle {
    let loaded = ledger.load(QUOTA).expect("load ledger");
    assert!(loaded.fault.is_none());
    assert!(loaded.orphans.is_empty(), "orphans {:?}", loaded.orphans);
    loaded.lifecycle
}

fn assert_readable(objects: &FsObjectStore, [segment, manifest]: [ObjectId; 2], bytes: &[u8; 3]) {
    let read = |id: ObjectId| {
        let mut file = objects
            .open_object(id.raw())
            .expect("open object")
            .expect("object exists");
        let mut contents = Vec::new();
        file.read_to_end(&mut contents).expect("read object");
        contents
    };
    assert_eq!(read(manifest), segment.raw().to_le_bytes());
    assert_eq!(read(segment), bytes);
}

#[test]
fn dead_charge_survives_body_refusal_and_is_settled_once_after_restart() {
    if let Some(root) = std::env::var_os(READER_ROOT) {
        child_reader(Path::new(&root));
    }
    let dir = tempfile::tempdir().expect("disposable cache");
    #[cfg(unix)]
    let root = dir.path().join("cache\n\x1b\"");
    #[cfg(not(unix))]
    let root = dir.path().join("cache");
    fs::create_dir(&root).expect("create disposable cache");
    let (ledger, objects) = open(&root);
    let old = publish(&ledger, &objects, b"old");
    let (child, dead_path) = start_reader(&root);
    let live = publish(&ledger, &objects, b"pin");
    let (live_lease, live_hold) = pin(&ledger, &objects);
    let current = publish(&ledger, &objects, b"new");
    assert_readable(&objects, old, b"old");
    assert_readable(&objects, live, b"pin");
    exit_without_release(child);
    assert!(
        dead_path.is_file(),
        "reader left liveness evidence {dead_path:?}"
    );

    let before = fs::read(root.join(LEDGER_FILE)).expect("committed ledger");
    const REFUSAL: &str = "refused\n\x1b\"";
    let refused = ledger.update_reclaiming::<(), _>(QUOTA, &objects, |working| {
        assert_eq!(working.open_leases(), [live_lease]);
        Err(REFUSAL)
    });
    assert!(
        matches!(refused, Err(LedgerUpdateError::Body(REFUSAL))),
        "update refusal {:?}",
        refused.err()
    );
    assert_eq!(
        fs::read(root.join(LEDGER_FILE)).expect("unchanged ledger"),
        before
    );
    assert!(
        dead_path.is_file(),
        "refusal kept liveness evidence {dead_path:?}"
    );
    assert_readable(&objects, old, b"old");
    assert_eq!(load(&ledger).admitted(), 3 * GENERATION_BYTES);
    drop((ledger, objects));

    let (fresh, objects) = open(&root);
    fresh
        .update_reclaiming(QUOTA, &objects, |_| Ok::<_, CacheError>(((), false)))
        .expect("settle dead lease after restart");
    let settled = load(&fresh);
    assert_eq!(settled.open_leases(), [live_lease]);
    assert_eq!(settled.admitted(), 2 * GENERATION_BYTES);
    assert_eq!(settled.status().buckets.pending, 0);
    assert!(!dead_path.exists(), "durable release cleared {dead_path:?}");
    for id in old {
        assert!(settled.object_state(id).is_none());
        assert!(
            objects
                .open_object(id.raw())
                .expect("check reclaimed object")
                .is_none()
        );
    }
    assert!(live_hold.path().is_file());
    assert_readable(&objects, live, b"pin");
    assert_readable(&objects, current, b"new");

    let committed = fs::read(root.join(LEDGER_FILE)).expect("settled ledger");
    fresh
        .update_reclaiming(QUOTA, &objects, |_| Ok::<_, CacheError>(((), false)))
        .expect("retry settled update");
    assert_eq!(
        fs::read(root.join(LEDGER_FILE)).expect("ledger after retry"),
        committed
    );
    fresh
        .update_reclaiming(QUOTA, &objects, |life| {
            life.reader_release(live_lease)?;
            Ok::<_, CacheError>(((), true))
        })
        .expect("release control lease");
    drop(live_hold);
}

#[test]
fn dead_evidence_survives_a_failed_commit_of_a_current_lease() {
    let dir = tempfile::tempdir().expect("disposable cache");
    let (ledger, objects) = open(dir.path());
    let current = publish(&ledger, &objects, b"old");
    let (child, dead_path) = start_reader(dir.path());
    exit_without_release(child);

    let committed = dir.path().join(LEDGER_FILE);
    let saved = dir.path().join("saved-ledger");
    let failed = ledger.update_reclaiming(QUOTA, &objects, |working| {
        assert!(working.open_leases().is_empty());
        // Force the store's base-image check to refuse this commit, including when run as root.
        fs::rename(&committed, &saved).expect("temporarily move ledger");
        Ok::<_, CacheError>(((), true))
    });
    fs::rename(saved, committed).expect("restore committed ledger");
    assert!(matches!(
        failed,
        Err(LedgerUpdateError::Cache(CacheError::InvalidRequest {
            reason: "stale ledger"
        }))
    ));
    assert!(dead_path.is_file(), "failed store kept {dead_path:?}");
    assert_eq!(load(&ledger).status().active_leases, 1);

    let (fresh, objects) = open(dir.path());
    fresh
        .update_reclaiming(QUOTA, &objects, |_| Ok::<_, CacheError>(((), false)))
        .expect("release current lease after restart");
    assert!(!dead_path.exists(), "durable release cleared {dead_path:?}");
    let settled = load(&fresh);
    assert!(settled.open_leases().is_empty());
    assert_eq!(settled.admitted(), GENERATION_BYTES);
    assert_readable(&objects, current, b"old");
}

#[test]
fn missing_and_unreadable_evidence_preserve_legacy_pins() {
    let dir = tempfile::tempdir().expect("disposable cache");
    let (ledger, objects) = open(dir.path());
    let old = publish(&ledger, &objects, b"old");
    let (lease, _) = ledger
        .update_reclaiming(QUOTA, &objects, |life| {
            let lease = life.reader_begin(ROOT, SessionId::new(1))?;
            life.reader_confirm(lease)?;
            Ok::<_, CacheError>((lease, true))
        })
        .expect("persist legacy lease without evidence");
    let current = publish(&ledger, &objects, b"new");
    assert_eq!(load(&ledger).open_leases(), [lease]);
    let evidence = evidence_path(dir.path(), lease.raw());
    fs::create_dir(&evidence).expect("create invalid evidence");
    ledger
        .update_reclaiming(QUOTA, &objects, |_| Ok::<_, CacheError>(((), true)))
        .expect("update with invalid evidence");

    let persisted = load(&ledger);
    assert_eq!(persisted.open_leases(), [lease]);
    assert_eq!(persisted.admitted(), 2 * GENERATION_BYTES);
    assert!(evidence.is_dir());
    assert_readable(&objects, old, b"old");
    assert_readable(&objects, current, b"new");
}
