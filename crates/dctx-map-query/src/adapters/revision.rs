// SPDX-License-Identifier: Apache-2.0
//! Place revision from authorized content hashes only.
//! Implements SPEC-MAP-MODE section 7 under decision 078: no Git and no other
//! version-control tool. Freshness pre-filter (mtime and size) stays in inventory.
//! Inventory calls this after that pre-filter and then fills directories and rules.

use crate::{
    Result,
    ports::{Snapshot, SourceFile},
};
use std::collections::{BTreeMap, BTreeSet};

fn content_revision(files: &BTreeMap<String, SourceFile>) -> Result<String> {
    let mut identities = Vec::new();
    for (path, file) in files {
        identities.extend_from_slice(path.as_bytes());
        // NUL keeps a path from merging with the following hash.
        identities.push(0);
        identities.extend_from_slice(file.hash.as_bytes());
    }
    super::source::digest(&identities)
}

pub(super) fn snapshot(files: BTreeMap<String, SourceFile>) -> Result<Snapshot> {
    let revision = content_revision(&files)?;
    Ok(Snapshot {
        revision,
        files,
        dirty: BTreeSet::new(),
        recent: BTreeSet::new(),
        directories: BTreeMap::new(),
        rules: BTreeMap::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::FileStamp;

    fn file(hash: &str, mtime: i64) -> SourceFile {
        SourceFile {
            bytes: None,
            hash: hash.into(),
            size: hash.len() as u64,
            mtime,
            readable: true,
            allowed: true,
            stamp: FileStamp {
                size: hash.len() as u64,
                modified: mtime,
                ..FileStamp::default()
            },
        }
    }

    #[test]
    fn empty_place_revision_is_the_empty_content_digest() {
        let snapshot = snapshot(BTreeMap::new()).unwrap();
        assert_eq!(
            snapshot.revision,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert!(snapshot.dirty.is_empty());
        assert!(snapshot.recent.is_empty());
    }

    #[test]
    fn revision_follows_authorized_hashes_not_mtime_or_root() {
        let mut first = BTreeMap::new();
        first.insert("b.rs".into(), file("bbb", 1));
        first.insert("a.rs".into(), file("aaa", 2));
        let mut second = BTreeMap::new();
        second.insert("a.rs".into(), file("aaa", 9));
        second.insert("b.rs".into(), file("bbb", 8));
        let left = snapshot(first).unwrap();
        let right = snapshot(second).unwrap();
        assert_eq!(left.revision, right.revision);
        assert_eq!(left.revision.len(), 64);

        let mut changed = BTreeMap::new();
        changed.insert("a.rs".into(), file("aaa", 2));
        changed.insert("b.rs".into(), file("ccc", 1));
        let next = snapshot(changed).unwrap();
        assert_ne!(next.revision, left.revision);
    }

    #[test]
    fn prior_overlay_paths_are_not_carried_forward() {
        let mut previous = snapshot(BTreeMap::new()).unwrap();
        previous.dirty.insert("secret.rs".into());
        previous.recent.insert(".env".into());
        let next = snapshot(BTreeMap::new()).unwrap();
        assert_eq!(next.revision, previous.revision);
        assert!(next.dirty.is_empty());
        assert!(next.recent.is_empty());
        assert!(!next.files.contains_key("secret.rs"));
    }
}
