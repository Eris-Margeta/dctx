// SPDX-License-Identifier: Apache-2.0
//! Handle-relative source access. No pathname lookup can follow a swapped ancestor.
use super::{CoreError, FsSource, map_io, now_ns};
use dctx_core::datatypes::NodeType;
use dctx_core::error::Resource;
use dctx_core::facts::{EntryMeta, EntryStamp, ReadGrant, SourceEntry};
use dctx_core::ids::{RelativePath, Timestamp};
use dctx_ports::Source;
use dctx_ports::source::{Inventory, InventoryEntry};
use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, openat, statat};
use std::fs::File;
use std::io::Read;
use std::path::Path;

const DIRECTORY: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const CONTENT: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC)
    .union(OFlags::NONBLOCK);

fn error(error: rustix::io::Errno) -> CoreError {
    if matches!(error, rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) {
        CoreError::PolicyDenied
    } else {
        map_io(error.into())
    }
}

pub(super) fn open_root(path: &Path) -> Result<File, CoreError> {
    let mut directory = openat(rustix::fs::CWD, "/", DIRECTORY, Mode::empty())
        .map(File::from)
        .map_err(error)?;
    for component in path.components() {
        match component {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(name) => {
                directory = openat(&directory, name, DIRECTORY, Mode::empty())
                    .map(File::from)
                    .map_err(error)?;
            }
            _ => return Err(CoreError::PolicyDenied),
        }
    }
    Ok(directory)
}

fn open_content(parent: &File, name: &str) -> Result<File, CoreError> {
    let file = openat(parent, name, CONTENT, Mode::empty())
        .map(File::from)
        .map_err(error)?;
    if !file.metadata().map_err(map_io)?.is_file() {
        return Err(CoreError::PolicyDenied);
    }
    Ok(file)
}

impl FsSource {
    fn directory(&self, path: &RelativePath) -> Result<File, CoreError> {
        let mut directory = self.directory.try_clone().map_err(map_io)?;
        if !path.is_root() {
            for name in path.as_str().split('/') {
                directory = openat(&directory, name, DIRECTORY, Mode::empty())
                    .map(File::from)
                    .map_err(error)?;
            }
        }
        Ok(directory)
    }

    fn parent(&self, path: &RelativePath) -> Result<(File, String), CoreError> {
        let (parent, name) = path
            .as_str()
            .rsplit_once('/')
            .unwrap_or(("", path.as_str()));
        Ok((
            self.directory(&RelativePath::parse(parent)?)?,
            name.to_string(),
        ))
    }

    fn content(&self, path: &RelativePath) -> Result<File, CoreError> {
        let (parent, name) = self.parent(path)?;
        open_content(&parent, &name)
    }

    fn source_stat(&self, path: &RelativePath) -> Result<rustix::fs::Stat, CoreError> {
        if path.is_root() {
            rustix::fs::fstat(&*self.directory).map_err(error)
        } else {
            let (parent, name) = self.parent(path)?;
            statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(error)
        }
    }
}

impl FsSource {
    fn entries_bounded(
        &self,
        path: &RelativePath,
        cap: u64,
    ) -> Result<Vec<SourceEntry>, CoreError> {
        let directory = self.directory(path)?;
        let mut reader = Dir::read_from(&directory).map_err(error)?;
        Ok(list_directory(&mut reader, cap)?
            .into_iter()
            .map(|item| item.entry)
            .collect())
    }
}

fn list_directory(reader: &mut Dir, cap: u64) -> Result<Vec<InventoryEntry>, CoreError> {
    let mut entries = Vec::new();
    let mut visited = 0u64;
    while let Some(entry) = reader.read() {
        let entry = entry.map_err(error)?;
        let name = entry.file_name().to_bytes();
        if matches!(name, b"." | b"..") {
            continue;
        }
        // Count even unrepresentable names before allocating the inventory.
        visited += 1;
        if visited > cap {
            return Err(CoreError::ResourceLimit {
                resource: Resource::Entries,
                limit: cap,
            });
        }
        let Ok(name) = std::str::from_utf8(name) else {
            continue;
        };
        let now = now_ns();
        let kind =
            statat(reader.fd().map_err(error)?, name, AtFlags::SYMLINK_NOFOLLOW).map_err(error)?;
        entries.push(InventoryEntry {
            stat: Some(stamped_meta(&kind, now)),
            entry: if FileType::from_raw_mode(kind.st_mode) == FileType::Directory {
                SourceEntry::directory(name)
            } else {
                SourceEntry::file(name)
            },
        });
    }
    Ok(entries)
}

/// A DFS stack pins each ancestor once. Moving to a sibling drops only the
/// finished subtree; opening a child always uses its pinned parent and NOFOLLOW.
struct FsWalk {
    root: std::sync::Arc<File>,
    stack: Vec<(RelativePath, Dir)>,
}

impl Inventory for FsWalk {
    fn entries(&mut self, path: &RelativePath) -> Result<Vec<InventoryEntry>, CoreError> {
        if self.stack.is_empty() {
            self.stack.push((
                RelativePath::root(),
                Dir::read_from(&*self.root).map_err(error)?,
            ));
        }
        while self.stack.last().is_some_and(|(parent, _)| {
            !parent.is_root()
                && !path
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
            let parent = self.stack.last().ok_or(CoreError::SourceUnavailable)?;
            let child = openat(
                parent.1.fd().map_err(error)?,
                name,
                DIRECTORY,
                Mode::empty(),
            )
            .map_err(error)?;
            prefix = prefix.child(name)?;
            self.stack
                .push((prefix.clone(), Dir::new(child).map_err(error)?));
        }
        let (_, reader) = self.stack.last_mut().ok_or(CoreError::SourceUnavailable)?;
        reader.rewind();
        let cap = crate::domain::entries_ceiling().map_err(|_| CoreError::InvalidRequest {
            reason: "invalid DCTX_MAX_ENTRIES ceiling",
        })?;
        list_directory(reader, cap)
    }
}

#[allow(
    clippy::useless_conversion,
    reason = "Unix stat timestamp fields have different signedness across targets."
)]
fn entry_meta(meta: &rustix::fs::Stat) -> EntryMeta {
    EntryMeta {
        size: u64::try_from(meta.st_size).unwrap_or(u64::MAX),
        modified: Timestamp(
            u64::try_from(meta.st_mtime)
                .unwrap_or(0)
                .saturating_mul(1000)
                .saturating_add(u64::try_from(meta.st_mtime_nsec).unwrap_or(0) / 1_000_000),
        ),
    }
}

impl Source for FsSource {
    fn inventory(&self) -> Option<Box<dyn Inventory + '_>> {
        Some(Box::new(FsWalk {
            root: self.directory.clone(),
            stack: Vec::new(),
        }))
    }

    fn root_kind(&self) -> Result<NodeType, CoreError> {
        Ok(NodeType::Directory)
    }

    fn entries(&self, path: &RelativePath) -> Result<Vec<SourceEntry>, CoreError> {
        let cap = crate::domain::entries_ceiling().map_err(|_| CoreError::InvalidRequest {
            reason: "invalid DCTX_MAX_ENTRIES ceiling",
        })?;
        self.entries_bounded(path, cap)
    }

    fn kind(&self, path: &RelativePath) -> Result<NodeType, CoreError> {
        Ok(
            if FileType::from_raw_mode(self.source_stat(path)?.st_mode) == FileType::Directory {
                NodeType::Directory
            } else {
                NodeType::File
            },
        )
    }

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, CoreError> {
        Ok(entry_meta(&self.source_stat(path)?))
    }

    fn stat(&self, path: &RelativePath) -> Result<(EntryMeta, Option<EntryStamp>), CoreError> {
        let now = now_ns();
        Ok(stamped_meta(&self.source_stat(path)?, now))
    }

    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, CoreError> {
        let file = self.content(grant.path())?;
        let too_large = || CoreError::ResourceLimit {
            resource: Resource::FileBytes,
            limit: max_bytes,
        };
        if file.metadata().map_err(map_io)?.len() > max_bytes {
            return Err(too_large());
        }
        let mut bytes = Vec::new();
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(map_io)?;
        if bytes.len() as u64 > max_bytes {
            return Err(too_large());
        }
        Ok(bytes)
    }
}

#[allow(
    clippy::useless_conversion,
    clippy::unnecessary_fallible_conversions,
    reason = "Unix stat fields have different signedness and widths across targets."
)]
fn stamped_meta(meta: &rustix::fs::Stat, now: u128) -> (EntryMeta, Option<EntryStamp>) {
    let entry = entry_meta(meta);
    let modified_ns = u128::try_from(meta.st_mtime).ok().and_then(|seconds| {
        Some(seconds * 1_000_000_000 + u128::try_from(meta.st_mtime_nsec).ok()?)
    });
    let stamp = modified_ns
        .filter(|mtime| *mtime != 0 && mtime.saturating_add(super::RACY_WINDOW_NS) <= now)
        .filter(|_| FileType::from_raw_mode(meta.st_mode) == FileType::RegularFile)
        .map(|modified_ns| EntryStamp {
            size: entry.size,
            modified_ns,
            inode: u64::try_from(meta.st_ino).unwrap_or(0),
        });
    (entry, stamp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_core::FileHandlingDecision;
    use std::fs;
    use std::os::unix::fs::symlink;

    fn grant(path: &str) -> ReadGrant {
        ReadGrant::authorize(
            &RelativePath::parse(path).unwrap(),
            FileHandlingDecision::Include,
            true,
        )
        .unwrap()
    }

    #[test]
    fn inventory_pins_ancestors_and_refuses_unopened_replacement_links() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("sub/deeper")).unwrap();
        fs::write(root.path().join("sub/deeper/inside.rs"), b"inside").unwrap();
        fs::write(outside.path().join("OUTSIDE_SENTINEL.rs"), b"outside").unwrap();
        let source = FsSource::open(root.path()).unwrap();
        let mut cursor = source.inventory().unwrap();
        cursor
            .entries(&RelativePath::parse("sub").unwrap())
            .unwrap();
        fs::rename(root.path().join("sub"), root.path().join("old")).unwrap();
        symlink(outside.path(), root.path().join("sub")).unwrap();
        let entries = cursor
            .entries(&RelativePath::parse("sub/deeper").unwrap())
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry.name, "inside.rs");
        assert_eq!(entries[0].stat.unwrap().0.size, 6);
        assert!(
            source
                .stat(&RelativePath::parse("sub/deeper/inside.rs").unwrap())
                .is_err()
        );
        // A new traversal cannot enter the replacement link, even though an
        // earlier traversal pinned the original ancestor.
        assert!(
            source
                .inventory()
                .unwrap()
                .entries(&RelativePath::parse("sub").unwrap())
                .is_err()
        );
        cursor.entries(&RelativePath::root()).unwrap();
        assert!(
            cursor
                .entries(&RelativePath::parse("sub").unwrap())
                .is_err()
        );
        fs::remove_file(root.path().join("sub")).unwrap();
        fs::rename(root.path().join("old"), root.path().join("sub")).unwrap();
        fs::remove_file(root.path().join("sub/deeper/inside.rs")).unwrap();
        symlink(
            outside.path().join("OUTSIDE_SENTINEL.rs"),
            root.path().join("sub/deeper/inside.rs"),
        )
        .unwrap();
        let entries = cursor
            .entries(&RelativePath::parse("sub/deeper").unwrap())
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].stat.unwrap().1,
            None,
            "a link never certifies a stamp"
        );
        assert!(source.read(&grant("sub/deeper/inside.rs"), 64).is_err());
    }

    #[test]
    fn a_swapped_ancestor_cannot_redirect_an_open_directory_or_a_later_read() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("sub")).unwrap();
        fs::write(root.path().join("sub/a.rs"), b"inside").unwrap();
        fs::write(outside.path().join("a.rs"), b"OUTSIDE_SENTINEL").unwrap();
        let source = FsSource::open(root.path()).unwrap();
        let parent = source
            .directory(&RelativePath::parse("sub").unwrap())
            .unwrap();
        fs::rename(root.path().join("sub"), root.path().join("old")).unwrap();
        symlink(outside.path(), root.path().join("sub")).unwrap();
        let mut bytes = Vec::new();
        open_content(&parent, "a.rs")
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, b"inside");
        assert_eq!(
            source.read(&grant("sub/a.rs"), 64).unwrap_err(),
            CoreError::PolicyDenied
        );
        assert!(
            source
                .entries(&RelativePath::parse("sub").unwrap())
                .is_err()
        );
        assert!(
            source
                .metadata(&RelativePath::parse("sub/a.rs").unwrap())
                .is_err()
        );
        assert!(
            source
                .stat(&RelativePath::parse("sub/a.rs").unwrap())
                .is_err()
        );
        assert!(
            source
                .kind(&RelativePath::parse("sub/a.rs").unwrap())
                .is_err()
        );
    }

    #[test]
    fn root_and_final_component_swaps_never_follow_the_replacement_link() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let outside = temp.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(root.join("a.rs"), b"inside").unwrap();
        fs::write(outside.join("a.rs"), b"OUTSIDE_SENTINEL").unwrap();
        let source = FsSource::open(&root).unwrap();
        fs::rename(&root, temp.path().join("old")).unwrap();
        symlink(&outside, &root).unwrap();
        assert_eq!(source.read(&grant("a.rs"), 64).unwrap(), b"inside");
        fs::remove_file(temp.path().join("old/a.rs")).unwrap();
        symlink(outside.join("a.rs"), temp.path().join("old/a.rs")).unwrap();
        assert_eq!(
            source.read(&grant("a.rs"), 64).unwrap_err(),
            CoreError::PolicyDenied
        );
    }

    #[test]
    fn sparse_files_are_limited_by_logical_size_and_fifos_do_not_block() {
        let root = tempfile::tempdir().unwrap();
        let size = if cfg!(target_os = "linux") {
            1 << 40
        } else {
            9 * 1024 * 1024
        };
        fs::File::create(root.path().join("sparse"))
            .unwrap()
            .set_len(size)
            .unwrap();
        let source = FsSource::open(root.path()).unwrap();
        assert!(matches!(
            source.read(&grant("sparse"), 64),
            Err(CoreError::ResourceLimit {
                resource: Resource::FileBytes,
                limit: 64
            })
        ));
        // rustix has no mkfifoat on Apple targets; the POSIX mkfifo utility exists on every Unix CI host.
        let made = std::process::Command::new("mkfifo")
            .args(["-m", "600"])
            .arg(root.path().join("pipe"))
            .status()
            .unwrap();
        assert!(made.success());
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || send.send(source.read(&grant("pipe"), 64)).unwrap());
        assert_eq!(
            receive
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
                .unwrap_err(),
            CoreError::PolicyDenied
        );
    }

    #[test]
    fn directory_collection_checks_the_cap_before_building_an_unbounded_list() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a"), b"").unwrap();
        fs::write(root.path().join("b"), b"").unwrap();
        let source = FsSource::open(root.path()).unwrap();
        assert!(matches!(
            source.entries_bounded(&RelativePath::root(), 1),
            Err(CoreError::ResourceLimit {
                resource: Resource::Entries,
                limit: 1
            })
        ));
        assert_eq!(
            source
                .entries_bounded(&RelativePath::root(), 2)
                .unwrap()
                .len(),
            2
        );
    }
}
