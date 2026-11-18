// SPDX-License-Identifier: Apache-2.0

use dctx_adapter_fs::{
    DirEntry, EntryMeta, FileBody, FreshnessMode, FsSource, SourceError, SourceFs, SourceOptions,
    SpyFs, StaleReason, StdFs, Validation,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

fn temp() -> tempfile::TempDir {
    tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap()
}

#[cfg(unix)]
#[test]
fn symlink_loop_is_skipped_without_opening_either_link() {
    use std::os::unix::fs::symlink;

    let dir = temp();
    symlink("second", dir.path().join("first")).unwrap();
    symlink("first", dir.path().join("second")).unwrap();
    let source = FsSource::load(
        dir.path().to_path_buf(),
        SourceOptions::default(),
        SpyFs::new(StdFs),
    )
    .unwrap();
    assert!(source.inventory().unwrap().is_empty());
    assert!(matches!(
        source.read("first"),
        Err(SourceError::InvalidPath { .. })
    ));
    assert!(source.filesystem().read_files().is_empty());
}

#[cfg(unix)]
#[test]
fn an_escaping_directory_link_is_refused_before_content_io() {
    let dir = temp();
    let root = dir.path().join("source");
    fs::create_dir(&root).unwrap();
    fs::write(dir.path().join("private.txt"), b"outside").unwrap();
    std::os::unix::fs::symlink("..", root.join("outside")).unwrap();
    let source = FsSource::load(root, SourceOptions::default(), SpyFs::new(StdFs)).unwrap();

    assert!(matches!(
        source.read("outside/private.txt"),
        Err(SourceError::Escape { target, .. }) if target == dir.path()
    ));
    assert!(matches!(
        source.inventory(),
        Err(SourceError::Escape { .. })
    ));
    assert!(source.filesystem().read_files().is_empty());
}

#[test]
fn traversal_is_rejected_without_filesystem_operations() {
    let dir = temp();
    let root = dir.path().join("source");
    fs::create_dir(&root).unwrap();
    fs::write(dir.path().join("private.txt"), b"outside").unwrap();
    let source = FsSource::load(root, SourceOptions::default(), SpyFs::new(StdFs)).unwrap();
    source.filesystem().clear();

    for relative in [
        "../private.txt",
        "child/../../private.txt",
        r"..\private.txt",
    ] {
        assert!(matches!(
            source.read(relative),
            Err(SourceError::InvalidPath { .. })
        ));
    }
    assert!(source.filesystem().operations().is_empty());
}

#[test]
fn sparse_file_over_the_content_cap_is_refused_by_read_and_inventory() {
    let dir = temp();
    let path = dir.path().join("large.txt");
    fs::File::create(&path)
        .unwrap()
        .set_len(8 * 1024 * 1024 + 1)
        .unwrap();
    let source = FsSource::open(dir.path(), SourceOptions::default()).unwrap();

    for error in [
        source.read("large.txt").unwrap_err(),
        source.inventory().unwrap_err(),
    ] {
        assert!(matches!(
            error,
            SourceError::Io { path: rejected, source }
                if rejected == path && source.kind() == io::ErrorKind::FileTooLarge
        ));
    }
}

#[cfg(unix)]
#[test]
fn unreadable_file_does_not_become_an_empty_success() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp();
    let path = dir.path().join("unreadable.txt");
    fs::write(&path, b"private").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    // A process with permission to override mode bits cannot exercise this OS refusal.
    if fs::File::open(&path).is_ok() {
        return;
    }
    let source = FsSource::open(dir.path(), SourceOptions::default()).unwrap();

    for error in [
        source.read("unreadable.txt").unwrap_err(),
        source.inventory().unwrap_err(),
    ] {
        assert!(matches!(
            error,
            SourceError::Io { path: rejected, source }
                if rejected == path && source.kind() == io::ErrorKind::PermissionDenied
        ));
    }
}

#[derive(Clone, Copy)]
enum Disturbance {
    Grow,
    Remove,
}

struct ChangingFs(Disturbance);

impl SourceFs for ChangingFs {
    fn metadata(&self, path: &Path) -> io::Result<EntryMeta> {
        StdFs.metadata(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        StdFs.read_dir(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        StdFs.read_link(path)
    }

    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        match self.0 {
            Disturbance::Grow => {
                let bytes = StdFs.read_file(path)?;
                fs::write(path, b"new contents")?;
                Ok(bytes)
            }
            Disturbance::Remove => {
                fs::remove_file(path)?;
                StdFs.read_file(path)
            }
        }
    }

    fn read_policy(&self, path: &Path) -> io::Result<Option<String>> {
        StdFs.read_policy(path)
    }
}

#[test]
fn a_file_changed_or_removed_during_read_is_stale() {
    let dir = temp();
    let path = dir.path().join("changing.txt");
    fs::write(&path, b"old").unwrap();
    let previous = FsSource::open(dir.path(), SourceOptions::default())
        .unwrap()
        .read("changing.txt")
        .unwrap()
        .identity();

    for (disturbance, expected) in [
        (Disturbance::Grow, StaleReason::Race),
        (Disturbance::Remove, StaleReason::Missing),
    ] {
        let source = FsSource::load(
            dir.path().to_path_buf(),
            SourceOptions::default(),
            ChangingFs(disturbance),
        )
        .unwrap();
        fs::write(&path, b"old").unwrap();
        assert!(matches!(
            source.read("changing.txt"),
            Err(SourceError::Stale { reason, .. }) if reason == expected
        ));
        fs::write(&path, b"old").unwrap();
        assert!(matches!(
            source.inventory().unwrap().as_slice(),
            [entry] if entry.body == FileBody::Stale { reason: expected }
        ));
        fs::write(&path, b"old").unwrap();
        assert_eq!(
            source
                .validate("changing.txt", &previous, FreshnessMode::Strict)
                .unwrap(),
            Validation::Stale { reason: expected }
        );
    }
}
