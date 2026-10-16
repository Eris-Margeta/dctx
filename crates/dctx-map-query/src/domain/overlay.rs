//! Paths added, edited, or removed since the clean base generation.
//! Implements ARCHITECTURE-PROPOSAL DX-C5 overlay on a content snapshot.
//! Query rendering does not read this set; wire dirty counts stay on the snapshot.
use crate::ports::Snapshot;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Source paths that differ from the last clean base.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overlay {
    /// Paths present in both generations whose content hash changed.
    pub dirty: BTreeSet<String>,
    /// Paths that exist only in the new generation.
    pub added: BTreeSet<String>,
    /// Paths that existed only in the previous generation.
    pub deleted: BTreeSet<String>,
}

impl Overlay {
    /// The first commit is a clean base. Later commits classify path changes.
    pub(crate) fn observe(ready: bool, previous: Option<&Snapshot>, next: &Snapshot) -> Self {
        if !ready {
            return Self::default();
        }
        let Some(previous) = previous else {
            return Self::default();
        };
        let mut overlay = Self::default();
        for (path, file) in &next.files {
            match previous.files.get(path) {
                None => {
                    overlay.added.insert(path.clone());
                }
                Some(old) if old.hash != file.hash => {
                    overlay.dirty.insert(path.clone());
                }
                Some(_) => {}
            }
        }
        for path in previous.files.keys() {
            if !next.files.contains_key(path) {
                overlay.deleted.insert(path.clone());
            }
        }
        overlay
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{FileStamp, SourceFile};
    use std::collections::BTreeMap;

    fn file(hash: &str) -> SourceFile {
        SourceFile {
            bytes: None,
            hash: hash.into(),
            size: 1,
            mtime: 1,
            readable: true,
            allowed: true,
            stamp: FileStamp::default(),
        }
    }

    fn snapshot(pairs: &[(&str, &str)]) -> Snapshot {
        Snapshot {
            files: pairs
                .iter()
                .map(|(path, hash)| ((*path).to_string(), file(hash)))
                .collect(),
            revision: String::new(),
            dirty: BTreeSet::new(),
            recent: BTreeSet::new(),
            directories: BTreeMap::new(),
            rules: BTreeMap::new(),
        }
    }

    #[test]
    fn clean_base_then_dirty_added_and_deleted() {
        let base = snapshot(&[("a.rs", "aaa"), ("b.rs", "bbb")]);
        assert_eq!(Overlay::observe(false, None, &base), Overlay::default());
        let next = snapshot(&[("a.rs", "ccc"), ("c.rs", "ddd")]);
        let overlay = Overlay::observe(true, Some(&base), &next);
        assert_eq!(overlay.dirty.iter().collect::<Vec<_>>(), ["a.rs"]);
        assert_eq!(overlay.added.iter().collect::<Vec<_>>(), ["c.rs"]);
        assert_eq!(overlay.deleted.iter().collect::<Vec<_>>(), ["b.rs"]);
    }
}
