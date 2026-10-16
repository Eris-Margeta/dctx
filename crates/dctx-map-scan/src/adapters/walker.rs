//! Deterministic traversal with decisions before content access.
//! Implements SPEC-MAP-MODE section 13a and ADR-009/015.
//! Uses filesystem ports, pure ignore rules and SHA-256 scan records.

use crate::domain::{
    classifier::{FileContentHandling, classify_file},
    scanned_file::{ScannedFile, content_hash},
};
use crate::ports::fs::{FileKind, FileSystem};
use crate::{FileHandlingDecision, IgnoreEngine, ItemContext};
use std::{
    io,
    path::{Component, Path, PathBuf},
};

/// Explicit failure; incomplete scans never appear successful.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// Filesystem failure at a named path.
    #[error("scan IO at {path:?}: {source}")]
    Io {
        /// Failed path.
        path: PathBuf,
        /// Underlying IO failure.
        #[source]
        source: io::Error,
    },
    /// Invalid child component or non-UTF-8 rule path.
    #[error("invalid scan path: {0:?}")]
    InvalidPath(PathBuf),
}
fn at<T>(path: &Path, result: io::Result<T>) -> Result<T, ScanError> {
    result.map_err(|source| ScanError::Io {
        path: path.into(),
        source,
    })
}

/// Scans regular files in relative-path order, skipping links and special files.
/// Omitted and debug-visible files have metadata but no hash: they are never opened.
/// SVG content is governed by omit rules, allowing explicit rule negation.
pub fn scan(
    root: &Path,
    engine: &IgnoreEngine,
    fs: &impl FileSystem,
    debug_ignored: bool,
) -> Result<Vec<ScannedFile>, ScanError> {
    let kind = at(root, fs.kind(root))?;
    let relative = if kind == FileKind::File {
        PathBuf::from(
            root.file_name()
                .ok_or_else(|| ScanError::InvalidPath(root.into()))?,
        )
    } else {
        PathBuf::new()
    };
    let mut pending = vec![(
        root.to_path_buf(),
        relative,
        kind,
        FileHandlingDecision::Include,
    )];
    let mut records = Vec::new();
    while let Some((disk, relative, kind, parent)) = pending.pop() {
        if kind == FileKind::Other {
            continue;
        }
        let text = relative
            .to_str()
            .ok_or_else(|| ScanError::InvalidPath(relative.clone()))?;
        let name = match relative.file_name() {
            Some(name) => name
                .to_str()
                .ok_or_else(|| ScanError::InvalidPath(relative.clone()))?,
            None => "",
        };
        let decision = if relative.as_os_str().is_empty() {
            parent
        } else {
            engine.decide_with_parent(
                &ItemContext {
                    relative_path: text,
                    name,
                    is_directory: kind == FileKind::Directory,
                    debug_mode: debug_ignored,
                },
                parent,
            )
        };
        if !decision.show_in_tree() {
            continue;
        }
        if kind == FileKind::Directory {
            let mut entries = at(&disk, fs.entries(&disk))?;
            entries.sort_by(|a, b| b.name.cmp(&a.name));
            for entry in entries {
                let child = Path::new(&entry.name);
                let mut components = child.components();
                if !matches!(components.next(), Some(Component::Normal(_)))
                    || components.next().is_some()
                {
                    return Err(ScanError::InvalidPath(child.into()));
                }
                pending.push((disk.join(child), relative.join(child), entry.kind, decision));
            }
        } else {
            let (decision, size, hash) = if decision.should_read_content() {
                let bytes = at(&disk, fs.read_content(&disk))?;
                let final_decision = match classify_file(&relative, &bytes) {
                    FileContentHandling::OmitBinary { .. } => FileHandlingDecision::OmitContent,
                    _ => decision,
                };
                (
                    final_decision,
                    bytes.len() as u64,
                    Some(content_hash(&bytes)),
                )
            } else {
                (decision, at(&disk, fs.size(&disk))?, None)
            };
            records.push(ScannedFile {
                path: relative,
                decision,
                size,
                hash,
            });
        }
    }
    records.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{
        fs::StdFileSystem,
        spy::{Operation, SpyFileSystem},
    };
    use crate::domain::ignore::parse_ignore_pattern_line;
    use std::fs;

    fn engine() -> IgnoreEngine {
        IgnoreEngine::from_rules(
            ["private/", "secret", "!private/child"]
                .into_iter()
                .filter_map(parse_ignore_pattern_line)
                .collect(),
            ["*.lock", "*.svg", "!keep.svg"]
                .into_iter()
                .filter_map(parse_ignore_pattern_line)
                .collect(),
        )
    }
    #[test]
    fn excluded_files_never_open_or_stat_and_directories_never_list() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join("private")).unwrap();
        for name in [
            "secret",
            "private/child",
            "deps.lock",
            "icon.svg",
            "keep.svg",
            "main.rs",
        ] {
            fs::write(root.join(name), b"abc").unwrap();
        }
        let spy = SpyFileSystem::new(StdFileSystem);
        let records = scan(root, &engine(), &spy, false).unwrap();
        assert_eq!(
            records
                .iter()
                .map(|r| r.path.to_str().unwrap())
                .collect::<Vec<_>>(),
            ["deps.lock", "icon.svg", "keep.svg", "main.rs"]
        );
        for name in ["secret", "private/child", "deps.lock", "icon.svg"] {
            assert!(!spy.operations().contains(&Operation::Open(root.join(name))));
        }
        assert!(
            !spy.operations()
                .contains(&Operation::Size(root.join("secret")))
        );
        assert!(
            !spy.operations()
                .contains(&Operation::List(root.join("private")))
        );
        assert!(
            spy.operations()
                .contains(&Operation::Open(root.join("main.rs")))
        );
        assert_eq!(records[0].hash, None);
        assert_eq!(records[0].size, 3);
        assert_eq!(records[2].decision, FileHandlingDecision::Include);
        assert_eq!(records[3].hash, Some(content_hash(b"abc")));
    }
    #[test]
    fn debug_ancestor_protects_even_negated_children() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("private")).unwrap();
        fs::write(dir.path().join("private/child"), b"secret").unwrap();
        fs::write(dir.path().join("public"), b"hello").unwrap();
        let spy = SpyFileSystem::new(StdFileSystem);
        let records = scan(dir.path(), &engine(), &spy, true).unwrap();
        assert_eq!(records[0].decision, FileHandlingDecision::DebugVisible);
        assert_eq!(records[0].hash, None);
        assert_eq!(records[0].size, 6);
        assert_eq!(
            spy.operations()
                .iter()
                .filter(|o| matches!(o, Operation::Open(_)))
                .count(),
            1
        );
    }
    #[test]
    fn binary_empty_and_single_file_roots() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("binary"), b"a\0b").unwrap();
        fs::write(dir.path().join("empty"), b"").unwrap();
        let records = scan(dir.path(), &engine(), &StdFileSystem, false).unwrap();
        assert_eq!(records[0].decision, FileHandlingDecision::OmitContent);
        assert_eq!(records[0].hash, Some(content_hash(b"a\0b")));
        assert_eq!(records[1].hash, Some(content_hash(b"")));
        assert_eq!(
            scan(&dir.path().join("empty"), &engine(), &StdFileSystem, false).unwrap(),
            vec![records[1].clone()]
        );
        assert!(
            scan(
                &dir.path().join("missing"),
                &engine(),
                &StdFileSystem,
                false
            )
            .is_err()
        );
    }

    struct RefusingFs {
        child: &'static str,
        fail_list: bool,
        fail_size: bool,
    }
    impl crate::ports::ContentSource for RefusingFs {
        fn read_content(&self, _: &Path) -> io::Result<Vec<u8>> {
            Err(io::ErrorKind::PermissionDenied.into())
        }
    }
    impl FileSystem for RefusingFs {
        fn kind(&self, _: &Path) -> io::Result<FileKind> {
            Ok(FileKind::Directory)
        }
        fn entries(&self, _: &Path) -> io::Result<Vec<crate::ports::fs::Entry>> {
            if self.fail_list {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            Ok(vec![crate::ports::fs::Entry {
                name: self.child.into(),
                kind: FileKind::File,
            }])
        }
        fn size(&self, _: &Path) -> io::Result<u64> {
            if self.fail_size {
                Err(io::ErrorKind::PermissionDenied.into())
            } else {
                Ok(3)
            }
        }
    }
    #[test]
    fn io_failures_and_invalid_entries_refuse_without_partial_success() {
        let root = Path::new("root");
        for (child, fail_list, fail_size) in [
            ("public", true, false),
            ("public", false, false),
            ("deps.lock", false, true),
        ] {
            let fs = RefusingFs {
                child,
                fail_list,
                fail_size,
            };
            assert!(matches!(
                scan(root, &engine(), &fs, false),
                Err(ScanError::Io { .. })
            ));
        }
        let valid = RefusingFs {
            child: "deps.lock",
            fail_list: false,
            fail_size: false,
        };
        assert!(scan(root, &engine(), &valid, false).is_ok());
        for child in ["../escape", "/absolute", "nested/file", ".", ""] {
            let fs = RefusingFs {
                child,
                fail_list: false,
                fail_size: false,
            };
            assert!(matches!(
                scan(root, &engine(), &fs, false),
                Err(ScanError::InvalidPath(_))
            ));
        }
    }
}
