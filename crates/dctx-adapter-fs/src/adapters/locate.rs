//! Single-path lookup that stops at the selection decision.
//! Implements ADR-014 for `read` and `validate` without scanning sibling content.
//! Depends on [`super::link`] and [`super::path`]. Content reads happen only after this returns.

use super::error::SourceError;
use super::link::{LinkTarget, classify_link};
use super::path::{component_name, join_rel, split_relative};
use crate::domain::{FileHandlingDecision, ItemContext, SelectionPolicy};
use crate::ports::{FsKind, SourceFs};
use std::{ffi::OsStr, io, path::PathBuf};

pub(crate) struct Resolved {
    pub disk: PathBuf,
    pub relative: String,
    pub decision: FileHandlingDecision,
}

pub(crate) enum Located {
    Missing,
    Denied { relative: String },
    File(Resolved),
}

pub(crate) fn locate<F: SourceFs>(
    root: &std::path::Path,
    policy: &SelectionPolicy,
    fs: &F,
    relative: &str,
    debug: bool,
) -> Result<Located, SourceError> {
    let parts = split_relative(relative)?;
    let mut disk = root.to_path_buf();
    let mut current = String::new();
    let mut parent = FileHandlingDecision::Include;
    for (index, name) in parts.iter().enumerate() {
        let entries = match fs.read_dir(&disk) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Located::Missing),
            Err(error) => return Err(SourceError::io(&disk, error)),
        };
        let wanted = OsStr::new(name);
        let Some(entry) = entries.into_iter().find(|entry| entry.name == wanted) else {
            return Ok(Located::Missing);
        };
        component_name(&entry.name, &disk)?;
        current = join_rel(&current, name);
        let decision = policy.decide_with_parent(
            &ItemContext {
                relative_path: &current,
                name,
                is_directory: entry.kind == FsKind::Directory,
                debug_mode: debug,
            },
            parent,
        );
        let child = disk.join(name);
        if entry.kind == FsKind::Symlink {
            return symlink_decision(root, fs, &child, decision, current);
        }
        if !decision.show_in_tree() {
            return Ok(Located::Denied { relative: current });
        }
        let last = index + 1 == parts.len();
        if last {
            if entry.kind != FsKind::File {
                return Err(SourceError::InvalidPath { path: child });
            }
            return Ok(Located::File(Resolved {
                disk: child,
                relative: current,
                decision,
            }));
        }
        if entry.kind != FsKind::Directory || !decision.should_recurse() {
            return Ok(Located::Denied { relative: current });
        }
        parent = decision;
        disk = child;
    }
    Err(SourceError::InvalidPath {
        path: PathBuf::from(relative),
    })
}

fn symlink_decision<F: SourceFs>(
    root: &std::path::Path,
    fs: &F,
    link: &std::path::Path,
    decision: FileHandlingDecision,
    relative: String,
) -> Result<Located, SourceError> {
    if !decision.should_read_content() {
        return Ok(Located::Denied { relative });
    }
    match classify_link(root, link, fs)? {
        LinkTarget::Outside { path, target } => Err(SourceError::Escape { path, target }),
        LinkTarget::Inside => Err(SourceError::InvalidPath {
            path: link.to_path_buf(),
        }),
    }
}
