//! Privacy-filtered filesystem snapshots and selected source reads.
//! Implements SPEC-MAP-MODE sections 6.1 and 7 using the landed scanner.
//! Captures only flagged content; the indexer never reopens excluded files.
use crate::{
    Error, Result,
    domain::validate_path,
    ports::{RuleFile, Snapshot, Source, SourceFile},
};
use dctx_map_scan::adapters::{fs::StdFileSystem, rules::FileRuleSource};
use dctx_map_scan::ports::fs::{FileKind, FileSystem};
use dctx_map_scan::{FileHandlingDecision, IgnoreEngine, ItemContext, content_hash};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// Read-only place source; cache paths are chosen separately.
pub struct FileSource {
    pub(crate) root: PathBuf,
    reads: Cell<usize>,
    rules: RefCell<BTreeMap<String, RuleFile>>,
}
impl FileSource {
    /// Canonicalizes a directory root without changing its contents.
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(Error::Request("place must be a directory".into()));
        }
        Ok(Self {
            root,
            reads: Cell::new(0),
            rules: RefCell::new(BTreeMap::new()),
        })
    }
    /// Canonical place directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Actual content opens, including ignore rules and explicit source reads.
    pub fn content_reads(&self) -> usize {
        self.reads.get()
    }

    pub(super) fn read_bytes(&self, path: &Path) -> Result<Vec<u8>> {
        self.reads.set(self.reads.get() + 1);
        Ok(fs::read(path)?)
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
        self.snapshot_since(None)
    }
    fn snapshot_since(&self, previous: Option<&Snapshot>) -> Result<Snapshot> {
        let snapshot = super::inventory::capture(self, previous)?;
        *self.rules.borrow_mut() = snapshot.rules.clone();
        Ok(snapshot)
    }
    fn unchanged(&self, path: &str, file: &SourceFile) -> Result<bool> {
        validate_path(path)?;
        for (name, rule) in self.rules.borrow().iter() {
            if super::inventory::optional_stamp(&self.root.join(name))? != rule.stamp {
                return Ok(false);
            }
        }
        let mut disk = self.root.clone();
        for component in Path::new(path).components() {
            disk.push(component);
            if fs::symlink_metadata(&disk)?.is_symlink() {
                return Ok(false);
            }
        }
        Ok(super::inventory::optional_stamp(&disk)?.as_ref() == Some(&file.stamp))
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
        self.read_bytes(&disk)
    }
}

pub(crate) fn hex(hash: [u8; 32]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}
pub(crate) fn digest(bytes: &[u8]) -> String {
    hex(content_hash(bytes))
}

pub(super) fn forbidden(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".agent" | ".aws" | ".ssh" | ".gnupg" | ".env" | "credentials" | "credentials.json"
    ) || name.starts_with(".env.")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
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
