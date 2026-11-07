// SPDX-License-Identifier: Apache-2.0
//! Bounded listing of names the ledger does not own.
//! Implements review F14: orphan walks have count, depth and byte budgets.
//! Depends on [`super::LEDGER_FILE`] and [`super::lock::LOCK_FILE`]. Load refuses when a budget trips.
//! Known object files are not charged. A walk that passes the budget returns a typed refusal.

use super::LEDGER_FILE;
use super::lock::{ENTRY_FILE, LOCK_FILE};
use crate::error::CacheError;
use crate::ports::OrphanFile;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Unknown names recorded for one load. The startup slice is a separate bound.
const ORPHAN_MAX_COUNT: usize = 128;
/// Directory components below the cache root. `objects/<id>` is depth 1.
const ORPHAN_MAX_DEPTH: usize = 8;
/// Sum of relative path lengths for charged entries, in bytes.
const ORPHAN_MAX_BYTES: usize = 16 * 1024;

/// Names under `root` that `known` does not own, or a budget refusal.
pub(super) fn list_orphans(
    root: &Path,
    known: &BTreeSet<u64>,
) -> Result<Vec<OrphanFile>, CacheError> {
    let mut found = Vec::new();
    let mut budget = Budget { count: 0, bytes: 0 };
    walk(root, "", 0, known, &mut budget, &mut found)?;
    found.sort();
    Ok(found)
}

struct Budget {
    count: usize,
    bytes: usize,
}

fn walk(
    dir: &Path,
    prefix: &str,
    depth: usize,
    known: &BTreeSet<u64>,
    budget: &mut Budget,
    found: &mut Vec<OrphanFile>,
) -> Result<(), CacheError> {
    if depth > ORPHAN_MAX_DEPTH {
        return Err(budget_error());
    }
    let entries = fs::read_dir(dir).map_err(|_| CacheError::LedgerIo {
        reason: "read directory",
    })?;
    for entry in entries {
        let entry = entry.map_err(|_| CacheError::LedgerIo {
            reason: "read directory",
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let path = entry.path();
        let meta =
            fs::symlink_metadata(&path).map_err(|_| CacheError::LedgerIo { reason: "metadata" })?;
        if relative == LEDGER_FILE
            || relative == LOCK_FILE
            || relative == ENTRY_FILE
            || super::reclaim::is_lease_lock(&relative)
            || known_object(&relative, known)
        {
            continue;
        }
        charge(budget, &relative)?;
        if meta.file_type().is_symlink() {
            found.push(OrphanFile { relative });
            continue;
        }
        if meta.is_dir() {
            if depth >= ORPHAN_MAX_DEPTH {
                return Err(budget_error());
            }
            walk(&path, &relative, depth + 1, known, budget, found)?;
            continue;
        }
        found.push(OrphanFile { relative });
    }
    Ok(())
}

fn charge(budget: &mut Budget, relative: &str) -> Result<(), CacheError> {
    budget.count = budget.count.saturating_add(1);
    budget.bytes = budget.bytes.saturating_add(relative.len());
    if budget.count > ORPHAN_MAX_COUNT || budget.bytes > ORPHAN_MAX_BYTES {
        Err(budget_error())
    } else {
        Ok(())
    }
}

fn budget_error() -> CacheError {
    CacheError::InvalidRequest {
        reason: "orphan walk exceeded its budget",
    }
}

fn known_object(relative: &str, known: &BTreeSet<u64>) -> bool {
    let Some(name) = relative.strip_prefix("objects/") else {
        return false;
    };
    if name.contains('/') {
        return false;
    }
    let Ok(id) = name.parse::<u64>() else {
        return false;
    };
    id.to_string() == name && known.contains(&id)
}

#[cfg(test)]
mod tests {
    use super::{ORPHAN_MAX_BYTES, ORPHAN_MAX_COUNT, ORPHAN_MAX_DEPTH};
    use crate::adapters::FsLedgerStore;
    use crate::error::CacheError;
    use crate::ports::LedgerStore;
    use crate::{LEDGER_MAX_BYTES, Lifecycle, Quota};
    use std::fs::{self, File};
    use std::path::{Path, PathBuf};

    fn root() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().join("cache");
        fs::create_dir(&root).expect("dir");
        (dir, root)
    }

    fn assert_budget(root: &Path) {
        let store = FsLedgerStore::open(root).expect("open");
        let error = store.load(Quota::scaled(10)).expect_err("budget");
        assert!(matches!(
            error,
            CacheError::InvalidRequest {
                reason: "orphan walk exceeded its budget"
            }
        ));
        assert!(
            root.join(super::super::LEDGER_FILE).exists()
                || fs::read_dir(root).expect("dir").next().is_some()
        );
    }

    #[test]
    fn ledger_reads_are_bounded() {
        let (_dir, small) = root();
        let store = FsLedgerStore::open(&small).expect("open");
        fs::write(small.join("ok.txt"), b"ok").expect("note");
        fs::create_dir(small.join("keep")).expect("dir");
        fs::write(small.join("keep/ok.txt"), b"ok").expect("nested");
        let loaded = store.load(Quota::scaled(10)).expect("small");
        assert!(loaded.fault.is_none());
        assert!(loaded.orphans.iter().any(|file| file.relative == "ok.txt"));
        assert!(
            loaded
                .orphans
                .iter()
                .any(|file| file.relative == "keep/ok.txt")
        );

        let (_over, over) = root();
        let ledger = over.join(super::super::LEDGER_FILE);
        let file = File::create(&ledger).expect("create");
        file.set_len(u64::try_from(LEDGER_MAX_BYTES).expect("limit") + 1)
            .expect("len");
        let loaded = FsLedgerStore::open(&over)
            .expect("open")
            .load(Quota::scaled(10))
            .expect("huge");
        assert!(matches!(
            loaded.fault,
            Some(CacheError::CorruptLedger {
                reason: "ledger is too large"
            })
        ));
        assert!(ledger.is_file());
        fs::remove_file(&ledger).expect("remove");
        let life = Lifecycle::new(Quota::scaled(10)).expect("quota");
        FsLedgerStore::open(&over)
            .expect("open")
            .store(&life)
            .expect("store");
        let loaded = FsLedgerStore::open(&over)
            .expect("open")
            .load(Quota::scaled(10))
            .expect("fits");
        assert!(loaded.fault.is_none());
        assert_eq!(loaded.lifecycle.admitted(), 0);

        let (_wide, wide) = root();
        for index in 0..=ORPHAN_MAX_COUNT {
            fs::write(wide.join(format!("n{index}")), b"x").expect("name");
        }
        assert_budget(&wide);

        let (_deep, deep) = root();
        let mut dir = deep.clone();
        for _ in 0..=ORPHAN_MAX_DEPTH {
            dir.push("d");
            fs::create_dir(&dir).expect("nest");
        }
        fs::write(dir.join("leaf"), b"x").expect("leaf");
        assert_budget(&deep);

        let (_bytes, bytes) = root();
        let name = "p".repeat(200);
        let many = ORPHAN_MAX_BYTES / name.len() + 1;
        assert!(many <= ORPHAN_MAX_COUNT);
        for index in 0..many {
            fs::write(bytes.join(format!("{name}{index}")), b"x").expect("long");
        }
        assert_budget(&bytes);
    }
}
