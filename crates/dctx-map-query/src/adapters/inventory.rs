// SPDX-License-Identifier: Apache-2.0
//! Metadata-first inventory with persisted directory and privacy-rule stamps.
//! Implements SPEC-MAP-MODE sections 6.1 and 7 using the scan domain's policies.
//! `FileSource` reads and hashes only flagged authorized files through the fs port.
use super::source::{FileSource, digest, forbidden};
use crate::{
    Error, Result,
    ports::{FileStamp, RuleFile, Snapshot, SourceFile},
};
use dctx_adapter_fs::ports::{EntryMeta, FsKind};
use dctx_map_scan::domain::classifier::{FileContentHandling, classify_file};
use dctx_map_scan::ports::RuleSource;
use dctx_map_scan::{FileHandlingDecision, IgnoreEngine, ItemContext};
use std::{collections::BTreeMap, io, path::Path};

const RULES: [&str; 4] = [
    ".gitignore",
    ".dctxignore",
    ".legacyignore",
    ".dctxomitcontent",
];

pub(super) fn stamp_of(meta: EntryMeta) -> FileStamp {
    FileStamp {
        size: meta.size,
        modified: i64::try_from(meta.modified_ns).unwrap_or(i64::MAX),
        changed: 0,
        identity: u128::from(meta.inode.unwrap_or(0)),
    }
}

pub(super) fn optional_stamp(source: &FileSource, path: &Path) -> Result<Option<FileStamp>> {
    match source.metadata(path) {
        Ok(meta) if meta.kind == FsKind::Symlink => Err(Error::Denied(path.display().to_string())),
        Ok(meta) => Ok(Some(stamp_of(meta))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

struct Rules<'a>(&'a BTreeMap<String, RuleFile>);

impl RuleSource for Rules<'_> {
    fn read_rules(&self, path: &Path) -> io::Result<Option<String>> {
        Ok(path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| self.0.get(name))
            .and_then(|rule| rule.text.clone()))
    }
}

pub(super) fn engine(root: &Path, rules: &BTreeMap<String, RuleFile>) -> Result<IgnoreEngine> {
    Ok(IgnoreEngine::build_with_source(root, None, &Rules(rules))?)
}

pub(super) fn capture(source: &FileSource, previous: Option<&Snapshot>) -> Result<Snapshot> {
    let mut rules = BTreeMap::new();
    let mut same_rules = previous.is_some();
    for name in RULES {
        let path = source.root.join(name);
        let current = optional_stamp(source, &path)?;
        let old = previous.and_then(|snapshot| snapshot.rules.get(name));
        let rule = if let Some(old) = old.filter(|rule| rule.stamp == current) {
            old.clone()
        } else {
            same_rules = false;
            let text = current
                .as_ref()
                .map(|_| {
                    String::from_utf8(source.read_bytes(&path)?)
                        .map_err(|error| Error::Request(error.to_string()))
                })
                .transpose()?;
            RuleFile {
                stamp: current,
                text,
            }
        };
        rules.insert(name.into(), rule);
    }
    let mut same_directories = same_rules;
    if let Some(previous) = previous {
        for (path, old) in &previous.directories {
            match source.metadata(&source.root.join(path)) {
                Ok(meta) if meta.kind == FsKind::Directory && stamp_of(meta) == *old => {}
                Ok(_) => {
                    same_directories = false;
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    same_directories = false;
                    break;
                }
                Err(error) => return Err(error.into()),
            }
        }
        same_directories &= !previous.directories.is_empty();
    }
    let mut files = BTreeMap::new();
    let directories = if let Some(previous) = previous.filter(|_| same_directories) {
        for (path, old) in &previous.files {
            let meta = source.metadata(&source.root.join(path))?;
            if meta.kind != FsKind::File {
                return Err(Error::Denied(path.clone()));
            }
            files.insert(
                path.clone(),
                capture_file(source, path, old.allowed, meta, Some(old))?,
            );
        }
        previous.directories.clone()
    } else {
        let engine = engine(&source.root, &rules)?;
        walk(source, &engine, previous, &mut files)?
    };
    let mut snapshot = super::revision::snapshot(files)?;
    snapshot.directories = directories;
    snapshot.rules = rules;
    Ok(snapshot)
}

fn capture_file(
    source: &FileSource,
    path: &str,
    allowed: bool,
    meta: EntryMeta,
    old: Option<&SourceFile>,
) -> Result<SourceFile> {
    if meta.kind != FsKind::File {
        return Err(Error::Denied(path.into()));
    }
    let current = stamp_of(meta);
    if let Some(old) = old.filter(|old| old.stamp == current && old.allowed == allowed) {
        if !source.is_strict() {
            let mut file = old.clone();
            file.bytes = None;
            return Ok(file);
        }
        let disk = source.root.join(path);
        let content = source.read_bytes(&disk)?;
        if optional_stamp(source, &disk)?.as_ref() != Some(&current) {
            return Err(Error::Request(format!("source changed during scan: {path}")));
        }
        let hash = digest(&content)?;
        if hash == old.hash {
            let mut file = old.clone();
            file.bytes = None;
            return Ok(file);
        }
        return Ok(sourced(path, content, hash, allowed, current));
    }
    let mut bytes = None;
    let mut hash = String::new();
    let mut readable = false;
    if allowed {
        let disk = source.root.join(path);
        let content = source.read_bytes(&disk)?;
        if optional_stamp(source, &disk)?.as_ref() != Some(&current) {
            return Err(Error::Request(format!("source changed during scan: {path}")));
        }
        hash = digest(&content)?;
        readable = readable_file(path, &content);
        if readable {
            bytes = Some(content);
        }
    }
    Ok(SourceFile {
        bytes,
        hash,
        size: current.size,
        mtime: current.modified,
        readable,
        allowed,
        stamp: current,
    })
}

fn sourced(path: &str, content: Vec<u8>, hash: String, allowed: bool, stamp: FileStamp) -> SourceFile {
    let readable = readable_file(path, &content);
    SourceFile {
        bytes: readable.then_some(content),
        hash,
        size: stamp.size,
        mtime: stamp.modified,
        readable,
        allowed,
        stamp,
    }
}

fn readable_file(path: &str, content: &[u8]) -> bool {
    !matches!(
        classify_file(Path::new(path), content),
        FileContentHandling::OmitBinary { .. }
    )
}

fn walk(
    source: &FileSource,
    engine: &IgnoreEngine,
    previous: Option<&Snapshot>,
    files: &mut BTreeMap<String, SourceFile>,
) -> Result<BTreeMap<String, FileStamp>> {
    let mut directories = BTreeMap::new();
    let mut pending = vec![(String::new(), FileHandlingDecision::Include)];
    while let Some((relative, parent)) = pending.pop() {
        let disk = source.root.join(&relative);
        let meta = source.metadata(&disk)?;
        if meta.kind != FsKind::Directory {
            return Err(Error::Denied(relative));
        }
        directories.insert(relative.clone(), stamp_of(meta));
        let mut entries = source.list(&disk)?;
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        for entry in entries {
            let name = entry
                .name
                .to_str()
                .ok_or_else(|| Error::Denied("non-UTF-8 path".into()))?
                .to_string();
            if forbidden(&name) {
                continue;
            }
            if entry.kind == FsKind::Symlink || entry.kind == FsKind::Other {
                continue;
            }
            let path = if relative.is_empty() {
                name.clone()
            } else {
                format!("{relative}/{name}")
            };
            let decision = engine.decide_with_parent(
                &ItemContext {
                    relative_path: &path,
                    name: &name,
                    is_directory: entry.kind == FsKind::Directory,
                    debug_mode: false,
                },
                parent,
            );
            if !decision.show_in_tree() {
                continue;
            }
            if entry.kind == FsKind::Directory {
                pending.push((path, decision));
            } else {
                let meta = source.metadata(&source.root.join(&path))?;
                let file = capture_file(
                    source,
                    &path,
                    decision.should_read_content(),
                    meta,
                    previous.and_then(|snapshot| snapshot.files.get(&path)),
                )?;
                files.insert(path, file);
            }
        }
    }
    Ok(directories)
}
