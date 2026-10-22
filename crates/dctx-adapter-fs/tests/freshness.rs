//! Freshness, symlink confinement, and same-stamp digest checks.
//! Implements DX-B3 race, escape, and strict-digest contracts (decision 078).
//! Depends on `dctx-adapter-fs` and `tempfile`. These tests drive [`FsSource`].

use dctx_adapter_fs::{
    DirEntry, EntryMeta, FileBody, FreshnessMode, FsKind, FsSource, SourceError, SourceFs,
    SourceOptions, SpyFs, StdFs, StaleReason, Validation,
};
use std::{
    cell::Cell,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

fn root(label: &str) -> PathBuf {
    std::env::temp_dir().join(label)
}

struct Script {
    root: PathBuf,
    entries: Vec<DirEntry>,
    link_target: Option<String>,
    bytes: Vec<u8>,
    race: bool,
    hits: Cell<u32>,
}

impl Script {
    fn new(label: &str, race: bool, link_target: Option<&str>) -> Self {
        let root = root(label);
        Self {
            root,
            entries: vec![
                DirEntry {
                    name: "leak".into(),
                    kind: FsKind::Symlink,
                },
                DirEntry {
                    name: "ok.txt".into(),
                    kind: FsKind::File,
                },
            ],
            link_target: link_target.map(str::to_string),
            bytes: b"ok".to_vec(),
            race,
            hits: Cell::new(0),
        }
    }
}

impl SourceFs for Script {
    fn metadata(&self, path: &Path) -> io::Result<EntryMeta> {
        if path == self.root {
            return Ok(EntryMeta {
                kind: FsKind::Directory,
                size: 0,
                modified_ns: 1,
                inode: Some(1),
            });
        }
        if path == self.root.join("ok.txt") || path == self.root.join("a.txt") {
            let hit = self.hits.get();
            self.hits.set(hit + 1);
            let modified_ns = if self.race && hit > 0 { 20 } else { 10 };
            return Ok(EntryMeta {
                kind: FsKind::File,
                size: self.bytes.len() as u64,
                modified_ns,
                inode: Some(7),
            });
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "meta"))
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        if path != self.root {
            return Err(io::Error::new(io::ErrorKind::NotFound, "dir"));
        }
        if self.race {
            return Ok(vec![DirEntry {
                name: "a.txt".into(),
                kind: FsKind::File,
            }]);
        }
        Ok(self.entries.clone())
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        if path == self.root.join("leak") {
            return Ok(PathBuf::from(
                self.link_target.clone().unwrap_or_else(|| "../secret".to_string()),
            ));
        }
        Err(io::Error::new(io::ErrorKind::InvalidInput, "not a link"))
    }

    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        if path == self.root.join("ok.txt") || path == self.root.join("a.txt") {
            return Ok(self.bytes.clone());
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "file"))
    }

    fn read_policy(&self, _: &Path) -> io::Result<Option<String>> {
        Ok(None)
    }
}

#[test]
fn symlink_escape_is_refused() {
    let escaping = Script::new("dctx-b3-escape", false, Some("../secret"));
    let root = escaping.root.clone();
    let source = FsSource::load(root, SourceOptions::default(), SpyFs::new(escaping)).unwrap();
    let error = source.inventory().unwrap_err();
    assert!(matches!(error, SourceError::Escape { .. }));
    assert!(
        source
            .filesystem()
            .read_files()
            .iter()
            .all(|path| !path.ends_with("secret"))
    );

    let inside = Script::new("dctx-b3-inside", false, Some("ok.txt"));
    let root = inside.root.clone();
    let source = FsSource::load(root, SourceOptions::default(), SpyFs::new(inside)).unwrap();
    let files = source.inventory().unwrap();
    assert!(files.iter().any(|file| file.relative == "ok.txt"));
    assert!(
        source
            .filesystem()
            .read_files()
            .iter()
            .any(|path| path.ends_with("ok.txt"))
    );
}

#[test]
fn race_during_read_is_stale() {
    let script = Script::new("dctx-b3-race", true, None);
    let root = script.root.clone();
    let source = FsSource::load(root, SourceOptions::default(), SpyFs::new(script)).unwrap();
    let files = source.inventory().unwrap();
    assert_eq!(files.len(), 1);
    assert!(matches!(
        files[0].body,
        FileBody::Stale {
            reason: StaleReason::Race
        }
    ));
    assert_eq!(source.filesystem().read_files().len(), 1);
}

#[test]
fn strict_digest_catches_same_size_same_mtime_edit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note.txt");
    fs::write(&path, b"aaaa").unwrap();
    let source = FsSource::load(
        dir.path().to_path_buf(),
        SourceOptions::default(),
        SpyFs::new(StdFs),
    )
    .unwrap();
    let first = source.read("note.txt").unwrap();
    assert_eq!(first.bytes, b"aaaa");
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(&path).unwrap().ino()
    };
    {
        let mut file = fs::OpenOptions::new().write(true).truncate(true).open(&path).unwrap();
        file.write_all(b"bbbb").unwrap();
        file.set_modified(modified).unwrap();
    }
    let after = fs::metadata(&path).unwrap();
    assert_eq!(after.len(), 4);
    assert_eq!(after.modified().unwrap(), modified);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(after.ino(), inode);
    }
    source.filesystem().clear();
    let metadata = source
        .validate("note.txt", &first.identity(), FreshnessMode::Metadata)
        .unwrap();
    assert!(matches!(metadata, Validation::MetadataMatch { .. }));
    assert_eq!(source.filesystem().read_files(), [] as [std::path::PathBuf; 0]);
    let strict = source
        .validate("note.txt", &first.identity(), FreshnessMode::Strict)
        .unwrap();
    let again = source.read("note.txt").unwrap();
    assert_eq!(again.bytes, b"bbbb");
    match strict {
        Validation::DigestMismatch { digest, .. } => {
            assert_eq!(digest, again.digest);
            assert_ne!(digest, first.digest);
            assert_eq!(digest.byte_length(), 4);
        }
        other => panic!("strict mode missed the edit: {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn real_symlink_escape_is_refused_and_a_plain_file_is_read() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("proj");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("ok.txt"), b"ok").unwrap();
    fs::write(temp.path().join("secret.txt"), b"nope").unwrap();
    std::os::unix::fs::symlink("../secret.txt", root.join("leak")).unwrap();
    let source = FsSource::load(root.clone(), SourceOptions::default(), SpyFs::new(StdFs)).unwrap();
    assert!(matches!(source.inventory().unwrap_err(), SourceError::Escape { .. }));
    assert!(
        source
            .filesystem()
            .read_files()
            .iter()
            .all(|path| path.file_name().and_then(|name| name.to_str()) != Some("secret.txt"))
    );

    let clean = temp.path().join("clean");
    fs::create_dir(&clean).unwrap();
    fs::write(clean.join("ok.txt"), b"ok").unwrap();
    let source = FsSource::load(clean, SourceOptions::default(), SpyFs::new(StdFs)).unwrap();
    let files = source.inventory().unwrap();
    assert!(files.iter().any(|file| file.relative == "ok.txt"));
    assert!(source.filesystem().read_files().iter().any(|path| path.ends_with("ok.txt")));
}
