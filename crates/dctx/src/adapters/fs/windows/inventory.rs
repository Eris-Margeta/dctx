// SPDX-License-Identifier: Apache-2.0
//! One guarded directory stack per traversal, rather than one ancestor walk per file.
use super::{pin_directory, stat_entry};
use crate::adapters::fs::{FsSource, map_io, modified_millis, now_ns, settled_stamp};
use dctx_core::error::{DctxError, Resource};
use dctx_core::facts::{EntryMeta, SourceEntry};
use dctx_core::ids::{RelativePath, Timestamp};
use dctx_ports::source::{Inventory, InventoryEntry};
use std::fs::File;

pub(in crate::adapters::fs) struct Walk<'a> {
    source: &'a FsSource,
    // FsSource already pins the root and its absolute ancestors. Each child
    // needs only one new guard; keep it until leaving that subtree.
    stack: Vec<(RelativePath, File)>,
}

impl<'a> Walk<'a> {
    pub(in crate::adapters::fs) fn new(source: &'a FsSource) -> Self {
        Self {
            source,
            stack: Vec::new(),
        }
    }

    fn pin(&mut self, path: &RelativePath) -> Result<(), DctxError> {
        while self.stack.last().is_some_and(|(parent, _)| {
            !path
                .as_str()
                .strip_prefix(parent.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        }) {
            self.stack.pop();
        }
        let mut prefix = self
            .stack
            .last()
            .map_or_else(RelativePath::root, |(path, _)| path.clone());
        let remaining = path
            .as_str()
            .strip_prefix(prefix.as_str())
            .unwrap_or(path.as_str())
            .trim_start_matches('/');
        for name in remaining.split('/').filter(|name| !name.is_empty()) {
            prefix = prefix.child(name)?;
            let guard = pin_directory(&self.source.confined(&prefix)?).map_err(map_io)?;
            self.stack.push((prefix.clone(), guard));
        }
        Ok(())
    }
}

impl Inventory for Walk<'_> {
    fn entries(&mut self, dir: &RelativePath) -> Result<Vec<InventoryEntry>, DctxError> {
        self.pin(dir)?;
        let full = self.source.confined(dir)?;
        // Read the clock before enumeration, since Windows caches DirEntry
        // metadata in the listing. Never settle a timestamp against a later clock.
        let now = now_ns();
        let read = std::fs::read_dir(&full).map_err(map_io)?;
        let cap = crate::domain::entries_ceiling().map_err(|_| DctxError::InvalidRequest {
            reason: "invalid DCTX_MAX_ENTRIES ceiling",
        })?;
        let mut entries = Vec::new();
        for (visited, item) in read.enumerate() {
            if visited as u64 >= cap {
                return Err(DctxError::ResourceLimit {
                    resource: Resource::Entries,
                    limit: cap,
                });
            }
            let item = item.map_err(map_io)?;
            let Some(name) = item.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if name.is_empty() || name == "." || name == ".." {
                continue;
            }
            let kind = item.file_type().map_err(map_io)?;
            let directory = kind.is_dir() && !kind.is_symlink();
            let entry = if directory {
                SourceEntry::directory(&name)
            } else {
                SourceEntry::file(&name)
            };
            // Directory metadata comes directly from enumeration on Windows.
            // Files still need the existing handle-based stat: enumeration has
            // no file index, and cannot establish READ_ATTRIBUTES permission.
            // Failure falls back to Source::stat after policy selection, keeping
            // the walk's per-file partial/error handling unchanged.
            let stat = if directory {
                item.metadata().ok().map(|meta| (meta, None))
            } else if dir.child(&name).is_ok() {
                stat_entry(&item).ok()
            } else {
                None
            };
            let stat = stat.map(|(meta, inode)| {
                (
                    EntryMeta {
                        size: meta.len(),
                        modified: Timestamp(modified_millis(&meta)),
                    },
                    settled_stamp(&meta, now, inode),
                )
            });
            entries.push(InventoryEntry { entry, stat });
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_ports::Source;
    use std::fs;

    #[test]
    fn traversal_pins_each_ancestor_once_and_releases_finished_subtrees() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("sub/deeper")).unwrap();
        fs::create_dir(temp.path().join("sibling")).unwrap();
        fs::write(temp.path().join("sub/deeper/a.rs"), b"abc").unwrap();
        File::options()
            .write(true)
            .open(temp.path().join("sub/deeper/a.rs"))
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000))
            .unwrap();
        let source = FsSource::open(temp.path()).unwrap();
        let mut walk = Walk::new(&source);
        let sub = RelativePath::parse("sub/deeper").unwrap();
        let entries = walk.entries(&sub).unwrap();
        assert_eq!(walk.stack.len(), 2);
        let file = sub.child("a.rs").unwrap();
        assert_eq!(entries[0].stat.unwrap(), source.stat(&file).unwrap());
        walk.entries(&sub).unwrap();
        assert_eq!(walk.stack.len(), 2, "revisiting does not reopen ancestors");
        assert!(fs::rename(temp.path().join("sub"), temp.path().join("moved")).is_err());
        walk.entries(&RelativePath::parse("sibling").unwrap())
            .unwrap();
        assert_eq!(walk.stack.len(), 1);
        fs::rename(temp.path().join("sub"), temp.path().join("moved")).unwrap();
        drop(walk);
        fs::rename(temp.path().join("sibling"), temp.path().join("released")).unwrap();
    }
}
