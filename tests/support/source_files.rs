// SPDX-License-Identifier: Apache-2.0
//! Stable source-policy inputs: tracked working-tree files, never disposable fixtures.
//! Source archives without Git use a walk that tolerates concurrent removal only.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn files(dir: &Path) -> Vec<PathBuf> {
    files_with_git(dir, Command::new("git"))
}

/// Select the Git executable per call so archive tests need no process-wide environment edits.
pub fn files_with_git(dir: &Path, mut git: Command) -> Vec<PathBuf> {
    let mut files = match git
        .current_dir(dir)
        .args(["ls-files", "--cached", "-z", "--", "."])
        .output()
    {
        Ok(output) if output.status.success() => output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .map(|name| dir.join(path_name(name)))
            .filter(|path| metadata(path).is_some_and(|meta| meta.is_file()))
            .collect(),
        _ => {
            let mut files = Vec::new();
            walk(dir, &mut files);
            files
        }
    };
    files.sort();
    files.dedup();
    files
}

#[cfg(unix)]
fn path_name(bytes: &[u8]) -> &std::ffi::OsStr {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(bytes)
}

#[cfg(not(unix))]
fn path_name(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("Git source paths must be UTF-8")
}

fn present<T>(result: io::Result<T>, path: &Path) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => panic!("scan {}: {error}", path.display()),
    }
}

fn metadata(path: &Path) -> Option<fs::Metadata> {
    present(fs::symlink_metadata(path), path)
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
    let Some(entries) = present(fs::read_dir(dir), dir) else {
        return;
    };
    for entry in entries {
        let Some(entry) = present(entry, dir) else {
            continue;
        };
        let path = entry.path();
        let Some(meta) = metadata(&path) else {
            continue;
        };
        if meta.is_file() {
            files.push(path);
        } else if meta.is_dir() && entry.file_name() != ".git" && entry.file_name() != "target" {
            walk(&path, files);
        }
    }
}
