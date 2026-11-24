// SPDX-License-Identifier: Apache-2.0
//! A process exit tests live filesystem visibility. The Unix sync trace tests the
//! durability prerequisites separately, in the order the native adapter attempts them.

#![cfg(any(unix, windows))]

mod common;

use std::path::Path;
use std::process::{Command, Output};

use common::{digest_of, req, segment};
use dctx_adapter_store::ports::{FsCapabilities, StoreFs};
use dctx_adapter_store::{
    Boundary, PublishRequest, SegmentBlob, SegmentRole, SegmentStore, StdFs, StoreError,
    StoreOptions,
};

const CHILD_MODE: &str = "DCTX_NATIVE_PUBLISH_CHILD";
const CHILD_ROOT: &str = "DCTX_NATIVE_PUBLISH_ROOT";
const CHILD_CASE: &str = "DCTX_NATIVE_PUBLISH_CASE";
const CUT_EXIT: i32 = 37;

struct NativeWithCuts {
    inner: StdFs,
    cut: Box<dyn Fn(Boundary) + Send + Sync>,
}

macro_rules! forward_to_native {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        $(fn $name(&self, $($arg: $ty),*) -> $ret {
            self.inner.$name($($arg),*)
        })*
    };
}

impl StoreFs for NativeWithCuts {
    fn checkpoint(&self, boundary: Boundary) -> Result<(), StoreError> {
        (self.cut)(boundary);
        self.inner.checkpoint(boundary)
    }

    forward_to_native! {
        capabilities() -> FsCapabilities;
        try_lock(root: &str) -> Result<(), StoreError>;
        unlock(root: &str) -> Result<(), StoreError>;
        lock_held(root: &str) -> bool;
        held_roots() -> Vec<String>;
        create(path: &str) -> Result<(), StoreError>;
        write(path: &str, bytes: &[u8]) -> Result<(), StoreError>;
        flush(path: &str) -> Result<(), StoreError>;
        sync_file(path: &str) -> Result<(), StoreError>;
        sync_dir() -> Result<(), StoreError>;
        rename_new(from: &str, to: &str) -> Result<(), StoreError>;
        rename_replace(from: &str, to: &str) -> Result<(), StoreError>;
        read(path: &str) -> Result<Vec<u8>, StoreError>;
        exists(path: &str) -> Result<bool, StoreError>;
        len(path: &str) -> Result<u64, StoreError>;
        unlink(path: &str) -> Result<(), StoreError>;
        list_prefix(prefix: &str) -> Result<Vec<String>, StoreError>;
    }
}

fn fixture() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("native store fixture")
}

fn open<F: StoreFs>(fs: F) -> SegmentStore<F> {
    SegmentStore::open(fs, StoreOptions::capped(8 * 1024 * 1024)).expect("segment store")
}

fn request(generation: u64) -> PublishRequest {
    let label = if generation == 1 { "old" } else { "new" };
    let mut request = req("main", generation, segment(label));
    request.segments.push(SegmentBlob {
        role: SegmentRole::Fact,
        bytes: segment("shared"),
    });
    request
}

fn assert_generation(store: &SegmentStore<impl StoreFs>, generation: u64) {
    assert_eq!(
        store.raw_current_generation("main").unwrap(),
        Some(generation)
    );
    assert!(store.current_pointer_closed("main").unwrap());
    let pin = store.pin("main").unwrap();
    assert_eq!(pin.generation(), generation);
    let expected = request(generation).segments;
    assert_eq!(pin.segments().len(), expected.len());
    for blob in expected {
        let digest = digest_of(&blob.bytes);
        assert_eq!(store.read_segment(&pin, &digest).unwrap(), blob.bytes);
    }
    store.release(pin).unwrap();
}

#[test]
fn process_exit_visibility_at_name_publication_cuts() {
    if let Some(mode) = std::env::var_os(CHILD_MODE) {
        let root = std::env::var_os(CHILD_ROOT).expect("child root");
        let case = std::env::var(CHILD_CASE).expect("child case");
        let fs = StdFs::open(Path::new(&root)).unwrap();
        match mode.to_str() {
            Some("writer") => {
                let store = open(NativeWithCuts {
                    inner: fs,
                    cut: Box::new(move |boundary| {
                        if boundary.as_str() == case {
                            std::process::exit(CUT_EXIT);
                        }
                    }),
                });
                store.publish(request(2)).unwrap();
                panic!("writer missed the requested cut");
            }
            Some("reader") => assert_generation(&open(fs), case.parse().expect("generation")),
            _ => panic!("unknown child mode {mode:?}"),
        }
        return;
    }

    use Boundary::*;
    for (boundary, generation) in [
        (AfterSegmentRename, "1"),
        (AfterSegmentDirSync, "1"),
        (AfterManifestRename, "1"),
        (AfterManifestDirSync, "1"),
        (AfterPreviousRetain, "1"),
        (AfterPointerSync, "1"),
        (AfterPointerRename, "2"),
        (AfterPointerDirSync, "2"),
    ] {
        let fixture = fixture();
        let root = fixture.path();
        open(StdFs::open(root).unwrap())
            .publish(request(1))
            .unwrap();
        let writer = child(root, "writer", boundary.as_str());
        assert_eq!(
            writer.status.code(),
            Some(CUT_EXIT),
            "cut {boundary:?}: {}",
            diagnostic(&writer)
        );

        let reader = child(root, "reader", generation);
        assert!(
            reader.status.success(),
            "cut {boundary:?}: {}",
            diagnostic(&reader)
        );
    }
}

fn child(root: &Path, mode: &str, case: &str) -> Output {
    Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "process_exit_visibility_at_name_publication_cuts",
            "--nocapture",
        ])
        .env(CHILD_ROOT, root)
        .env(CHILD_MODE, mode)
        .env(CHILD_CASE, case)
        .output()
        .expect("run native publication child")
}

fn diagnostic(output: &Output) -> String {
    format!(
        "status {:?}, stdout {:?}, stderr {:?}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::io;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    enum Event {
        Cut(Boundary),
        Sync(PathBuf),
    }

    type Trace = Arc<Mutex<Vec<Event>>>;

    struct RestorePermissions<'a>(&'a Path, fs::Permissions);

    impl Drop for RestorePermissions<'_> {
        fn drop(&mut self) {
            fs::set_permissions(self.0, self.1.clone()).expect("restore fixture permissions");
        }
    }

    fn set_fixture_mode(path: &Path, mode: u32) -> RestorePermissions<'_> {
        let original = fs::metadata(path).unwrap().permissions();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        RestorePermissions(path, original)
    }

    fn traced(
        root: &Path,
        fail: Option<(Boundary, &str)>,
    ) -> (SegmentStore<NativeWithCuts>, Trace) {
        let events = Trace::default();
        let fail = fail.map(|(cut, path)| (cut, PathBuf::from(path)));
        let inner = StdFs::open_with_dir_sync_hook(root, {
            let events = Arc::clone(&events);
            move |path| {
                let mut events = events.lock().unwrap();
                let cut = events.iter().rev().find_map(|event| match event {
                    Event::Cut(cut) => Some(*cut),
                    Event::Sync(_) => None,
                });
                events.push(Event::Sync(path.to_owned()));
                if let Some((boundary, suffix)) = &fail
                    && cut == Some(*boundary)
                    && path.ends_with(suffix)
                {
                    return Err(io::Error::other("injected directory sync failure"));
                }
                Ok(())
            }
        })
        .unwrap();
        let cut = {
            let events = Arc::clone(&events);
            Box::new(move |cut| events.lock().unwrap().push(Event::Cut(cut)))
        };
        (open(NativeWithCuts { inner, cut }), events)
    }

    fn syncs_between(trace: &[Event], from: Boundary, to: Boundary) -> Vec<&Path> {
        let start = trace
            .iter()
            .position(|event| *event == Event::Cut(from))
            .unwrap();
        let after_start = &trace[start + 1..];
        let end = after_start
            .iter()
            .position(|event| *event == Event::Cut(to))
            .unwrap();
        after_start[..end]
            .iter()
            .filter_map(|event| match event {
                Event::Sync(dir) => Some(dir.as_path()),
                Event::Cut(_) => None,
            })
            .collect()
    }

    // Historical sibling name derived from the SHA-256 digest of "store".
    const OLD_MARKER: &str =
        ".dctx-mkdir-824d80d71985f082a26997a8db88b5d1dd45b777d73585d03d236303e21bde97";

    #[test]
    fn refusing_a_missing_store_preserves_unsafe_ancestors() {
        use std::collections::BTreeMap;
        use std::ffi::OsString;
        use std::os::unix::fs::{PermissionsExt, symlink};

        fn snapshot(dir: &Path) -> (u32, BTreeMap<OsString, u32>) {
            let mode = |p: &Path| fs::symlink_metadata(p).unwrap().permissions().mode();
            let entries = fs::read_dir(dir)
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    (entry.file_name(), mode(&entry.path()))
                })
                .collect();
            (mode(dir), entries)
        }

        let fixture = fixture();
        let shared = fixture.path().join("shared");
        let existing = shared.join("existing");
        let link = fixture.path().join("trusted-link");
        fs::create_dir_all(&existing).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o777)).unwrap();
        // Resolving this trusted link exposes the unsafe parent of its target.
        symlink(&existing, &link).unwrap();
        let parents = [fixture.path(), &shared, &existing];
        let before = parents.map(snapshot);
        for parent in [&shared, &link] {
            let root = parent.join("missing/store\n\x1b\"name");
            for _ in 0..2 {
                assert!(matches!(
                    StdFs::open(&root),
                    Err(StoreError::InvalidRequest { reason: "root" })
                ));
                assert_eq!(parents.map(snapshot), before);
            }
        }
    }

    #[test]
    fn existing_store_preserves_matching_sibling_symlink() {
        use std::os::unix::fs::{MetadataExt, symlink};

        let fixture = fixture();
        let root = fixture.path().join("store");
        fs::create_dir(&root).unwrap();
        let sibling = fixture.path().join(OLD_MARKER);
        let target = Path::new(".dctx-pending-directory-sync-v1");
        symlink(target, &sibling).unwrap();
        let inode = fs::symlink_metadata(&sibling).unwrap().ino();

        let store = open(StdFs::open(&root).unwrap());
        store.publish(request(1)).unwrap();
        assert_generation(&store, 1);
        assert_eq!(fs::read_link(&sibling).unwrap(), target);
        assert_eq!(fs::symlink_metadata(&sibling).unwrap().ino(), inode);
    }

    #[test]
    fn missing_store_preserves_file_at_historical_marker_name() {
        let fixture = fixture();
        let root = fixture.path().join("store");
        let sibling = fixture.path().join(OLD_MARKER);
        let bytes = b"unrelated\n\x1b\"file";
        fs::write(&sibling, bytes).unwrap();

        let store = open(StdFs::open(&root).unwrap());
        store.publish(request(1)).unwrap();
        assert_generation(&store, 1);
        assert_eq!(fs::read(sibling).unwrap(), bytes);
    }

    #[test]
    fn publication_syncs_mutated_parents_at_each_barrier() {
        use Boundary::*;

        let fixture = fixture();
        let root = fs::canonicalize(fixture.path())
            .unwrap()
            .join("nested/store");
        let (store, trace) = traced(&root, None);
        store.publish(req("main", 1, segment("initial"))).unwrap();
        {
            let mut trace = trace.lock().unwrap();
            let dirs = syncs_between(&trace, AfterSegmentRename, AfterSegmentDirSync);
            assert_eq!(
                dirs,
                [root.join("segments"), root.join("tmp")]
                    .into_iter()
                    .chain([
                        root.clone(),
                        root.parent().unwrap().to_owned(),
                        fs::canonicalize(fixture.path()).unwrap()
                    ])
                    .collect::<Vec<_>>()
            );
            trace.clear();
        }

        store.publish(req("main", 2, segment("second"))).unwrap();
        let trace = trace.lock().unwrap();
        let assert_dirs = |from, to, expected: &[&str]| {
            let expected: Vec<_> = expected.iter().map(|dir| root.join(dir)).collect();
            assert_eq!(
                syncs_between(&trace, from, to),
                expected,
                "cuts {from:?} to {to:?}"
            );
        };
        assert_dirs(
            AfterSegmentRename,
            AfterSegmentDirSync,
            &["segments", "tmp", ""],
        );
        assert_dirs(
            AfterManifestRename,
            AfterManifestDirSync,
            &["manifests", "tmp", ""],
        );
        assert_dirs(
            AfterManifestDirSync,
            AfterPreviousRetain,
            &[
                "segments",
                "manifests",
                "tmp",
                "",
                "roots/main",
                "roots",
                "tmp",
                "",
            ],
        );
        assert_dirs(
            AfterPointerFlush,
            AfterPointerSync,
            &["segments", "manifests", "tmp", ""],
        );
        assert_dirs(
            AfterPointerRename,
            AfterPointerDirSync,
            &["roots/main", "roots", "tmp", ""],
        );
        assert!(trace.contains(&Event::Cut(BeforeUnlock)));
    }

    #[test]
    fn publication_beneath_a_search_only_ancestor_syncs_only_affected_parents() {
        use Boundary::*;

        let fixture = fixture();
        let ancestor = fs::canonicalize(fixture.path())
            .unwrap()
            .join("search-only");
        let existing = ancestor.join("existing");
        fs::create_dir_all(&existing).unwrap();
        let _restore = set_fixture_mode(&ancestor, 0o100);

        // Check the attempted paths too, so this detects a regression even for a
        // privileged test runner that can open a mode-0100 directory for reading.
        for (root, mut expected_parents) in [
            (existing.clone(), vec![]),
            (
                existing.join("nested/store"),
                vec![existing.join("nested"), existing.clone()],
            ),
        ] {
            let (store, trace) = traced(&root, None);
            store.publish(request(1)).unwrap();
            assert_generation(&store, 1);
            let trace = trace.lock().unwrap();
            let mut expected = vec![root.join("segments"), root.join("tmp"), root.clone()];
            expected.append(&mut expected_parents);
            assert_eq!(
                syncs_between(&trace, AfterSegmentRename, AfterSegmentDirSync),
                expected
            );
            assert!(
                trace
                    .iter()
                    .all(|event| !matches!(event, Event::Sync(dir) if ancestor.starts_with(dir)))
            );
        }
    }

    #[test]
    fn directory_sync_errors_refuse_success_at_each_publication_phase() {
        use Boundary::*;

        for (phase, dir, forbidden_cut, visible) in [
            (AfterSegmentRename, "segments", AfterSegmentDirSync, 1),
            (AfterManifestRename, "manifests", AfterManifestDirSync, 1),
            (AfterManifestDirSync, "roots/main", AfterPreviousRetain, 1),
            (AfterPointerFlush, "segments", AfterPointerRename, 1),
            (AfterPointerRename, "roots/main", AfterPointerDirSync, 2),
            (AfterPointerRename, "roots", AfterPointerDirSync, 2),
        ] {
            let fixture = fixture();
            let root = fixture.path();
            open(StdFs::open(root).unwrap())
                .publish(request(1))
                .unwrap();
            let old_pointer = fs::read(root.join("roots/main/current")).unwrap();
            let (store, trace) = traced(root, Some((phase, dir)));
            let error = store.publish(request(2)).unwrap_err();
            assert!(
                matches!(error, StoreError::Io { ref detail } if detail.contains("injected directory sync failure")),
                "{error:?}"
            );
            assert!(
                !trace.lock().unwrap().contains(&Event::Cut(forbidden_cut)),
                "cut {forbidden_cut:?}"
            );
            if visible == 2 {
                assert_eq!(
                    fs::read(root.join("roots/main/previous")).unwrap(),
                    old_pointer
                );
            }
            assert_generation(&open(StdFs::open(root).unwrap()), visible);
        }
    }

    #[test]
    fn retry_on_reopened_store_syncs_reused_names_before_first_pointer() {
        use Boundary::*;

        let fixture = fixture();
        let root = fixture.path();
        let (store, _) = traced(root, Some((AfterManifestRename, "manifests")));
        assert!(matches!(
            store.publish(request(1)),
            Err(StoreError::Io { .. })
        ));
        drop(store);
        assert!(!root.join("roots/main/current").exists());

        let (store, trace) = traced(root, Some((AfterPointerFlush, "manifests")));
        assert!(matches!(
            store.publish(request(1)),
            Err(StoreError::Io { .. })
        ));
        assert!(!root.join("roots/main/current").exists());
        let trace = trace.lock().unwrap();
        for unexpected in [AfterSegmentRename, AfterManifestRename, AfterPointerRename] {
            assert!(
                !trace.contains(&Event::Cut(unexpected)),
                "cut {unexpected:?}"
            );
        }
        let root = fs::canonicalize(root).unwrap();
        let object_syncs: Vec<_> = trace.iter().filter(|event| {
            matches!(event, Event::Sync(dir) if dir.ends_with("segments") || dir.ends_with("manifests"))
        }).collect();
        assert_eq!(
            object_syncs,
            [
                &Event::Sync(root.join("segments")),
                &Event::Sync(root.join("manifests"))
            ]
        );
        drop(trace);
        drop(store);

        let store = open(StdFs::open(&root).unwrap());
        store.publish(request(1)).unwrap();
        assert_generation(&store, 1);
    }

    #[test]
    fn reopened_writer_retries_failed_store_ancestor_barrier() {
        use Boundary::*;

        let fixture = fixture();
        let outer = fs::canonicalize(fixture.path()).unwrap().join("outer");
        let nested = outer.join("nested");
        let root = nested.join("store");
        let current = root.join("roots/main/current");

        // Each failed attempt leaves its installed object visible for the next writer.
        for (phase, inputs) in [
            (AfterSegmentRename, &["segments", "tmp"][..]),
            (AfterManifestRename, &["manifests", "tmp"][..]),
            (AfterPointerFlush, &["segments", "manifests", "tmp"][..]),
        ] {
            let (store, trace) = traced(&root, Some((phase, "outer")));
            let error = store
                .publish(req("main", 1, segment("initial")))
                .unwrap_err();
            assert!(
                matches!(error, StoreError::Io { ref detail } if detail.starts_with("sync store ancestor:") && detail.contains("injected directory sync failure")),
                "{error:?}"
            );
            assert!(!current.exists());
            let trace = trace.lock().unwrap();
            assert!(!trace.contains(&Event::Cut(AfterPointerRename)));
            let barrier = trace
                .iter()
                .position(|event| *event == Event::Cut(phase))
                .unwrap();
            let expected: Vec<_> = inputs
                .iter()
                .map(|dir| root.join(dir))
                .chain([root.clone(), nested.clone(), outer.clone()])
                .map(Event::Sync)
                .collect();
            assert!(trace[barrier + 1..].starts_with(&expected), "{trace:?}");
        }

        let (store, trace) = traced(&root, None);
        store.publish(req("main", 1, segment("initial"))).unwrap();
        assert!(store.current_pointer_closed("main").unwrap());
        assert_eq!(store.raw_current_generation("main").unwrap(), Some(1));
        let trace = trace.lock().unwrap();
        assert_eq!(
            syncs_between(&trace, AfterPointerFlush, AfterPointerSync),
            [
                root.join("segments"),
                root.join("manifests"),
                root.join("tmp")
            ]
            .into_iter()
            .chain([
                root.clone(),
                nested,
                outer,
                fs::canonicalize(fixture.path()).unwrap()
            ])
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn nonwritable_ancestor_keeps_its_receipt_pending_across_publication() {
        if rustix::process::geteuid().is_root() {
            eprintln!("skipping mode-0555 ancestor test because root bypasses write permissions");
            return;
        }

        let fixture = fixture();
        let base = fs::canonicalize(fixture.path()).unwrap();
        let ancestor = base.join("ancestor");
        let root = ancestor.join("store");
        // Install an ancestor with its own pending receipt for the name in base.
        drop(StdFs::open(&ancestor).unwrap());
        fs::create_dir(&root).unwrap();
        let _restore = set_fixture_mode(&ancestor, 0o555);

        let deny_ancestor_sync = || {
            let base = base.clone();
            StdFs::open_with_dir_sync_hook(&root, move |dir| {
                if dir == base {
                    return Err(rustix::io::Errno::ACCESS.into());
                }
                Ok(())
            })
            .unwrap()
        };
        let error = open(deny_ancestor_sync()).publish(request(1)).unwrap_err();
        assert!(
            matches!(&error, StoreError::Io { detail } if detail.starts_with("sync store ancestor:")),
            "{error:?}"
        );
        assert!(!root.join("roots/main/current").try_exists().unwrap());

        open(StdFs::open(&root).unwrap())
            .publish(request(1))
            .unwrap();

        // Even after successful publication, a fresh opener retries the
        // barrier whose completion file could not be written in the ancestor.
        let reader = deny_ancestor_sync();
        let error = reader.sync_dir().unwrap_err();
        assert!(
            matches!(&error, StoreError::Io { detail } if detail.starts_with("sync store ancestor:")),
            "{error:?}"
        );
        assert_generation(&open(reader), 1);
        assert_eq!(
            fs::metadata(&ancestor).unwrap().permissions().mode() & 0o777,
            0o555
        );
    }

    #[test]
    fn sibling_retries_shared_ancestor_sync_after_failed_installation() {
        let fixture = fixture();
        let base = fs::canonicalize(fixture.path()).unwrap();
        let shared = base.join("shared");
        let first = shared.join("branch/first");
        let sibling = shared.join("branch/sibling");
        let current = sibling.join("roots/main/current");

        // Deny the sync of the directory naming "shared", after it becomes
        // visible. Use the hook so the failure also applies to privileged users.
        let open_attempt = |root: &Path, fail: bool| {
            let attempted = Arc::new(Mutex::new(Vec::new()));
            let trace = Arc::clone(&attempted);
            let base = base.clone();
            let current = current.clone();
            let fs = StdFs::open_with_dir_sync_hook(root, move |dir| {
                trace.lock().unwrap().push(dir.to_owned());
                if dir == base {
                    assert!(!current.try_exists().unwrap());
                    if fail {
                        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
                    }
                }
                Ok(())
            });
            (fs, attempted)
        };

        let (initial, trace) = open_attempt(&first, true);
        assert!(matches!(initial, Err(StoreError::Io { .. })));
        assert_eq!(trace.lock().unwrap().last(), Some(&base));
        assert!(shared.is_dir());

        let (retry, trace) = open_attempt(&sibling, true);
        let result = retry.and_then(|fs| open(fs).publish(request(1)));
        assert!(matches!(result, Err(StoreError::Io { .. })), "{result:?}");
        assert_eq!(trace.lock().unwrap().last(), Some(&base));
        assert!(!current.try_exists().unwrap());

        let (retry, trace) = open_attempt(&sibling, false);
        open(retry.unwrap()).publish(request(1)).unwrap();
        assert!(trace.lock().unwrap().contains(&base));
        let reader = child(&sibling, "reader", "1");
        assert!(reader.status.success(), "{}", diagnostic(&reader));
    }

    #[test]
    fn sync_failure_diagnostic_escapes_hostile_text() {
        let fixture = fixture();
        let root = fixture.path().join("private-fixture\n\x1b\"name");
        let fs = StdFs::open_with_dir_sync_hook(&root, |dir| {
            if dir.ends_with("tmp\"dir") {
                return Err(io::Error::other("bad\n\x1b\"name"));
            }
            Ok(())
        })
        .unwrap();
        fs.create("tmp\"dir/staged").unwrap();
        let error = fs.sync_dir().unwrap_err().to_string();
        assert!(!error.chars().any(char::is_control), "{error:?}");
        assert!(!error.contains("private-fixture"), "{error:?}");
        assert!(
            error.contains(r#""tmp\"dir""#) && error.contains(r#""bad\\n\\u{1b}\"name""#),
            "{error:?}"
        );
    }
}
