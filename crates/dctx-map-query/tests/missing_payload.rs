// SPDX-License-Identifier: Apache-2.0
//! Recovery parity for independent opens of a remembered and an uncached journal.

use dctx_adapter_store::{SegmentRole, SegmentStore, StdFs, StoreError, StoreOptions};
use dctx_map_query::{Error, FileSource, Index, Request, Verb};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

const PLACE: &str = "fixture";
const CURRENT: &str = "roots/fixture/current";
const PREVIOUS: &str = "roots/fixture/previous";
const CHILD_FIXTURE: &str = "DCTX_MISSING_PAYLOAD_CHILD_FIXTURE";
const PARITY_TEST: &str = "warm_and_cold_reopens_select_the_same_closure";

// The journal cache has one process-wide slot.
static CACHE_TEST: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct JournalSelection {
    generation: u64,
    records: Vec<(Value, i64)>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
enum Selection {
    Journal(JournalSelection),
    CorruptPointer,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
struct Reopen {
    selected: Selection,
    pointer: Vec<u8>,
}

struct Closure {
    journal: JournalSelection,
    pointer: Vec<u8>,
    manifest: String,
    view: [u8; 32],
}

struct Fixture {
    source: FileSource,
    database: PathBuf,
    previous: Closure,
    current: Closure,
}

impl Fixture {
    fn new(directory: &Path, added_file: &str) -> Self {
        let source_dir = directory.join("source");
        let warm_dir = directory.join("warm");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir(&warm_dir).unwrap();
        fs::write(source_dir.join("base.rs"), "pub fn base() {}\n").unwrap();
        let source = FileSource::new(&source_dir).unwrap();
        let database = warm_dir.join("index.sqlite");
        let mut index = Index::open(PLACE, &database).unwrap();
        index.refresh(&source, None).unwrap();
        let previous = closure(&index, &source, &database);
        fs::write(source_dir.join(added_file), "pub fn added() {}\n").unwrap();
        index.refresh(&source, None).unwrap();
        let current = closure(&index, &source, &database);
        drop(index);

        // Refresh writes an interrupted generation before the complete one.
        // Keep the earlier complete generation as the fixture's fallback.
        fs::write(
            database.with_extension("segments").join(PREVIOUS),
            &previous.pointer,
        )
        .unwrap();
        assert_ne!(current.journal.records, previous.journal.records);
        assert_ne!(current.view, previous.view);
        Self {
            source,
            database,
            previous,
            current,
        }
    }
}

fn store(database: &Path) -> SegmentStore<StdFs> {
    let fs = StdFs::open(&database.with_extension("segments")).unwrap();
    SegmentStore::open(fs, StoreOptions::capped(512 * 1024 * 1024)).unwrap()
}

fn hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn selection(index: &Index, source: &FileSource, generation: u64) -> JournalSelection {
    let mut request = Request::new(Verb::Map, ".");
    request.depth = 3;
    let records = index.answer(source, &request).unwrap().records;
    assert!(!records.is_empty());
    let records = records
        .iter()
        .map(|record| (serde_json::to_value(record).unwrap(), record.score))
        .collect();
    JournalSelection {
        generation,
        records,
    }
}

fn closure(index: &Index, source: &FileSource, database: &Path) -> Closure {
    let store = store(database);
    let pin = store.pin(PLACE).unwrap();
    let journal = selection(index, source, pin.generation());
    let view = *pin
        .segments()
        .iter()
        .find(|segment| segment.role() == SegmentRole::View)
        .unwrap()
        .digest();
    let manifest = format!("manifests/{}.man", hex(pin.manifest()));
    store.release(pin).unwrap();
    let pointer = fs::read(database.with_extension("segments").join(CURRENT)).unwrap();
    Closure {
        journal,
        pointer,
        manifest,
        view,
    }
}

fn observe(database: &Path, source: &FileSource) -> Reopen {
    let selected = match Index::open(PLACE, database) {
        Ok(index) => Selection::Journal(selection(
            &index,
            source,
            store(database).committed(PLACE).unwrap().unwrap(),
        )),
        Err(Error::Segment(StoreError::CorruptIndex { reason: "pointer" })) => {
            Selection::CorruptPointer
        }
        Err(error) => panic!("unexpected open failure {error:?}"),
    };
    let pointer = fs::read(database.with_extension("segments").join(CURRENT)).unwrap();
    Reopen { selected, pointer }
}

fn copy_fixture(from: &Path, to: &Path) {
    fs::create_dir(to).unwrap();
    for file in fs::read_dir(from).unwrap() {
        let file = file.unwrap();
        let destination = to.join(file.file_name());
        if file.file_type().unwrap().is_dir() {
            copy_fixture(&file.path(), &destination);
        } else {
            fs::copy(file.path(), destination).unwrap();
        }
    }
}

fn child_streams(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    format!("stdout {stdout:?}, stderr {stderr:?}")
}

fn cold_reopen(directory: &Path) -> Reopen {
    let output = Command::new(env::current_exe().unwrap())
        .args([PARITY_TEST, "--exact", "--color", "never"])
        .env(CHILD_FIXTURE, directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cold reader exited with {:?}, {}",
        output.status,
        child_streams(&output.stdout, &output.stderr),
    );
    serde_json::from_slice(&fs::read(directory.join("cold-result.json")).unwrap()).unwrap()
}

#[derive(Clone, Copy, Debug)]
enum Damage {
    Healthy,
    MissingCurrentView,
    MissingCurrentManifest,
    MissingBothManifests,
}

#[test]
fn warm_and_cold_reopens_select_the_same_closure() {
    if let Some(directory) = env::var_os(CHILD_FIXTURE) {
        let directory = PathBuf::from(directory);
        let source = FileSource::new(directory.parent().unwrap().join("source")).unwrap();
        let result = observe(&directory.join("cold/index.sqlite"), &source);
        fs::write(
            directory.join("cold-result.json"),
            serde_json::to_vec(&result).unwrap(),
        )
        .unwrap();
        return;
    }
    let _guard = CACHE_TEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let seed = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let fixture = Fixture::new(seed.path(), "current.rs");
    for damage in [
        Damage::Healthy,
        Damage::MissingCurrentView,
        Damage::MissingCurrentManifest,
        Damage::MissingBothManifests,
    ] {
        let directory = tempfile::tempdir_in(seed.path()).unwrap();
        let warm_dir = directory.path().join("warm");
        let cold_dir = directory.path().join("cold");
        let seed_cache = fixture.database.parent().unwrap();
        copy_fixture(seed_cache, &warm_dir);
        copy_fixture(seed_cache, &cold_dir);
        let warm_database = warm_dir.join("index.sqlite");
        let Selection::Journal(warmed) = observe(&warm_database, &fixture.source).selected else {
            panic!("healthy fixture failed to reopen");
        };
        assert_eq!(warmed, fixture.current.journal);

        for cache in [warm_dir, cold_dir] {
            let segments = cache.join("index.segments");
            match damage {
                Damage::Healthy => {}
                Damage::MissingCurrentView => {
                    fs::remove_file(
                        segments.join(format!("segments/{}.seg", hex(&fixture.current.view))),
                    )
                    .unwrap();
                }
                Damage::MissingCurrentManifest | Damage::MissingBothManifests => {
                    fs::remove_file(segments.join(&fixture.current.manifest)).unwrap();
                    if matches!(damage, Damage::MissingBothManifests) {
                        fs::remove_file(segments.join(&fixture.previous.manifest)).unwrap();
                    }
                }
            }
        }
        let warm = observe(&warm_database, &fixture.source);
        let cold = cold_reopen(directory.path());
        let (selected, pointer) = match damage {
            Damage::Healthy => (
                Selection::Journal(fixture.current.journal.clone()),
                &fixture.current.pointer,
            ),
            Damage::MissingCurrentView | Damage::MissingCurrentManifest => (
                Selection::Journal(fixture.previous.journal.clone()),
                &fixture.previous.pointer,
            ),
            Damage::MissingBothManifests => (Selection::CorruptPointer, &fixture.current.pointer),
        };
        assert_eq!(cold.selected, selected, "{damage:?}");
        assert_eq!(&cold.pointer, pointer, "{damage:?}");
        assert_eq!(warm, cold, "{damage:?}");
    }
}

#[test]
fn a_different_manifest_at_the_same_generation_replaces_the_remembered_journal() {
    let _guard = CACHE_TEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let original = Fixture::new(&directory.path().join("original"), "original.rs");
    let replacement = Fixture::new(&directory.path().join("replacement"), "replacement.rs");
    assert_eq!(
        original.current.journal.generation,
        replacement.current.journal.generation
    );
    assert_ne!(original.current.manifest, replacement.current.manifest);
    assert_ne!(
        original.current.journal.records,
        replacement.current.journal.records
    );
    assert_eq!(
        observe(&original.database, &original.source).selected,
        Selection::Journal(original.current.journal)
    );

    let segments = original.database.with_extension("segments");
    fs::rename(&segments, directory.path().join("retired.segments")).unwrap();
    fs::rename(replacement.database.with_extension("segments"), segments).unwrap();
    fs::copy(&replacement.database, &original.database).unwrap();
    assert_eq!(
        observe(&original.database, &replacement.source),
        Reopen {
            selected: Selection::Journal(replacement.current.journal),
            pointer: replacement.current.pointer,
        }
    );
}

#[test]
fn child_failure_output_escapes_hostile_text() {
    let hostile = b"line\n\x1b[31m\"value";
    assert_eq!(
        child_streams(hostile, hostile),
        r#"stdout "line\n\u{1b}[31m\"value", stderr "line\n\u{1b}[31m\"value""#,
    );
}
