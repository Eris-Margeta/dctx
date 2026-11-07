// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
//! Parallel ignore-aware traversal. No regular source file is opened.

use crate::{Entry, Metadata, Tier};
use ignore::{WalkBuilder, WalkState};
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Collected metadata and walker diagnostics. Errors make a run incomplete.
#[derive(Default)]
pub struct Snapshot {
    pub entries: Vec<Entry>,
    pub directories: Vec<(Vec<u8>, bool)>,
    pub errors: Vec<String>,
    pub symlinks_skipped: u64,
    pub elapsed: Duration,
}

struct Local<'a> {
    snapshot: Snapshot,
    all: &'a Mutex<Vec<Snapshot>>,
}

impl Drop for Local<'_> {
    fn drop(&mut self) {
        self.all
            .lock()
            .unwrap()
            .push(std::mem::take(&mut self.snapshot));
    }
}

/// Walk regular files in parallel, retaining raw Unix path bytes and stat fields.
/// Includes hidden files; honors root/nested `.gitignore` files.
/// Ancestor/global ignores, `.ignore`, and Git info/exclude are disabled.
/// Directories with symlinked `.gitignore`/`.jj` are refused with a diagnostic,
/// because the underlying ignore walker otherwise follows these implicitly.
/// `.git`, `node_modules`, and `target` are pruned at every depth.
/// Symlinks (including symlink files) are excluded; mount boundaries are allowed.
/// Requires a quiescent tree: this path-based walker is not an adversarial-race sandbox.
///
/// # Errors
/// Returns an error for an invalid/symlink root; per-entry errors are in `Snapshot`.
#[allow(
    clippy::too_many_lines,
    reason = "keep the thread-local visitor and its aggregation lifecycle together"
)]
#[allow(
    clippy::missing_panics_doc,
    reason = "collector locks, root-relative paths, and stat nanoseconds are internal walker invariants"
)]
pub fn walk(root: &Path, threads: usize, exclude: Option<&Path>) -> io::Result<Snapshot> {
    if threads == 0 || !fs::symlink_metadata(root)?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "root must be a real directory; threads > 0",
        ));
    }
    let start = Instant::now();
    let root = fs::canonicalize(root)?;
    guard_implicit_links(&root)?;
    let refused = Arc::new(Mutex::new(Vec::new()));
    let refused_ref = Arc::clone(&refused);
    let exclude = exclude.map(Path::to_path_buf);
    let mut builder = WalkBuilder::new(&root);
    builder
        .hidden(false)
        .follow_links(false)
        .threads(threads)
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .parents(false)
        .require_git(false)
        .filter_entry(move |entry| {
            if exclude
                .as_ref()
                .is_some_and(|path| entry.path().starts_with(path))
            {
                return false;
            }
            if entry.file_type().is_some_and(|kind| kind.is_dir())
                && let Err(error) = guard_implicit_links(entry.path())
            {
                refused_ref.lock().unwrap().push(error.to_string());
                return false;
            }
            entry.depth() == 0
                || !matches!(
                    entry.file_name().as_bytes(),
                    b".git" | b"node_modules" | b"target"
                )
        });
    let all = Mutex::new(Vec::new());
    builder.build_parallel().run(|| {
        let mut local = Local {
            snapshot: Snapshot::default(),
            all: &all,
        };
        let root = &root;
        Box::new(move |result| {
            let entry = match result {
                Ok(entry) => entry,
                Err(error) => {
                    local.snapshot.errors.push(error.to_string());
                    return WalkState::Continue;
                }
            };
            if let Some(error) = entry.error() {
                local.snapshot.errors.push(error.to_string());
            }
            let path = entry.path();
            let Some(kind) = entry.file_type() else {
                return WalkState::Continue;
            };
            if kind.is_symlink() {
                local.snapshot.symlinks_skipped += 1;
                return WalkState::Continue;
            }
            let relative = path
                .strip_prefix(root)
                .expect("walker remains beneath root")
                .as_os_str()
                .as_bytes()
                .to_vec();
            if kind.is_dir() {
                let git = match fs::symlink_metadata(path.join(".git")) {
                    Ok(marker) => marker.is_file() || marker.is_dir(),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                    Err(error) => {
                        local.snapshot.errors.push(format!("git marker: {error}"));
                        false
                    }
                };
                local.snapshot.directories.push((relative, git));
            } else if kind.is_file() {
                match fs::symlink_metadata(path) {
                    Ok(metadata) if metadata.is_file() => {
                        let tier = Tier::from_name(entry.file_name().as_bytes());
                        local.snapshot.entries.push(Entry {
                            path: relative,
                            metadata: Metadata {
                                size: metadata.len(),
                                mtime_seconds: metadata.mtime(),
                                mtime_nanos: u32::try_from(metadata.mtime_nsec())
                                    .expect("stat nanoseconds"),
                                inode: metadata.ino(),
                                device: metadata.dev(),
                                tier,
                            },
                        });
                    }
                    Ok(_) => local
                        .snapshot
                        .errors
                        .push("file type changed during walk".into()),
                    Err(error) => local.snapshot.errors.push(error.to_string()),
                }
            }
            WalkState::Continue
        })
    });
    let mut snapshot = Snapshot::default();
    for mut local in all.into_inner().unwrap() {
        snapshot.entries.append(&mut local.entries);
        snapshot.directories.append(&mut local.directories);
        snapshot.errors.append(&mut local.errors);
        snapshot.symlinks_skipped += local.symlinks_skipped;
    }
    snapshot.errors.append(&mut refused.lock().unwrap());
    snapshot.elapsed = start.elapsed();
    Ok(snapshot)
}

// This check is for quiescent trees, as is the underlying path-based walker.
fn guard_implicit_links(directory: &Path) -> io::Result<()> {
    for name in [".gitignore", ".jj"] {
        match fs::symlink_metadata(directory.join(name)) {
            Ok(metadata) if metadata.is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "refusing directory with symlinked {name}: {}",
                        directory.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("ignore preflight {}: {error}", directory.display()),
                ));
            }
        }
    }
    Ok(())
}
