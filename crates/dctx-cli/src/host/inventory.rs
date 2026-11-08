// SPDX-License-Identifier: Apache-2.0
//! One traversal supplies both nested rule discovery and the engine inventory.
//! Only the first snapshot consumes the recorded listing. Later traversals,
//! targeted stats and all content reads use the live confined source.
use super::*;
use dctx::{Inventory, InventoryEntry};
use dctx_core::datatypes::NodeType;
use dctx_core::error::DctxError as CoreError;
use dctx_core::facts::{EntryMeta, EntryStamp, ReadGrant, SourceEntry};
use dctx_core::ids::RelativePath;
use std::collections::BTreeMap;
use std::sync::Mutex;

type Directories = BTreeMap<String, Vec<InventoryEntry>>;

pub(super) struct PreparedSource {
    source: FsSource,
    directories: Mutex<Option<Directories>>,
}

impl PreparedSource {
    pub(super) fn new(source: FsSource, directories: Directories) -> Self {
        Self {
            source,
            directories: Mutex::new(Some(directories)),
        }
    }
}

struct Replay<'a> {
    source: &'a FsSource,
    directories: Directories,
    live: Option<Box<dyn Inventory + 'a>>,
}

impl Inventory for Replay<'_> {
    fn entries(&mut self, dir: &RelativePath) -> Result<Vec<InventoryEntry>, CoreError> {
        if let Some(entries) = self.directories.remove(dir.as_str()) {
            return Ok(entries);
        }
        // Rule discovery has its own depth ceiling. A deeper snapshot still
        // performs its full live walk and enforces its independent limits.
        if let Some(live) = self.live.as_mut() {
            live.entries(dir)
        } else {
            self.source.entries(dir).map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| InventoryEntry { entry, stat: None })
                    .collect()
            })
        }
    }
}

impl Source for PreparedSource {
    fn invalidate_inventory(&self) {
        if let Ok(mut directories) = self.directories.lock() {
            *directories = None;
        }
    }

    fn inventory(&self) -> Option<Box<dyn Inventory + '_>> {
        let directories = self.directories.lock().ok()?.take().unwrap_or_default();
        Some(Box::new(Replay {
            source: &self.source,
            directories,
            live: self.source.inventory(),
        }))
    }
    fn root_kind(&self) -> Result<NodeType, CoreError> {
        self.source.root_kind()
    }
    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, CoreError> {
        self.source.entries(dir)
    }
    fn kind(&self, path: &RelativePath) -> Result<NodeType, CoreError> {
        self.source.kind(path)
    }
    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, CoreError> {
        self.source.metadata(path)
    }
    fn stat(&self, path: &RelativePath) -> Result<(EntryMeta, Option<EntryStamp>), CoreError> {
        self.source.stat(path)
    }
    fn read(&self, grant: &ReadGrant, cap: u64) -> Result<Vec<u8>, CoreError> {
        self.source.read(grant, cap)
    }
}

pub(super) struct RuleWalk<'a> {
    source: &'a FsSource,
    cursor: Option<Box<dyn Inventory + 'a>>,
    pub(super) directories: Directories,
    policy: Option<(usize, SelectionPolicy)>,
}

impl<'a> RuleWalk<'a> {
    pub(super) fn new(source: &'a FsSource) -> Self {
        Self {
            source,
            cursor: source.inventory(),
            directories: Directories::new(),
            policy: None,
        }
    }

    pub(super) fn entries(&mut self, dir: &RelativePath) -> Result<Vec<SourceEntry>, CoreError> {
        let entries = if let Some(cursor) = self.cursor.as_mut() {
            cursor.entries(dir)?
        } else {
            self.source
                .entries(dir)?
                .into_iter()
                .map(|entry| InventoryEntry { entry, stat: None })
                .collect()
        };
        let names = entries.iter().map(|item| item.entry.clone()).collect();
        self.directories.insert(dir.as_str().to_string(), entries);
        Ok(names)
    }

    pub(super) fn excluded(
        &mut self,
        global: Option<&str>,
        local: &str,
        relative: &str,
        name: &str,
    ) -> bool {
        // The concatenated text only grows. Recompile only after a rule was
        // appended, retaining the original sequential DFS selection semantics.
        if self
            .policy
            .as_ref()
            .is_none_or(|(len, _)| *len != local.len())
        {
            self.policy = Some((
                local.len(),
                SelectionPolicy::from_texts(&RuleTexts {
                    global_ignore: global,
                    local_ignore: (!local.is_empty()).then_some(local),
                    ..RuleTexts::default()
                }),
            ));
        }
        self.policy.as_ref().is_some_and(|(_, policy)| {
            policy.decide(&ItemContext {
                relative_path: relative,
                name,
                is_directory: true,
                debug_mode: false,
            }) == FileHandlingDecision::Exclude
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_is_one_use_and_live_reads_and_invalidation_remain_fresh() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("a.rs"), b"first").unwrap();
        let source = FsSource::open(temp.path()).unwrap();
        let mut walk = RuleWalk::new(&source);
        walk.entries(&RelativePath::root()).unwrap();
        let directories = std::mem::take(&mut walk.directories);
        drop(walk);
        let source = PreparedSource::new(source, directories);
        let entries = source
            .inventory()
            .unwrap()
            .entries(&RelativePath::root())
            .unwrap();
        assert_eq!(entries.len(), 1);
        fs::write(temp.path().join("a.rs"), b"second version").unwrap();
        fs::write(temp.path().join("b.rs"), b"new").unwrap();
        let path = RelativePath::parse("a.rs").unwrap();
        assert_eq!(source.stat(&path).unwrap().0.size, 14);
        let grant = ReadGrant::authorize(&path, FileHandlingDecision::Include, true).unwrap();
        assert_eq!(source.read(&grant, 64).unwrap(), b"second version");
        assert_eq!(
            source
                .inventory()
                .unwrap()
                .entries(&RelativePath::root())
                .unwrap()
                .len(),
            2
        );

        let mut walk = RuleWalk::new(&source.source);
        walk.entries(&RelativePath::root()).unwrap();
        *source.directories.lock().unwrap() = Some(std::mem::take(&mut walk.directories));
        drop(walk);
        fs::write(temp.path().join("c.rs"), b"hook-created").unwrap();
        source.invalidate_inventory();
        assert_eq!(
            source
                .inventory()
                .unwrap()
                .entries(&RelativePath::root())
                .unwrap()
                .len(),
            3
        );
    }
}
