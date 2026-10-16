//! Base revision and dirty overlay identity from read-only Git commands.
//! Implements SPEC-MAP-MODE sections 6.4 and 7 for filesystem sources.
//! Uses an explicit place root; non-Git trees receive a content identity.
use crate::{
    Result,
    ports::{Snapshot, SourceFile},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    process::Command,
};

fn git(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}
pub(super) fn snapshot(root: &Path, files: BTreeMap<String, SourceFile>) -> Result<Snapshot> {
    let revision = git(root, &["rev-parse", "--verify", "HEAD"])
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| s.len() >= 7 && s.bytes().all(|b| b.is_ascii_hexdigit()));
    let Some(revision) = revision else {
        let mut identities = Vec::new();
        for (path, file) in &files {
            identities.extend_from_slice(path.as_bytes());
            identities.push(0);
            identities.extend_from_slice(file.hash.as_bytes());
        }
        return Ok(Snapshot {
            revision: super::source::digest(&identities),
            files,
            dirty: BTreeSet::new(),
            recent: BTreeSet::new(),
        });
    };
    let mut dirty = BTreeSet::new();
    if let Some(bytes) = git(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    ) {
        let mut fields = bytes.split(|b| *b == 0).filter(|b| !b.is_empty());
        while let Some(field) = fields.next() {
            if field.len() < 4 {
                continue;
            }
            dirty.insert(String::from_utf8_lossy(&field[3..]).into_owned());
            if field[..2].iter().any(|b| matches!(b, b'R' | b'C')) {
                fields.next();
            }
        }
    } else {
        return Err(crate::Error::Request(
            "cannot determine Git dirty overlay".into(),
        ));
    }
    let recent = git(
        root,
        &[
            "log",
            "-5",
            "--format=",
            "--name-only",
            "-z",
            "--no-renames",
        ],
    )
    .map(|bytes| {
        bytes
            .split(|b| *b == 0)
            .filter_map(|b| std::str::from_utf8(b).ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    })
    .unwrap_or_default();
    Ok(Snapshot {
        revision,
        files,
        dirty,
        recent,
    })
}
