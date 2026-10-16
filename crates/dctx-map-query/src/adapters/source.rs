//! Privacy-filtered filesystem snapshots and selected source reads.
//! Implements SPEC-MAP-MODE sections 6.1 and 7 using the landed scanner.
//! Captures authorized bytes once; the indexer never reopens excluded files.
use crate::{
    Error, Result,
    domain::validate_path,
    ports::{Snapshot, Source, SourceFile},
};
use dctx_map_scan::adapters::{fs::StdFileSystem, rules::FileRuleSource};
use dctx_map_scan::ports::{
    ContentSource,
    fs::{Entry, FileKind, FileSystem},
};
use dctx_map_scan::{FileHandlingDecision, IgnoreEngine, ItemContext, content_hash};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

/// Read-only place source; cache paths are chosen separately.
pub struct FileSource {
    pub(crate) root: PathBuf,
}
impl FileSource {
    /// Canonicalizes a directory root without changing its contents.
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(Error::Request("place must be a directory".into()));
        }
        Ok(Self { root })
    }
    /// Canonical place directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn engine(&self) -> Result<IgnoreEngine> {
        Ok(IgnoreEngine::build_with_source(
            &self.root,
            None,
            &FileRuleSource,
        )?)
    }
}
impl Source for FileSource {
    fn snapshot(&self) -> Result<Snapshot> {
        let engine = self.engine()?;
        let capture = Capture {
            root: &self.root,
            bytes: RefCell::new(BTreeMap::new()),
        };
        let scanned = dctx_map_scan::scan(&self.root, &engine, &capture, false)?;
        let mut bytes = capture.bytes.into_inner();
        let mut files = BTreeMap::new();
        for file in scanned {
            let path = file
                .path
                .to_str()
                .ok_or_else(|| Error::Denied("non-UTF-8 path".into()))?
                .to_owned();
            let content = if file.decision.should_read_content() {
                bytes.remove(&self.root.join(&file.path))
            } else {
                None
            };
            let mtime = fs::symlink_metadata(self.root.join(&file.path))?
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos().min(i64::MAX as u128) as i64);
            files.insert(
                path,
                SourceFile {
                    bytes: content,
                    hash: file.hash.map(hex).unwrap_or_default(),
                    size: file.size,
                    mtime,
                },
            );
        }
        super::revision::snapshot(&self.root, files)
    }
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        validate_path(path)?;
        let engine = self.engine()?;
        let mut disk = self.root.clone();
        let mut relative = String::new();
        let mut parent = FileHandlingDecision::Include;
        for component in Path::new(path).components() {
            let name = component
                .as_os_str()
                .to_str()
                .ok_or_else(|| Error::Denied(path.into()))?;
            if forbidden(name) {
                return Err(Error::Denied(path.into()));
            }
            disk.push(name);
            if !relative.is_empty() {
                relative.push('/');
            }
            relative.push_str(name);
            let kind = StdFileSystem.kind(&disk)?;
            if kind == FileKind::Other {
                return Err(Error::Denied(path.into()));
            }
            parent = engine.decide_with_parent(
                &ItemContext {
                    relative_path: &relative,
                    name,
                    is_directory: kind == FileKind::Directory,
                    debug_mode: false,
                },
                parent,
            );
            if !parent.should_read_content() {
                return Err(Error::Denied(path.into()));
            }
        }
        Ok(fs::read(disk)?)
    }
}

pub(crate) fn hex(hash: [u8; 32]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}
pub(crate) fn digest(bytes: &[u8]) -> String {
    hex(content_hash(bytes))
}

fn forbidden(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".agent" | ".aws" | ".ssh" | ".gnupg" | ".env" | "credentials" | "credentials.json"
    ) || name.starts_with(".env.")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
}
struct Capture<'a> {
    root: &'a Path,
    bytes: RefCell<BTreeMap<PathBuf, Vec<u8>>>,
}
impl ContentSource for Capture<'_> {
    fn read_content(&self, path: &Path) -> io::Result<Vec<u8>> {
        if !path.starts_with(self.root)
            || path.strip_prefix(self.root).ok().is_some_and(|p| {
                p.components()
                    .any(|c| c.as_os_str().to_str().is_some_and(forbidden))
            })
        {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let bytes = fs::read(path)?;
        self.bytes.borrow_mut().insert(path.into(), bytes.clone());
        Ok(bytes)
    }
}
impl FileSystem for Capture<'_> {
    fn kind(&self, path: &Path) -> io::Result<FileKind> {
        StdFileSystem.kind(path)
    }
    fn size(&self, path: &Path) -> io::Result<u64> {
        StdFileSystem.size(path)
    }
    fn entries(&self, path: &Path) -> io::Result<Vec<Entry>> {
        Ok(StdFileSystem
            .entries(path)?
            .into_iter()
            .filter(|e| !e.name.to_str().is_some_and(forbidden))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secrets_ignored_files_and_symlinks_never_enter_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("lib.rs"), "fn public() {}").unwrap();
        fs::write(dir.path().join(".env"), "SECRET").unwrap();
        fs::write(dir.path().join(".gitignore"), "private.rs\n").unwrap();
        fs::write(dir.path().join("private.rs"), "SECRET").unwrap();
        let source = FileSource::new(dir.path()).unwrap();
        let snapshot = source.snapshot().unwrap();
        assert!(snapshot.files.contains_key("lib.rs"));
        assert!(!snapshot.files.contains_key(".env"));
        assert!(!snapshot.files.contains_key("private.rs"));
        assert!(source.read(".env").is_err());
        assert!(source.read("../escape").is_err());
        assert!(source.read("private.rs").is_err());
        assert!(source.read("lib.rs").is_ok());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path().join("lib.rs"), dir.path().join("link.rs"))
                .unwrap();
            assert!(source.read("link.rs").is_err());
            assert!(!source.snapshot().unwrap().files.contains_key("link.rs"));
        }
    }
}
