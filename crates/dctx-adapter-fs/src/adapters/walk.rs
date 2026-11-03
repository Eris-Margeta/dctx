// SPDX-License-Identifier: Apache-2.0
//! Inventory walk: selection, then metadata or an authorized hash.
//! Implements DX-B3. Escapes refuse the walk; races become stale records.
//! Depends on [`super::read`] and [`crate::domain`]. Ignored directories are not listed.

use super::error::SourceError;
use super::link::{LinkTarget, classify_link};
use super::path::{component_name, join_rel};
use super::read::{ReadAttempt, file_stamp, read_authorized};
use crate::domain::{
    FileBody, FileHandlingDecision, FreshnessMode, ItemContext, SelectionPolicy, SourceFile,
};
use crate::ports::{FsKind, SourceFs};
use std::path::{Path, PathBuf};

struct Frame {
    disk: PathBuf,
    relative: String,
    kind: FsKind,
    parent: FileHandlingDecision,
}

pub(crate) fn inventory<F: SourceFs>(
    root: &Path,
    policy: &SelectionPolicy,
    fs: &F,
    debug: bool,
    mode: FreshnessMode,
) -> Result<Vec<SourceFile>, SourceError> {
    let mut records = Vec::new();
    let mut stack = vec![Frame {
        disk: root.to_path_buf(),
        relative: String::new(),
        kind: FsKind::Directory,
        parent: FileHandlingDecision::Include,
    }];
    while let Some(frame) = stack.pop() {
        let is_root = frame.relative.is_empty();
        let decision = if is_root {
            FileHandlingDecision::Include
        } else {
            let name = frame.relative.rsplit('/').next().unwrap_or("");
            policy.decide_with_parent(
                &ItemContext {
                    relative_path: &frame.relative,
                    name,
                    is_directory: frame.kind == FsKind::Directory,
                    debug_mode: debug,
                },
                frame.parent,
            )
        };
        if !is_root && !decision.show_in_tree() {
            continue;
        }
        match frame.kind {
            FsKind::Directory => {
                if !is_root && !decision.should_recurse() {
                    continue;
                }
                push_children(fs, &frame, decision, &mut stack)?;
            }
            FsKind::File => {
                records.push(take_file(fs, &frame, decision, mode)?);
            }
            FsKind::Symlink => {
                take_link(root, fs, &frame, decision, mode, &mut records)?;
            }
            FsKind::Other => {}
        }
    }
    records.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(records)
}

fn push_children<F: SourceFs>(
    fs: &F,
    frame: &Frame,
    parent: FileHandlingDecision,
    stack: &mut Vec<Frame>,
) -> Result<(), SourceError> {
    let mut entries = fs
        .read_dir(&frame.disk)
        .map_err(|source| SourceError::io(&frame.disk, source))?;
    entries.sort_by(|left, right| right.name.cmp(&left.name));
    for entry in entries {
        let name = component_name(&entry.name, &frame.disk)?;
        stack.push(Frame {
            disk: frame.disk.join(name),
            relative: join_rel(&frame.relative, name),
            kind: entry.kind,
            parent,
        });
    }
    Ok(())
}

fn take_file<F: SourceFs>(
    fs: &F,
    frame: &Frame,
    decision: FileHandlingDecision,
    mode: FreshnessMode,
) -> Result<SourceFile, SourceError> {
    let body = if decision.should_read_content() && mode == FreshnessMode::Strict {
        match read_authorized(fs, &frame.disk)? {
            ReadAttempt::Stable(stable) => FileBody::Digested {
                stamp: stable.stamp,
                digest: stable.digest,
            },
            ReadAttempt::Stale(reason) => FileBody::Stale { reason },
        }
    } else {
        FileBody::Metadata {
            stamp: file_stamp(fs, &frame.disk)?,
        }
    };
    Ok(SourceFile {
        relative: frame.relative.clone(),
        decision,
        body,
    })
}

fn take_link<F: SourceFs>(
    root: &Path,
    fs: &F,
    frame: &Frame,
    decision: FileHandlingDecision,
    mode: FreshnessMode,
    records: &mut Vec<SourceFile>,
) -> Result<(), SourceError> {
    if !decision.should_read_content() {
        records.push(take_file(fs, frame, decision, mode)?);
        return Ok(());
    }
    match classify_link(root, &frame.disk, fs)? {
        LinkTarget::Outside { path, target } => Err(SourceError::Escape { path, target }),
        LinkTarget::Inside => Ok(()),
    }
}
