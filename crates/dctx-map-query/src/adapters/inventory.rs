//! Metadata-first inventory with persisted directory and privacy-rule stamps.
//! Implements SPEC-MAP-MODE sections 6.1 and 7 using the scan domain's policies.
//! FileSource uses this adapter to read and hash only flagged authorized files.
use super::source::{FileSource, digest, forbidden};
use crate::{
    Error, Result,
    ports::{FileStamp, RuleFile, Snapshot, SourceFile},
};
use dctx_map_scan::domain::classifier::{FileContentHandling, classify_file};
use dctx_map_scan::ports::RuleSource;
use dctx_map_scan::{FileHandlingDecision, IgnoreEngine, ItemContext};
use std::{collections::BTreeMap, fs, io, path::Path};

const RULES: [&str; 4] = [
    ".gitignore",
    ".dctxignore",
    ".legacyignore",
    ".dctxomitcontent",
];

pub(super) fn stamp(metadata: &fs::Metadata) -> Result<FileStamp> {
    let mut stamp = FileStamp {
        size: metadata.len(),
        modified: metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos().min(i64::MAX as u128) as i64),
        ..FileStamp::default()
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        stamp.changed =
            i128::from(metadata.ctime()) * 1_000_000_000 + i128::from(metadata.ctime_nsec());
        stamp.identity = (u128::from(metadata.dev()) << 64) | u128::from(metadata.ino());
    }
    Ok(stamp)
}

pub(super) fn optional_stamp(path: &Path) -> Result<Option<FileStamp>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_symlink() => Ok(Some(stamp(&metadata)?)),
        Ok(_) => Err(Error::Denied(path.display().to_string())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

struct Rules(BTreeMap<String, RuleFile>);
impl RuleSource for Rules {
    fn read_rules(&self, path: &Path) -> io::Result<Option<String>> {
        Ok(path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| self.0.get(n))
            .and_then(|r| r.text.clone()))
    }
}

pub(super) fn capture(source: &FileSource, previous: Option<&Snapshot>) -> Result<Snapshot> {
    let mut rules = Rules(BTreeMap::new());
    let mut same_rules = previous.is_some();
    for name in RULES {
        let path = source.root.join(name);
        let current = optional_stamp(&path)?;
        let old = previous.and_then(|s| s.rules.get(name));
        let rule = if let Some(old) = old.filter(|r| r.stamp == current) {
            old.clone()
        } else {
            same_rules = false;
            let text = current
                .as_ref()
                .map(|_| {
                    String::from_utf8(source.read_bytes(&path)?)
                        .map_err(|e| Error::Request(e.to_string()))
                })
                .transpose()?;
            RuleFile {
                stamp: current,
                text,
            }
        };
        rules.0.insert(name.into(), rule);
    }
    let mut same_directories = same_rules;
    if let Some(previous) = previous {
        for (path, old) in &previous.directories {
            let metadata = fs::symlink_metadata(source.root.join(path));
            match metadata {
                Ok(metadata) if metadata.is_dir() && stamp(&metadata)? == *old => {}
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
            let metadata = fs::symlink_metadata(source.root.join(path))?;
            if !metadata.is_file() || metadata.is_symlink() {
                return Err(Error::Denied(path.clone()));
            }
            files.insert(
                path.clone(),
                capture_file(source, path, old.allowed, &metadata, Some(old))?,
            );
        }
        previous.directories.clone()
    } else {
        let engine = IgnoreEngine::build_with_source(&source.root, None, &rules)?;
        walk(source, &engine, previous, &mut files)?
    };
    let mut snapshot = super::revision::snapshot(&source.root, files, previous)?;
    snapshot.directories = directories;
    snapshot.rules = rules.0;
    Ok(snapshot)
}

fn capture_file(
    source: &FileSource,
    path: &str,
    allowed: bool,
    metadata: &fs::Metadata,
    old: Option<&SourceFile>,
) -> Result<SourceFile> {
    if !metadata.is_file() || metadata.is_symlink() {
        return Err(Error::Denied(path.into()));
    }
    let current = stamp(metadata)?;
    if let Some(old) = old.filter(|old| old.stamp == current && old.allowed == allowed) {
        let mut file = old.clone();
        file.bytes = None;
        return Ok(file);
    }
    let mut bytes = None;
    let mut hash = String::new();
    let mut readable = false;
    if allowed {
        let disk = source.root.join(path);
        let content = source.read_bytes(&disk)?;
        if optional_stamp(&disk)?.as_ref() != Some(&current) {
            return Err(Error::Request(format!(
                "source changed during scan: {path}"
            )));
        }
        hash = digest(&content);
        readable = !matches!(
            classify_file(Path::new(path), &content),
            FileContentHandling::OmitBinary { .. }
        );
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
        let metadata = fs::symlink_metadata(&disk)?;
        if !metadata.is_dir() || metadata.is_symlink() {
            return Err(Error::Denied(relative));
        }
        directories.insert(relative.clone(), stamp(&metadata)?);
        for entry in fs::read_dir(&disk)? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| Error::Denied("non-UTF-8 path".into()))?;
            if forbidden(&name) {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() || !(kind.is_dir() || kind.is_file()) {
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
                    is_directory: kind.is_dir(),
                    debug_mode: false,
                },
                parent,
            );
            if !decision.show_in_tree() {
                continue;
            }
            if kind.is_dir() {
                pending.push((path, decision));
            } else {
                let file = capture_file(
                    source,
                    &path,
                    decision.should_read_content(),
                    &fs::symlink_metadata(entry.path())?,
                    previous.and_then(|s| s.files.get(&path)),
                )?;
                files.insert(path, file);
            }
        }
    }
    Ok(directories)
}
