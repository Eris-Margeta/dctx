// SPDX-License-Identifier: Apache-2.0
//! Hostile directory trees through the public scanner and its filesystem port.

use dctx_map_scan::{
    FileHandlingDecision, IgnoreEngine, ScanError, ScannedFile,
    adapters::fs::StdFileSystem,
    content_hash,
    ports::{
        ContentSource,
        fs::{Entry, FileKind, FileSystem},
    },
    scan,
};
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

fn scan_once(root: &Path, fs: &impl FileSystem) -> Result<Vec<ScannedFile>, ScanError> {
    let engine = IgnoreEngine::from_rules(vec![], vec![]);
    let start = Instant::now();
    let result = scan(root, &engine, fs, false);
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(8), "scan took {elapsed:?}");
    if let Ok(records) = &result {
        for record in records {
            assert!(record.path.file_name().is_some(), "{record:?}");
            assert!(
                record
                    .path
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
                "{record:?}"
            );
        }
        assert!(records.windows(2).all(|pair| pair[0].path < pair[1].path));
    }
    result
}

fn scan_twice(root: &Path, fs: &impl FileSystem) -> Vec<ScannedFile> {
    let first = scan_once(root, fs).unwrap();
    assert_eq!(scan_once(root, fs).unwrap(), first);
    first
}

fn record(path: impl Into<PathBuf>, decision: FileHandlingDecision, bytes: &[u8]) -> ScannedFile {
    ScannedFile {
        path: path.into(),
        decision,
        size: bytes.len() as u64,
        hash: Some(content_hash(bytes)),
    }
}

fn assert_io(root: &Path, fs: &impl FileSystem, expected: &Path, kind: io::ErrorKind) {
    for _ in 0..2 {
        match scan_once(root, fs).unwrap_err() {
            ScanError::Io { path, source } => {
                assert_eq!(path, expected);
                assert_eq!(source.kind(), kind);
            }
            error => panic!("expected an IO diagnostic, got {error:?}"),
        }
    }
}

// Keep real filesystem behavior except at the boundary each fixture controls.
struct FixtureFs {
    list: fn(&Path) -> io::Result<Vec<Entry>>,
    read: fn(&Path) -> io::Result<Vec<u8>>,
}

impl Default for FixtureFs {
    fn default() -> Self {
        Self {
            list: |path| StdFileSystem.entries(path),
            read: |path| StdFileSystem.read_content(path),
        }
    }
}

impl ContentSource for FixtureFs {
    fn read_content(&self, path: &Path) -> io::Result<Vec<u8>> {
        (self.read)(path)
    }
}

impl FileSystem for FixtureFs {
    fn kind(&self, path: &Path) -> io::Result<FileKind> {
        StdFileSystem.kind(path)
    }

    fn entries(&self, path: &Path) -> io::Result<Vec<Entry>> {
        (self.list)(path)
    }

    fn size(&self, path: &Path) -> io::Result<u64> {
        StdFileSystem.size(path)
    }
}

#[cfg(unix)]
#[test]
fn symlink_cycles_and_outside_targets_are_never_opened_or_listed() {
    use dctx_map_scan::adapters::spy::{Operation, SpyFileSystem};
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let outside = temp.path().join("outside");
    let nested = root.join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel.txt"), b"outside").unwrap();
    fs::write(nested.join("local.txt"), b"inside").unwrap();
    symlink(".", root.join("self")).unwrap();
    symlink("..", nested.join("parent")).unwrap();
    symlink("second", root.join("first")).unwrap();
    symlink("first", root.join("second")).unwrap();
    symlink(&outside, root.join("outside-dir")).unwrap();
    symlink(outside.join("sentinel.txt"), root.join("outside-file")).unwrap();

    let expected = [record(
        Path::new("nested").join("local.txt"),
        FileHandlingDecision::Include,
        b"inside",
    )];
    for _ in 0..2 {
        let spy = SpyFileSystem::new(StdFileSystem);
        assert_eq!(scan_once(&root, &spy).unwrap(), expected);
        assert_eq!(
            spy.operations(),
            [
                Operation::Kind(root.clone()),
                Operation::List(root.clone()),
                Operation::List(nested.clone()),
                Operation::Open(nested.join("local.txt")),
            ]
        );
    }
}

#[cfg(unix)]
#[test]
fn unreadable_directory_and_file_report_permission_denied() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let blocked = root.join("blocked");
    fs::create_dir(&blocked).unwrap();
    fs::write(blocked.join("child.txt"), b"child").unwrap();
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o100)).unwrap();

    // Privileged test runners can bypass mode bits. Inject the same failure
    // at the port in that case so the diagnostic is still exercised.
    if StdFileSystem.entries(&blocked).is_ok() {
        let denied = FixtureFs {
            list: |path| {
                if path.ends_with("blocked") {
                    return Err(io::ErrorKind::PermissionDenied.into());
                }
                StdFileSystem.entries(path)
            },
            ..FixtureFs::default()
        };
        assert_io(root, &denied, &blocked, io::ErrorKind::PermissionDenied);
    } else {
        assert_io(
            root,
            &StdFileSystem,
            &blocked,
            io::ErrorKind::PermissionDenied,
        );
    }
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700)).unwrap();

    let file = root.join("locked.txt");
    fs::write(&file, b"unreadable").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
    if StdFileSystem.read_content(&file).is_ok() {
        let denied = FixtureFs {
            read: |path| {
                if path.ends_with("locked.txt") {
                    return Err(io::ErrorKind::PermissionDenied.into());
                }
                StdFileSystem.read_content(path)
            },
            ..FixtureFs::default()
        };
        assert_io(root, &denied, &file, io::ErrorKind::PermissionDenied);
    } else {
        assert_io(root, &StdFileSystem, &file, io::ErrorKind::PermissionDenied);
    }
}

#[test]
fn five_mib_text_file_is_hashed_in_full() {
    let temp = tempfile::tempdir().unwrap();
    let mut bytes = vec![b'x'; 5 * 1024 * 1024];
    *bytes.last_mut().unwrap() = b'\n';
    fs::write(temp.path().join("large.txt"), &bytes).unwrap();
    assert_eq!(
        scan_twice(temp.path(), &StdFileSystem),
        [record("large.txt", FileHandlingDecision::Include, &bytes)]
    );
}

#[test]
fn binary_bytes_are_classified_and_hashed_without_decoding() {
    let temp = tempfile::tempdir().unwrap();
    let bytes = b"header\0\xff\x80trailer";
    fs::write(temp.path().join("payload.txt"), bytes).unwrap();
    assert_eq!(
        scan_twice(temp.path(), &StdFileSystem),
        [record(
            "payload.txt",
            FileHandlingDecision::OmitContent,
            bytes
        )]
    );
}

/// True when the filesystem itself refuses a hostile name (EILSEQ, ENAMETOOLONG, EINVAL).
fn fs_refuses(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(92 | 84 | 63 | 36 | 22))
        || error.kind() == std::io::ErrorKind::InvalidFilename
}

#[cfg(unix)]
#[test]
fn invalid_utf8_name_reports_the_original_relative_path() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("nested")).unwrap();
    let name = Path::new("nested").join(OsStr::from_bytes(b"bad-\xff.txt"));
    if let Err(error) = fs::write(temp.path().join(&name), b"contents") {
        assert!(fs_refuses(&error), "{error}");
        eprintln!("skip: fs refuses non-UTF-8 filename: {error}");
        return;
    }
    for _ in 0..2 {
        assert!(matches!(
            scan_once(temp.path(), &StdFileSystem),
            Err(ScanError::InvalidPath(path)) if path == name
        ));
    }
}

#[test]
fn traverses_six_hundred_directory_levels() {
    let temp = tempfile::tempdir().unwrap();
    let directory: PathBuf = std::iter::repeat_n("d", 600).collect();
    let leaf = directory.join("leaf.txt");
    let built = fs::create_dir_all(temp.path().join(&directory))
        .and_then(|()| fs::write(temp.path().join(&leaf), b"bottom"));
    if let Err(error) = built {
        assert!(fs_refuses(&error), "{error}");
        eprintln!("skip: fs refuses 600-level path: {error}");
        return;
    }
    fs::write(temp.path().join("top.txt"), b"top").unwrap();
    assert_eq!(
        scan_twice(temp.path(), &StdFileSystem),
        [
            record(leaf, FileHandlingDecision::Include, b"bottom"),
            record("top.txt", FileHandlingDecision::Include, b"top"),
        ]
    );
}

#[test]
fn ten_thousand_tiny_files_are_complete_and_sorted_with_reversed_enumeration() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for i in (0..10_000).rev() {
        fs::write(root.join(format!("{i:05}.txt")), b"x").unwrap();
    }

    let ascending = FixtureFs {
        list: |path| {
            let mut entries = StdFileSystem.entries(path)?;
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(entries)
        },
        ..FixtureFs::default()
    };
    let descending = FixtureFs {
        list: |path| {
            let mut entries = StdFileSystem.entries(path)?;
            entries.sort_by(|a, b| b.name.cmp(&a.name));
            Ok(entries)
        },
        ..FixtureFs::default()
    };
    let records = scan_once(root, &ascending).unwrap();
    assert_eq!(scan_once(root, &descending).unwrap(), records);
    assert_eq!(records.len(), 10_000);
    let hash = Some(content_hash(b"x"));
    for (i, item) in records.iter().enumerate() {
        assert_eq!(item.path, Path::new(&format!("{i:05}.txt")));
        assert_eq!(item.decision, FileHandlingDecision::Include);
        assert_eq!(item.size, 1);
        assert_eq!(item.hash, hash);
    }
}

#[test]
fn nul_in_a_directory_entry_reports_invalid_input() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("before"), b"prefix").unwrap();
    // The OS cannot create this name. Inject it at enumeration and let the
    // standard content adapter reject the full path without truncating it.
    let with_nul = FixtureFs {
        list: |path| {
            let mut entries = StdFileSystem.entries(path)?;
            entries.push(Entry {
                name: "before\0after.txt".into(),
                kind: FileKind::File,
            });
            Ok(entries)
        },
        ..FixtureFs::default()
    };
    assert_io(
        temp.path(),
        &with_nul,
        &temp.path().join("before\0after.txt"),
        io::ErrorKind::InvalidInput,
    );
}

#[cfg(unix)]
#[test]
fn newline_in_a_name_is_preserved_across_scans() {
    let temp = tempfile::tempdir().unwrap();
    let name = "split\nname.txt";
    fs::write(temp.path().join(name), b"newline in name").unwrap();
    fs::write(temp.path().join("splitname.txt"), b"distinct").unwrap();
    assert_eq!(
        scan_twice(temp.path(), &StdFileSystem),
        [
            record(name, FileHandlingDecision::Include, b"newline in name"),
            record("splitname.txt", FileHandlingDecision::Include, b"distinct"),
        ]
    );
}

#[test]
fn files_growing_and_shrinking_between_stat_and_read_use_the_bytes_read() {
    const SHORT: &[u8] = b"x";
    const LONG: &[u8] = b"more bytes after a resize";

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("changing.txt");
    let changes = FixtureFs {
        read: |path| {
            // The scan uses read length for included files. Force the size
            // change after a real stat and before the actual read.
            let replacement = if StdFileSystem.size(path)? == SHORT.len() as u64 {
                LONG
            } else {
                SHORT
            };
            fs::write(path, replacement)?;
            StdFileSystem.read_content(path)
        },
        ..FixtureFs::default()
    };
    for (before, after) in [(SHORT, LONG), (LONG, SHORT)] {
        for _ in 0..2 {
            fs::write(&path, before).unwrap();
            assert_eq!(
                scan_once(temp.path(), &changes).unwrap(),
                [record("changing.txt", FileHandlingDecision::Include, after)]
            );
            assert_eq!(fs::metadata(&path).unwrap().len(), after.len() as u64);
        }
    }
}
