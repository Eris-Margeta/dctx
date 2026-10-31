//! Privacy-filtered snapshots and selected source reads through the filesystem port.
//! Implements SPEC-MAP-MODE sections 6.1 and 7 using the DX-B3 source adapter.
//! Content opens are counted; metadata and denied paths are not.
use std::fmt::Write as _;
use crate::{
    Error, Result,
    domain::validate_path,
    ports::{RuleFile, Snapshot, Source, SourceFile},
};
use dctx_adapter_fs::ports::{EntryMeta, FsKind, SourceFs};
use dctx_adapter_fs::domain::join_lexical;
use dctx_adapter_fs::StdFs;
use dctx_map_scan::adapters::rules::FileRuleSource;
use dctx_map_scan::{FileHandlingDecision, IgnoreEngine, ItemContext};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

/// Read-only place source. Cache paths are chosen separately.
pub struct FileSource {
    pub(crate) root: PathBuf,
    reads: Cell<usize>,
    opened: RefCell<Vec<PathBuf>>,
    rules: RefCell<BTreeMap<String, RuleFile>>,
    strict: bool,
    fs: StdFs,
}

impl FileSource {
    /// Canonicalizes a directory root without changing its contents.
    ///
    /// # Errors
    /// Refuses a non-directory root or excessive symlink depth; propagates path-resolution and metadata IO failures.
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        Self::open(root.as_ref(), false)
    }

    /// Like [`Self::new`], and hashes bytes again when the metadata pre-filter matches.
    ///
    /// # Errors
    /// Refuses a non-directory root or excessive symlink depth; propagates path-resolution and metadata IO failures.
    pub fn strict(root: impl AsRef<Path>) -> Result<Self> {
        Self::open(root.as_ref(), true)
    }

    fn open(root: &Path, strict: bool) -> Result<Self> {
        let root = canonicalize(root)?;
        Ok(Self {
            root,
            reads: Cell::new(0),
            opened: RefCell::new(Vec::new()),
            rules: RefCell::new(BTreeMap::new()),
            strict,
            fs: StdFs,
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

    /// Content and rule paths opened so far, in order.
    pub fn opened_paths(&self) -> Vec<PathBuf> {
        self.opened.borrow().clone()
    }

    pub(super) fn is_strict(&self) -> bool {
        self.strict
    }

    pub(super) fn metadata(&self, path: &Path) -> std::io::Result<EntryMeta> {
        self.fs.metadata(path)
    }

    pub(super) fn list(&self, path: &Path) -> std::io::Result<Vec<dctx_adapter_fs::DirEntry>> {
        self.fs.read_dir(path)
    }

    pub(super) fn read_bytes(&self, path: &Path) -> Result<Vec<u8>> {
        self.reads.set(self.reads.get() + 1);
        self.opened.borrow_mut().push(path.to_path_buf());
        Ok(self.fs.read_file(path)?)
    }

    fn engine(&self) -> Result<IgnoreEngine> {
        let rules = self.rules.borrow();
        if rules.is_empty() {
            return Ok(IgnoreEngine::build_with_source(
                &self.root,
                None,
                &FileRuleSource,
            )?);
        }
        super::inventory::engine(&self.root, &rules)
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
            if super::inventory::optional_stamp(self, &self.root.join(name))? != rule.stamp {
                return Ok(false);
            }
        }
        let mut disk = self.root.clone();
        for component in Path::new(path).components() {
            disk.push(component);
            if self.metadata(&disk)?.kind == FsKind::Symlink {
                return Ok(false);
            }
        }
        if super::inventory::optional_stamp(self, &disk)?.as_ref() != Some(&file.stamp) {
            return Ok(false);
        }
        if !self.strict {
            return Ok(true);
        }
        Ok(digest(&self.read_bytes(&disk)?)? == file.hash)
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
            let kind = self.metadata(&disk)?.kind;
            if kind == FsKind::Symlink || kind == FsKind::Other {
                return Err(Error::Denied(path.into()));
            }
            parent = engine.decide_with_parent(
                &ItemContext {
                    relative_path: &relative,
                    name,
                    is_directory: kind == FsKind::Directory,
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
    hash.iter().fold(String::new(), |mut text, byte| { write!(text, "{byte:02x}").expect("writing to a String cannot fail"); text })
}

pub(crate) fn digest_bytes(bytes: &[u8]) -> Result<[u8; 32]> {
    let hashed = dctx_hash::Sha256::digest(bytes)
        .map_err(|error| Error::Request(error.to_string()))?;
    Ok(*hashed.as_bytes())
}

pub(crate) fn digest(bytes: &[u8]) -> Result<String> {
    Ok(hex(digest_bytes(bytes)?))
}

#[allow(clippy::case_sensitive_file_extension_comparisons, reason = "Preserve the existing literal suffix policy for source language dispatch and privacy exclusions; case folding would change which paths are parsed or denied.")]
pub(super) fn forbidden(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".agent" | ".aws" | ".ssh" | ".gnupg" | ".env" | "credentials" | "credentials.json"
    ) || name.starts_with(".env.")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || dctx_core::filter::is_secret_shaped_name(name)
}

#[derive(Clone)]
enum Piece {
    Prefix(std::ffi::OsString),
    Root,
    Cur,
    Parent,
    Name(std::ffi::OsString),
}

fn pieces(path: &Path) -> Vec<Piece> {
    path.components()
        .map(|component| match component {
            Component::Prefix(prefix) => Piece::Prefix(prefix.as_os_str().to_os_string()),
            Component::RootDir => Piece::Root,
            Component::CurDir => Piece::Cur,
            Component::ParentDir => Piece::Parent,
            Component::Normal(name) => Piece::Name(name.to_os_string()),
        })
        .collect()
}

fn canonicalize(path: &Path) -> Result<PathBuf> {
    let fs = StdFs;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut pending = pieces(&absolute);
    let mut resolved = PathBuf::new();
    let mut index = 0;
    let mut depth = 0;
    while index < pending.len() {
        let piece = pending[index].clone();
        match piece {
            Piece::Prefix(prefix) => resolved.push(prefix),
            Piece::Root => resolved.push(Component::RootDir),
            Piece::Cur => {}
            Piece::Parent => {
                resolved.pop();
            }
            Piece::Name(name) => {
                let next = resolved.join(&name);
                match fs.metadata(&next) {
                    Ok(meta) if meta.kind == FsKind::Symlink => {
                        depth += 1;
                        if depth > 40 {
                            return Err(Error::Request("symlink depth exceeded".into()));
                        }
                        let target = fs.read_link(&next)?;
                        let joined = if target.is_absolute() {
                            target
                        } else {
                            join_lexical(&resolved, &target)
                        };
                        let mut restarted = pieces(&joined);
                        restarted.extend(pending.iter().skip(index + 1).cloned());
                        pending = restarted;
                        resolved = PathBuf::new();
                        index = 0;
                        continue;
                    }
                    Ok(_) => resolved = next,
                    Err(error) => return Err(error.into()),
                }
            }
        }
        index += 1;
    }
    match fs.metadata(&resolved) {
        Ok(meta) if meta.kind == FsKind::Directory => Ok(resolved),
        Ok(_) => Err(Error::Request("place must be a directory".into())),
        Err(error) => Err(error.into()),
    }
}
