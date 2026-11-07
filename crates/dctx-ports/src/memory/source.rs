// SPDX-License-Identifier: Apache-2.0
//! In-memory `Source` with a call log: an ephemeral source and a read spy for tests.
//! Implements the `Source` contract (names-only listing, separate metadata, grant-checked reads)
//! over a map of files and directories; every call is recorded so P1 tests can prove what was touched.
//! Depends on [`crate::source::Source`] and dctx-core types. Used by dctx-pack and dctx-ports tests.

use crate::source::Source;
use dctx_core::datatypes::NodeType;
use dctx_core::error::{DctxError, Resource};
use dctx_core::facts::{EntryMeta, ReadGrant, SourceEntry};
use dctx_core::ids::{RelativePath, Timestamp};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

/// One recorded call on a [`MemorySource`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceCall {
    /// `root_kind()`.
    RootKind,
    /// `entries(path)`.
    Entries(String),
    /// `metadata(path)`.
    Metadata(String),
    /// `read(path)`.
    Read(String),
}

/// An in-memory directory tree rooted at a directory.
#[derive(Debug)]
pub struct MemorySource {
    dirs: BTreeMap<String, u64>,
    files: BTreeMap<String, (Vec<u8>, u64)>,
    unreadable: BTreeSet<String>,
    unlistable: BTreeSet<String>,
    calls: RefCell<Vec<SourceCall>>,
}

impl Default for MemorySource {
    fn default() -> Self {
        Self::new()
    }
}

impl MemorySource {
    /// An empty root directory.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the memory adapter API's existing caller lint behavior."
    )]
    pub fn new() -> Self {
        Self {
            dirs: BTreeMap::from([(String::new(), 0)]),
            files: BTreeMap::new(),
            unreadable: BTreeSet::new(),
            unlistable: BTreeSet::new(),
            calls: RefCell::new(Vec::new()),
        }
    }

    /// Adds (or replaces) a file and creates its parent directories.
    ///
    /// # Errors
    /// Returns `InvalidRequest` if `path` is not a valid relative path or
    /// names the root directory. Returns `ResourceLimit` if the path exceeds
    /// the core path-length limit.
    pub fn insert_file(
        &mut self,
        path: &str,
        bytes: &[u8],
        modified: u64,
    ) -> Result<(), DctxError> {
        let path = RelativePath::parse(path)?;
        if path.is_root() {
            return Err(DctxError::InvalidRequest {
                reason: "the root is a directory",
            });
        }
        self.insert_parents(path.as_str());
        self.files
            .insert(path.as_str().to_string(), (bytes.to_vec(), modified));
        Ok(())
    }

    /// Adds a directory (and its parents) with a modification time.
    ///
    /// # Errors
    /// Returns `InvalidRequest` if `path` is not a valid relative path, or
    /// `ResourceLimit` if it exceeds the core path-length limit.
    pub fn insert_dir(&mut self, path: &str, modified: u64) -> Result<(), DctxError> {
        let path = RelativePath::parse(path)?;
        self.insert_parents(path.as_str());
        self.dirs.insert(path.as_str().to_string(), modified);
        Ok(())
    }

    /// Makes reads of `path` fail with an IO error.
    pub fn fail_reads_of(&mut self, path: &str) {
        self.unreadable.insert(path.to_string());
    }

    /// Makes listing of directory `path` fail with an IO error.
    pub fn fail_listing_of(&mut self, path: &str) {
        self.unlistable.insert(path.to_string());
    }

    /// Every call so far, in order.
    pub fn calls(&self) -> Vec<SourceCall> {
        self.calls.borrow().clone()
    }

    /// Paths whose content was read, in order.
    pub fn reads(&self) -> Vec<String> {
        self.filter_calls(|call| match call {
            SourceCall::Read(path) => Some(path.clone()),
            _ => None,
        })
    }

    /// Directories that were listed, in order.
    pub fn listed(&self) -> Vec<String> {
        self.filter_calls(|call| match call {
            SourceCall::Entries(path) => Some(path.clone()),
            _ => None,
        })
    }

    /// Paths whose metadata was requested, in order.
    pub fn metadata_requests(&self) -> Vec<String> {
        self.filter_calls(|call| match call {
            SourceCall::Metadata(path) => Some(path.clone()),
            _ => None,
        })
    }

    /// True if any call named `path` or a path below it.
    pub fn touched(&self, path: &str) -> bool {
        let below = format!("{path}/");
        self.calls.borrow().iter().any(|call| match call {
            SourceCall::RootKind => false,
            SourceCall::Entries(p) | SourceCall::Metadata(p) | SourceCall::Read(p) => {
                p == path || p.starts_with(&below)
            }
        })
    }

    fn filter_calls(&self, pick: impl Fn(&SourceCall) -> Option<String>) -> Vec<String> {
        self.calls.borrow().iter().filter_map(pick).collect()
    }

    fn insert_parents(&mut self, path: &str) {
        let mut end = 0;
        while let Some(offset) = path[end..].find('/') {
            end += offset;
            self.dirs.entry(path[..end].to_string()).or_insert(0);
            end += 1;
        }
    }

    fn record(&self, call: SourceCall) {
        self.calls.borrow_mut().push(call);
    }
}

fn parent_of(path: &str) -> &str {
    path.rfind('/').map_or("", |index| &path[..index])
}

fn name_of(path: &str) -> &str {
    path.rfind('/').map_or(path, |index| &path[index + 1..])
}

impl Source for MemorySource {
    fn root_kind(&self) -> Result<NodeType, DctxError> {
        self.record(SourceCall::RootKind);
        Ok(NodeType::Directory)
    }

    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, DctxError> {
        let key = dir.as_str();
        self.record(SourceCall::Entries(key.to_string()));
        if self.unlistable.contains(key) {
            return Err(DctxError::io("listing refused"));
        }
        if !self.dirs.contains_key(key) {
            return Err(DctxError::SourceUnavailable);
        }
        let dirs = self
            .dirs
            .keys()
            .filter(|path| !path.is_empty() && parent_of(path) == key)
            .map(|path| SourceEntry::directory(name_of(path)));
        let files = self
            .files
            .keys()
            .filter(|path| parent_of(path) == key)
            .map(|path| SourceEntry::file(name_of(path)));
        Ok(dirs.chain(files).collect())
    }

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, DctxError> {
        let key = path.as_str();
        self.record(SourceCall::Metadata(key.to_string()));
        if let Some((bytes, modified)) = self.files.get(key) {
            return Ok(EntryMeta {
                size: bytes.len() as u64,
                modified: Timestamp(*modified),
            });
        }
        match self.dirs.get(key) {
            Some(modified) => Ok(EntryMeta {
                size: 0,
                modified: Timestamp(*modified),
            }),
            None => Err(DctxError::SourceUnavailable),
        }
    }

    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, DctxError> {
        let key = grant.path().as_str();
        self.record(SourceCall::Read(key.to_string()));
        if self.unreadable.contains(key) {
            return Err(DctxError::io("read refused"));
        }
        let (bytes, _) = self.files.get(key).ok_or(DctxError::SourceUnavailable)?;
        if bytes.len() as u64 > max_bytes {
            return Err(DctxError::ResourceLimit {
                resource: Resource::FileBytes,
                limit: max_bytes,
            });
        }
        Ok(bytes.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_core::domain::FileHandlingDecision;

    fn grant(path: &str) -> ReadGrant {
        let path = RelativePath::parse(path).unwrap();
        ReadGrant::authorize(&path, FileHandlingDecision::Include, true).unwrap()
    }

    fn sample() -> MemorySource {
        let mut source = MemorySource::new();
        source
            .insert_file("src/main.rs", b"fn main() {}", 7)
            .unwrap();
        source.insert_file("README.md", b"# x", 5).unwrap();
        source.insert_dir("empty", 3).unwrap();
        source
    }

    #[test]
    fn listing_returns_direct_children_only() {
        let source = sample();
        let root = source.entries(&RelativePath::root()).unwrap();
        let names: Vec<&str> = root.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["empty", "src", "README.md"]);
        assert!(root[1].is_dir());
        let src = source
            .entries(&RelativePath::parse("src").unwrap())
            .unwrap();
        assert_eq!(src, vec![SourceEntry::file("main.rs")]);
    }

    #[test]
    fn metadata_and_reads_are_logged() {
        let source = sample();
        let meta = source
            .metadata(&RelativePath::parse("src/main.rs").unwrap())
            .unwrap();
        assert_eq!(
            meta,
            EntryMeta {
                size: 12,
                modified: Timestamp(7)
            }
        );
        assert_eq!(
            source.read(&grant("src/main.rs"), 100).unwrap(),
            b"fn main() {}"
        );
        assert_eq!(source.reads(), ["src/main.rs"]);
        assert_eq!(source.metadata_requests(), ["src/main.rs"]);
        assert!(source.touched("src"));
        assert!(!source.touched("README.md"));
    }

    #[test]
    fn oversized_and_failing_reads_are_refused_with_positive_control() {
        let mut source = sample();
        assert!(source.read(&grant("README.md"), 3).is_ok());
        assert!(matches!(
            source.read(&grant("README.md"), 2),
            Err(DctxError::ResourceLimit { .. })
        ));
        source.fail_reads_of("README.md");
        assert!(matches!(
            source.read(&grant("README.md"), 3),
            Err(DctxError::Io { .. })
        ));
        source.fail_listing_of("src");
        assert!(source
            .entries(&RelativePath::parse("src").unwrap())
            .is_err());
        assert!(source
            .entries(&RelativePath::parse("missing").unwrap())
            .is_err());
    }

    #[test]
    fn invalid_paths_are_refused() {
        let mut source = MemorySource::new();
        assert!(source.insert_file("../x", b"", 0).is_err());
        assert!(source.insert_file("", b"", 0).is_err());
        assert!(source.insert_file("ok", b"", 0).is_ok());
    }
}
