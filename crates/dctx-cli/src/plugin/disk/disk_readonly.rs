// SPDX-License-Identifier: Apache-2.0
//! Bounded Windows registry reads without creating a store or resolving an artifact target.
//! Reparse points are refused; like the portable Unix adapter, parent checks have a
//! residual race against hostile code running as the same user.

use std::fs::{self, File, Metadata};
use std::io::{self, Read};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use dctx_plugin_distribution::DistributionError;

const REGISTRY_LIMIT: u64 = 16 << 20;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

fn plain(meta: &Metadata) -> bool {
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
}

fn root_path(root: &Path) -> io::Result<PathBuf> {
    let absolute = std::path::absolute(root)?;
    let mut cursor = PathBuf::new();
    for part in absolute.components() {
        cursor.push(part);
        if matches!(part, std::path::Component::Prefix(_)) {
            continue;
        }
        let meta = fs::symlink_metadata(&cursor)?;
        if !meta.is_dir() || !plain(&meta) {
            return Err(io::Error::other("non-directory or reparse point"));
        }
    }
    Ok(absolute)
}

pub(super) fn load_registry(root: &Path) -> Result<Option<Vec<u8>>, DistributionError> {
    let root = match root_path(root) {
        Ok(root) => root,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(DistributionError::Interrupted),
    };
    let path = root.join("registry.json");
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(DistributionError::Interrupted),
    };
    check_file(&meta)?;
    let file = super::disk(
        File::options()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path),
    )?;
    check_file(&super::disk(file.metadata())?)?;
    super::disk(root_path(&root))?;
    let mut bytes = Vec::new();
    super::disk(file.take(REGISTRY_LIMIT + 1).read_to_end(&mut bytes))?;
    check_size(u64::try_from(bytes.len()).unwrap_or(u64::MAX))?;
    Ok(Some(bytes))
}

fn check_file(meta: &Metadata) -> Result<(), DistributionError> {
    if !meta.is_file() || !plain(meta) {
        return Err(DistributionError::Interrupted);
    }
    check_size(meta.len())
}

fn check_size(size: u64) -> Result<(), DistributionError> {
    if size > REGISTRY_LIMIT {
        return Err(DistributionError::LimitExceeded {
            limit: REGISTRY_LIMIT,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::disk::FsStore;
    use dctx_plugin_distribution::Store;

    #[test]
    fn registry_reads_are_bounded_and_missing_roots_have_no_effects() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("plugins");
        let store = FsStore::new(&root);
        assert_eq!(store.load_registry().unwrap(), None);
        assert!(!root.exists());
        fs::create_dir(&root).unwrap();
        assert_eq!(store.load_registry().unwrap(), None);
        let path = root.join("registry.json");
        fs::write(&path, b"{}").unwrap();
        assert_eq!(store.load_registry().unwrap(), Some(b"{}".to_vec()));
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(REGISTRY_LIMIT + 1)
            .unwrap();
        assert_eq!(
            store.load_registry(),
            Err(DistributionError::LimitExceeded {
                limit: REGISTRY_LIMIT
            })
        );
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert_eq!(store.load_registry(), Err(DistributionError::Interrupted));
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    }
}
